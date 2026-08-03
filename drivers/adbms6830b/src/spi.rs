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
    /// 
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
    pub fn read_all<G: ReadableGroup>(
        &mut self,
    ) -> Result<impl Response<Group = G>, Error<SPI::Error>> {
        self.read::<G>(NUM_CHIPS)
    }
}
