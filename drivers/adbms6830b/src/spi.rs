//! Driver for talking to ADBMS6830B devices over SPI.
//! 
//! See the `Adbms6830b` struct. That is the main guy here

use embedded_hal::spi::{Operation, SpiDevice};

use crate::chip::pec::{DataPecRx, DataPecTx};
use crate::chip::registers::{ReadableGroup, WritableGroup, GROUP_BYTES};

/// Size in bytes of one device's data block.
pub const BLOCK_BYTES: usize = GROUP_BYTES + 2;

/// Size of a command frame (CMD0, CMD1, PEC0, PEC1).
pub const COMMAND_BYTES: usize = 4;

/// Errors returned by the driver.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Error<E> {
    /// More devices were passed than the chain was configured for.
    TooManyDevices,
    /// The underlying SPI transaction failed.
    Spi(E),
}

/// Per-device results of a chain read.
/// 
/// To access the results for each device, use the `.device()` function.
///
/// Indexed with 0 being the device nearest the host. `Response::device` returns `None` for a
/// device whose data PEC failed, so one bad device does not invalidate the rest of the chain.
pub trait Response {
    /// The register group this response carries.
    type Group: ReadableGroup;

    /// Number of devices this response covers.
    fn len(&self) -> usize;

    /// Whether this response covers no devices.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the data read from a chip.
    /// 
    /// ### Parameters
    /// - `index`: The index of the chip whose data you want to read. `0` corresponds to the
    /// closest chip to the host.
    /// 
    /// This function will either return the data read from that chip, or `None` if its PEC failed (or the index is out of range).
    fn device(&self, index: usize) -> Option<Self::Group>;

    /// Command counter (`CCNT[5:0]`) reported by device `index`.
    ///
    /// This will return `None` if the index is out of bounds.
    fn command_counter(&self, index: usize) -> Option<u8>;

    /// Indices of devices whose data PEC failed.
    fn failures(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.len()).filter(|&i| self.device(i).is_none())
    }

    /// Whether every device passed its PEC check.
    fn all_ok(&self) -> bool {
        (0..self.len()).all(|i| self.device(i).is_some())
    }

    /// Data from every device (with `None` where a PEC failed).
    /// 
    /// Note: The data from the closest chip to the host comes first.
    fn iter(&self) -> impl Iterator<Item = Option<Self::Group>> + '_ {
        (0..self.len()).map(|i| self.device(i))
    }
}

/// A `Response` that owns the bytes it was read into.
pub struct ResponseBuffer<G, const NUM_CHIPS: usize> {
    blocks: [[u8; BLOCK_BYTES]; NUM_CHIPS],
    used: usize,
    _group: core::marker::PhantomData<G>,
}

impl<G: ReadableGroup, const NUM_CHIPS: usize> ResponseBuffer<G, NUM_CHIPS> {
    const fn empty(used: usize) -> Self {
        Self {
            blocks: [[0; BLOCK_BYTES]; NUM_CHIPS],
            used,
            _group: core::marker::PhantomData,
        }
    }

    fn block(&self, index: usize) -> Option<&[u8; BLOCK_BYTES]> {
        (index < self.used).then(|| &self.blocks[index])
    }
}

impl<G: ReadableGroup, const NUM_CHIPS: usize> Response for ResponseBuffer<G, NUM_CHIPS> {
    type Group = G;

    fn len(&self) -> usize {
        self.used
    }

    fn device(&self, index: usize) -> Option<G> {
        let block = self.block(index)?;
        let mut data = [0u8; GROUP_BYTES];
        data.copy_from_slice(&block[..GROUP_BYTES]);
        let pec = DataPecRx::from_bytes([block[GROUP_BYTES], block[GROUP_BYTES + 1]]);
        pec.verify(&data).then(|| G::from_bytes(data))
    }

    fn command_counter(&self, index: usize) -> Option<u8> {
        let block = self.block(index)?;
        Some(DataPecRx::from_bytes([block[GROUP_BYTES], block[GROUP_BYTES + 1]]).ccnt())
    }
}

/// A daisy chain of `NUM_CHIPS` ADBMS6830B devices on one SPI bus.
pub struct Adbms6830b<SPI, const NUM_CHIPS: usize> {
    spi: SPI,
}

impl<SPI: SpiDevice, const NUM_CHIPS: usize> Adbms6830b<SPI, NUM_CHIPS> {
    /// Builds a driver over `spi`.
    pub const fn new(spi: SPI) -> Self {
        Self { spi }
    }

    /// Number of devices this chain is configured for.
    pub const fn num_chips(&self) -> usize {
        NUM_CHIPS
    }

    /// Releases the underlying SPI device.
    pub fn release(self) -> SPI {
        self.spi
    }

    /// Writes one register group per device.
    ///
    /// ### Parameters
    /// - `devices`: The list of data you want to write, with each index coresponding
    /// a chip in the daisy chain. `devices[0]` is the device nearest to the host, so tou should
    /// orient your list as such. If you have no chain and are just writing to one chip, you can just
    /// pass in a slice with a length of one.
    ///
    /// ### Examples
    /// Here's an example of how this function might be used:
    /// ```ignore
    /// // Build the same ConfigB for every chip.
    /// let config_b = ConfigB::default()
    ///     .with_vuv(UndervoltageThreshold::from_microvolts(3_000_000).unwrap())
    ///     .with_vov(OvervoltageThreshold::from_microvolts(4_200_000).unwrap());
    ///
    ///
    /// let configs = [config_b; NUM_CHIPS]; // (Index 0 is the chip closest to the host.)
    ///
    /// // Write all chips' ConfigB registers.
    /// match adbms.write(&configs) {
    ///     Ok(()) => info!("Wrote ConfigB to {} chips", configs.len()),
    ///     Err(err) => { warn!("evil error: {}", err); return; }
    /// }
    /// ```
    /// 
    /// If the reachable chain is shorter than `NUM_CHIPS` (maybe after a COMM_BK split), you can pass
    /// a shorter slice to only write to those chips:
    /// ```ignore
    /// // Create PwmB configs for the three chips reachable on this segment of the chain.
    /// let configs: [PwmB; 3] = [ 
    ///     PwmB::default().with_pwm13(PwmDutyCycleConfig::Pct26_4),
    ///     PwmB::default().with_pwm15(PwmDutyCycleConfig::Pct39_6),
    ///     PwmB::default().with_pwm14(PwmDutyCycleConfig::Pct72_6),
    /// ];
    ///
    /// // Write these configs to the three chips on this segment.
    /// match adbms.write(&configs) {
    ///     Ok(()) => info!("Wrote PwmB to {} chips", configs.len()),
    ///     Err(err) => { warn!("evil error: {}", err); return; }
    /// }
    /// ```
    pub fn write<G: WritableGroup>(&mut self, devices: &[G]) -> Result<(), Error<SPI::Error>> {
        let n = devices.len();
        if n > NUM_CHIPS {
            return Err(Error::TooManyDevices);
        }

        let mut blocks = [[0u8; BLOCK_BYTES]; NUM_CHIPS];
        for (i, group) in devices.iter().enumerate() {
            let block = &mut blocks[n - 1 - i];
            let data = group.to_bytes();
            block[..GROUP_BYTES].copy_from_slice(&data);
            let pec = DataPecTx::new(&data);
            block[GROUP_BYTES] = pec.pec0();
            block[GROUP_BYTES + 1] = pec.pec1();
        }

        self.spi
            .transaction(&mut [
                Operation::Write(&G::WRITE_COMMAND.to_bytes()),
                Operation::Write(&blocks.as_flattened()[..n * BLOCK_BYTES]),
            ])
            .map_err(Error::Spi)
    }

    /// Reads a register group from a chain of devices.
    /// 
    /// ### Parameters
    /// - `count`: The number of devices you want to read. `1` would mean that you
    /// only want to read the closest chip to the host (microcontroller). `2` would mean that
    /// you want to read the first two closest chips to the host. `3` would mean you want to read
    /// the first three closest chips to the host. You get the idea
    /// 
    /// ### Examples
    /// Here's an example of how this function might be used:
    /// ```ignore
    /// // Read the three closest chips' CellVoltagesA registers.
    /// let responses = match adbms.read::<CellVoltagesA>(3) {
    ///     Ok(response) => response,
    ///     Err(err) => { warn!("evil error: {}", err); return; }
    /// };
    ///
    /// // Loop through the returned responses for each chip.
    /// for (index, response) in responses.iter().enumerate() {
    ///     // Check each chip for PEC errors.
    ///     let cells_a: CellVoltagesA = match response {
    ///         None => {
    ///             warn!("PEC error when reading chip {}!!!", index);
    ///             return;
    ///         },
    ///         Some(cells_a) => cells_a,
    ///     };
    ///
    ///     // Log the data from each chip's CellVoltagesA register.
    ///     info!("Chip {}: Cell 1 voltage: {} uV", index, cells_a.c1v().as_microvolts());
    ///     info!("Chip {}: Cell 2 voltage: {} uV", index, cells_a.c2v().as_microvolts());
    ///     info!("Chip {}: Cell 3 voltage: {} uV", index, cells_a.c3v().as_microvolts());
    /// }
    /// ```
    pub fn read<G: ReadableGroup>(&mut self, count: usize) -> Result<impl Response<Group = G>, Error<SPI::Error>> {
        if count > NUM_CHIPS {
            return Err(Error::TooManyDevices);
        }

        let mut response = ResponseBuffer::<G, NUM_CHIPS>::empty(count);
        self.spi.transaction(&mut [
                Operation::Write(&G::READ_COMMAND.to_bytes()),
                Operation::Read(&mut response.blocks.as_flattened_mut()[..count * BLOCK_BYTES]),
            ])
            .map_err(Error::Spi)?;

        Ok(response)
    }

    /// Reads one register group from every device in the chain.
    /// 
    /// ### Examples
    /// Here's an example of how this function might be used:
    /// ```ignore
    /// // Read all chips' StatusB registers.
    /// let responses = match adbms.read_all::<StatusB>() {
    ///     Ok(response) => response,
    ///     Err(err) => { warn!("evil error: {}", err); return; }
    /// };
    ///
    /// // Loop through the returned responses for each chip.
    /// for (index, response) in responses.iter().enumerate() {
    ///     // Check each chip for PEC errors.
    ///     let status_b: StatusB = match response {
    ///         None => {
    ///             warn!("PEC error when reading chip {}!!!", index);
    ///             return;
    ///         },
    ///         Some(status_b) => status_b,
    ///     };
    ///     
    ///     // Log the data from each chip's StatusB register.
    ///     info!("Chip {}: Digital power supply voltage: {} uV", index, status_b.vd().as_microvolts());
    ///     info!("Chip {}: Analog power supply voltage: {} uV", index, status_b.va().as_microvolts());
    ///     info!("Chip {}: VREF2 across resistor: {} uV", index, status_b.vres().as_microvolts());
    /// }
    /// ```
    pub fn read_all<G: ReadableGroup>(&mut self,) -> Result<impl Response<Group = G>, Error<SPI::Error>> {
        self.read::<G>(NUM_CHIPS)
    }
}