//! Driver for talking to a single line of daisy-chained ADBMS6830B devices over SPI.
//!
//! See the `Chain` struct. That is the main guy here

use core::marker::PhantomData;

use embedded_hal::spi::{Operation, SpiDevice};

use crate::chip::pec::{DataPecRx, DataPecTx};
use crate::chip::registers::{ReadableGroup, WritableGroup, GROUP_BYTES};

/// Size in bytes of one device's data block.
const BLOCK_BYTES: usize = GROUP_BYTES + 2;

/// Size of a command frame (CMD0, CMD1, PEC0, PEC1).
#[allow(dead_code)]
const COMMAND_BYTES: usize = 4;

/// Largest chip count any `Chain` can be configured for.
///
/// This is a compile-time upper bound rather than the actual chain length.
/// You can customize this value by setting the `ADBMS6830B_MAX_CHIPS` environment variable (maybe in your .cargo/config.toml)
pub const MAX_CHIPS: usize = match option_env!("ADBMS6830B_MAX_CHIPS") {
    Some(text) => parse_max_chips(text),
    None => 10, // default!
};

/// Parses the `ADBMS6830B_MAX_CHIPS` env var at compile time.
const fn parse_max_chips(text: &str) -> usize {
    let bytes = text.as_bytes();
    let mut value = 0usize;
    let mut i = 0;
    while i < bytes.len() {
        let digit = bytes[i];
        assert!(
            digit >= b'0' && digit <= b'9',
            "ADBMS6830B_MAX_CHIPS must be a decimal number"
        );
        value = value * 10 + (digit - b'0') as usize;
        i += 1;
    }
    assert!(value > 0, "ADBMS6830B_MAX_CHIPS must be greater than zero");
    value
}

/// Errors returned by the driver.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Error<E> {
    /// More devices were asked for than the chain holds.
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
pub struct Response<G> {
    blocks: [[u8; BLOCK_BYTES]; MAX_CHIPS],
    used: usize,
    _group: PhantomData<G>,
}

impl<G: ReadableGroup> Response<G> {
    /// Builds an empty response covering `used` devices.
    const fn empty(used: usize) -> Self {
        Self {
            blocks: [[0; BLOCK_BYTES]; MAX_CHIPS],
            used,
            _group: PhantomData,
        }
    }

    fn block(&self, index: usize) -> Option<&[u8; BLOCK_BYTES]> {
        (index < self.used).then(|| &self.blocks[index])
    }

    /// Number of devices this response covers.
    pub fn len(&self) -> usize {
        self.used
    }

    /// Whether this response covers no devices.
    pub fn is_empty(&self) -> bool {
        self.used == 0
    }

    /// Returns the data read from a chip.
    ///
    /// ### Parameters
    /// - `index`: The index of the chip whose data you want to read. `0` corresponds to the
    /// closest chip to the host.
    ///
    /// This function will either return the data read from that chip, or `None` if its PEC failed (or the index is out of range).
    pub fn device(&self, index: usize) -> Option<G> {
        let block = self.block(index)?;
        let mut data = [0u8; GROUP_BYTES];
        data.copy_from_slice(&block[..GROUP_BYTES]);
        let pec = DataPecRx::from_bytes([block[GROUP_BYTES], block[GROUP_BYTES + 1]]);
        pec.verify(&data).then(|| G::from_bytes(data))
    }

    /// Command counter (`CCNT[5:0]`) reported by device `index`.
    ///
    /// This will return `None` if the index is out of bounds.
    pub fn command_counter(&self, index: usize) -> Option<u8> {
        let block = self.block(index)?;
        Some(DataPecRx::from_bytes([block[GROUP_BYTES], block[GROUP_BYTES + 1]]).ccnt())
    }

    /// Indices of devices whose data PEC failed.
    pub fn failures(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.used).filter(|&i| self.device(i).is_none())
    }

    /// Whether every device passed its PEC check.
    pub fn all_ok(&self) -> bool {
        (0..self.used).all(|i| self.device(i).is_some())
    }

    /// Data from every device (with `None` where a PEC failed).
    ///
    /// Note: The data from the closest chip to the host comes first.
    pub fn iter(&self) -> impl Iterator<Item = Option<G>> + '_ {
        (0..self.used).map(|i| self.device(i))
    }
}

/// One isoSPI line carrying a daisy chain of ADBMS6830B devices.
pub struct Chain<SPI> {
    spi: SPI,
    num_chips: usize,
}

impl<SPI: SpiDevice> Chain<SPI> {
    /// Builds a chain of `num_chips` devices on `spi`.
    ///
    /// Returns `TooManyDevices` if `num_chips` exceeds `MAX_CHIPS`.
    pub fn new(spi: SPI, num_chips: usize) -> Result<Self, Error<SPI::Error>> {
        if num_chips > MAX_CHIPS {
            return Err(Error::TooManyDevices);
        }
        Ok(Self { spi, num_chips })
    }

    /// Number of devices currently on this chain.
    pub fn num_chips(&self) -> usize {
        self.num_chips
    }

    /// u_TODO: Maybe future function:
    /// once the serial ID register groups are implemented we could have a `pub fn detect_num_chips(&mut self) -> Result<usize, Error<SPI::Error>>` that tells you how
    /// many chips are actually on the chain. This might be helpful for debugging or even some automatic state detection stuff? Or possibly just an occasional sanity check against the manually
    /// tracked `num_chips()` stuff that the application has to manage. Manually doing state tracking is kind of annoying though so it would be nice to make it
    /// automatic and error-proof somehow. See the u_Note about the possible `.split()` function so we wouldn't have to manually manage the num chips

    /// Changes how many devices this chain holds.
    ///
    /// You can use this after a COMM_BK split moves devices onto the other line. Returns `TooManyDevices`
    /// if `num_chips` exceeds `MAX_CHIPS`.
    /// 
    /// u_Note: in the future it could be nice to have just like a `.split()` function that sets `COMM_BK` for you and returns
    /// the two smaller-sized `Chain` instances. This maybe could be helpful? But dunno if that is too much abstraction for the driver layer. We will see
    pub fn set_num_chips(&mut self, num_chips: usize) -> Result<(), Error<SPI::Error>> {
        if num_chips > MAX_CHIPS {
            return Err(Error::TooManyDevices);
        }
        self.num_chips = num_chips;
        Ok(())
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
    /// ```rust,no_run
    /// // Build the same ConfigB for every chip.
    /// let config_b = ConfigB::default()
    ///     .with_vuv(UndervoltageThreshold::from_microvolts(3_000_000).unwrap())
    ///     .with_vov(OvervoltageThreshold::from_microvolts(4_200_000).unwrap());
    ///
    ///
    /// let configs = [config_b; MAX_CHIPS]; // (Index 0 is the chip closest to the host.)
    ///
    /// // Write all chips' ConfigB registers.
    /// match chain.write(&configs) {
    ///     Ok(()) => info!("Wrote ConfigB to {} chips", configs.len()),
    ///     Err(err) => { warn!("evil error: {}", err); return; }
    /// }
    /// ```
    ///
    /// If the reachable chain is shorter than `num_chips()`, for example after a COMM_BK split,
    /// you can pass a shorter slice:
    /// ```rust,no_run
    /// // Three chips reachable on this segment of the chain.
    /// let configs: [PwmB; 3] = [
    ///     PwmB::default().with_pwm13(PwmDutyCycleConfig::Pct26_4),
    ///     PwmB::default().with_pwm15(PwmDutyCycleConfig::Pct39_6),
    ///     PwmB::default().with_pwm14(PwmDutyCycleConfig::Pct72_6),
    /// ];
    ///
    /// // Write these configs to the three chips on this segment.
    /// match chain.write(&configs) {
    ///     Ok(()) => info!("Wrote PwmB to {} chips", configs.len()),
    ///     Err(err) => { warn!("evil error: {}", err); return; }
    /// }
    /// ```
    pub fn write<G: WritableGroup>(&mut self, devices: &[G]) -> Result<(), Error<SPI::Error>> {
        let n = devices.len();
        if n > self.num_chips {
            return Err(Error::TooManyDevices);
        }

        let mut blocks = [[0u8; BLOCK_BYTES]; MAX_CHIPS];
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
    /// ```rust,no_run
    /// // Read the three closest chips' CellVoltagesA registers.
    /// let responses: Response<CellVoltagesA> = match chain.read::<CellVoltagesA>(3) {
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
    pub fn read<G: ReadableGroup>(&mut self, count: usize) -> Result<Response<G>, Error<SPI::Error>> {
        if count > self.num_chips {
            return Err(Error::TooManyDevices);
        }

        let mut response = Response::<G>::empty(count);
        self.spi
            .transaction(&mut [
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
    /// ```rust,no_run
    /// // Read all chips' StatusB registers.
    /// let responses: Response<StatusB> = match chain.read_all::<StatusB>() {
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
    pub fn read_all<G: ReadableGroup>(&mut self) -> Result<Response<G>, Error<SPI::Error>> {
        self.read::<G>(self.num_chips)
    }
}
