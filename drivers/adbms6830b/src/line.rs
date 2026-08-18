//! Module representing a single Line of daisy-chained devices

use embedded_hal_async::spi::{Operation, SpiDevice};

use crate::chip::commands::{self, Command, CommandFrame};
use crate::chip::pec::{DataPecRx, DataPecTx};
use crate::chip::registers::{ReadableGroup, WritableGroup, GROUP_BYTES};
use crate::docs;

/// Bytes one device sends or receives per register group (its data plus the data PEC).
const BLOCK_BYTES: usize = GROUP_BYTES + 2;

/// Errors returned by this driver.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error<E> {
    /// The underlying SPI transaction failed.
    Spi(E),
    /// More devices were addressed than there is buffer space for.
    TooManyDevices,
    /// A user-provided timeout elapsed before the operation finished.
    Timeout,
}

/// Result of a device's data PEC check.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum PecStatus {
    /// The data PEC matched. This means the response data can be trusted.
    Success,
    /// The data PEC did not match. This means that the data was likely corrupted in transit.
    Failed,
}

impl PecStatus {
    /// Whether the PEC check passed.
    pub const fn is_success(self) -> bool {
        matches!(self, Self::Success)
    }

    /// Whether the PEC check failed.
    pub const fn is_failed(self) -> bool {
        matches!(self, Self::Failed)
    }
}

/// One device's answer to a read.
///
/// The data and command counter are readable even when the PEC failed, so check `pec()` before
/// trusting either of them.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ChipResponse<G> {
    data: G,
    command_counter: u8,
    pec: PecStatus,
}

impl<G: Copy> ChipResponse<G> {
    /// The register group the device sent back.
    pub const fn data(&self) -> G {
        self.data
    }

    /// Command counter (`CCNT[5:0]`) the device reported alongside its data.
    pub const fn command_counter(&self) -> u8 {
        self.command_counter
    }

    /// Whether this device's data PEC was found as valid.
    pub const fn pec(&self) -> PecStatus {
        self.pec
    }
}

impl<G: ReadableGroup> ChipResponse<G> {
    /// PRIVATE! Decodes one device's block off the wire (`GROUP_BYTES` of data, then PEC0/PEC1).
    fn from_block(block: &[u8; BLOCK_BYTES]) -> Self {
        let mut data = [0u8; GROUP_BYTES];
        data.copy_from_slice(&block[..GROUP_BYTES]);
        let pec = DataPecRx::from_bytes([block[GROUP_BYTES], block[GROUP_BYTES + 1]]);

        Self {
            data: G::from_bytes(data),
            command_counter: pec.ccnt(),
            pec: if pec.verify(&data) {
                PecStatus::Success
            } else {
                PecStatus::Failed
            },
        }
    }

    /// PRIVATE! Filler for slots that were never read.
    fn blank() -> Self {
        Self {
            data: G::from_bytes([0; GROUP_BYTES]),
            command_counter: 0,
            pec: PecStatus::Failed,
        }
    }
}

#[cfg(feature = "defmt")]
impl<G: defmt::Format> defmt::Format for ChipResponse<G> {
    fn format(&self, f: defmt::Formatter) {
        defmt::write!(
            f,
            "ChipResponse {{ data: {}, command_counter: {=u8}, pec: {} }}",
            self.data,
            self.command_counter,
            self.pec
        )
    }
}

/// Per-device results of a read on a line, nearest the host first.
///
/// Derefs to `[ChipResponse<G>]`. This means you can index it, iterate it, or use `.get()`/`.len()` on it.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Responses<G, const N: usize> {
    chips: [ChipResponse<G>; N],
    used: usize,
}

impl<G: ReadableGroup, const N: usize> Responses<G, N> {
    /// PRIVATE! Decodes the first `used` blocks that came back off from SPI.
    fn from_blocks(blocks: &[[u8; BLOCK_BYTES]; N], used: usize) -> Self {
        Self {
            chips: core::array::from_fn(|i| {
                if i < used {
                    ChipResponse::from_block(&blocks[i])
                } else {
                    ChipResponse::blank()
                }
            }),
            used,
        }
    }

    /// A response covering no devices.
    pub(crate) fn empty() -> Self {
        Self {
            chips: core::array::from_fn(|_| ChipResponse::blank()),
            used: 0,
        }
    }
}

impl<G, const N: usize> Responses<G, N> {
    /// Whether every device covered by this response passed its PEC check.
    pub fn all_ok(&self) -> bool {
        self.chips[..self.used]
            .iter()
            .all(|chip| chip.pec.is_success())
    }
}

impl<G, const N: usize> core::ops::Deref for Responses<G, N> {
    type Target = [ChipResponse<G>];

    fn deref(&self) -> &Self::Target {
        &self.chips[..self.used]
    }
}

#[cfg(feature = "defmt")]
impl<G, const N: usize> defmt::Format for Responses<G, N> {
    fn format(&self, f: defmt::Formatter) {
        defmt::write!(
            f,
            "Responses {{ len: {=usize}, all_ok: {=bool} }}",
            self.used,
            self.all_ok()
        )
    }
}

/// An individual SPI/isoSPI line that goes up to `N` daisy-chained ADBMS6830B devices.
///
/// Index `0` is the device nearest this end of the line.
#[doc = docs::isospi_indexing_example!()]
pub struct Line<SPI, const N: usize> {
    spi: SPI,
}

#[cfg(feature = "defmt")]
impl<SPI, const N: usize> defmt::Format for Line<SPI, N> {
    fn format(&self, f: defmt::Formatter) {
        defmt::write!(f, "Line {{ max_chips: {=usize} }}", N)
    }
}

impl<SPI: SpiDevice, const N: usize> Line<SPI, N> {
    /// Builds a line on `spi`.
    pub const fn new(spi: SPI) -> Self {
        Self { spi }
    }

    /// Releases the underlying SPI device.
    pub fn release(self) -> SPI {
        self.spi
    }

    /// PRIVATE!
    /// SPI bus transaction.
    /// `frame` is the command frame you want to send.
    /// `payload` is the payload if there is one.
    async fn transact(
        &mut self,
        frame: CommandFrame,
        payload: Option<Operation<'_, u8>>,
    ) -> Result<(), Error<SPI::Error>> {
        let bytes = frame.to_bytes();

        match payload {
            Some(payload) => {
                self.spi
                    .transaction(&mut [Operation::Write(&bytes), payload])
                    .await
            }
            None => self.spi.transaction(&mut [Operation::Write(&bytes)]).await,
        }
        .map_err(Error::Spi)
    }

    /// Sends a command that carries no payload.
    pub async fn command(&mut self, command: Command) -> Result<(), Error<SPI::Error>> {
        self.transact(command.frame(), None).await
    }

    /// Reads a register group from the `count` devices nearest this end of the line.
    pub async fn read<G: ReadableGroup>(
        &mut self,
        count: usize,
    ) -> Result<Responses<G, N>, Error<SPI::Error>> {
        if count > N {
            return Err(Error::TooManyDevices);
        }

        let mut blocks = [[0u8; BLOCK_BYTES]; N];
        self.transact(
            G::READ_COMMAND,
            Some(Operation::Read(
                &mut blocks.as_flattened_mut()[..count * BLOCK_BYTES],
            )),
        )
        .await?;

        Ok(Responses::from_blocks(&blocks, count))
    }

    /// Writes one register group per device.
    pub async fn write<G: WritableGroup>(&mut self, groups: &[G]) -> Result<(), Error<SPI::Error>> {
        let count = groups.len();
        if count > N {
            return Err(Error::TooManyDevices);
        }

        // The first block on the wire ends up in the furthest device so we need to reverse the payload
        let mut blocks = [[0u8; BLOCK_BYTES]; N];
        for (i, group) in groups.iter().enumerate() {
            let block = &mut blocks[count - 1 - i];
            let data = group.to_bytes();
            block[..GROUP_BYTES].copy_from_slice(&data);
            let pec = DataPecTx::new(&data);
            block[GROUP_BYTES] = pec.pec0();
            block[GROUP_BYTES + 1] = pec.pec1();
        }

        self.transact(
            G::WRITE_COMMAND,
            Some(Operation::Write(
                &blocks.as_flattened()[..count * BLOCK_BYTES],
            )),
        )
        .await
    }

    /// Sends a poll command and reports whether all `count` devices have finished.
    ///
    /// This does not wait! You need to keep polling it until it returns `true`, or use the Api's
    /// `*_autoconvert()` helpers (which do this automatically).
    pub async fn poll(&mut self, command: Command, count: usize) -> Result<bool, Error<SPI::Error>> {
        if count > N {
            return Err(Error::TooManyDevices);
        }

        // Poll status is only valid after 2*N clock pulses and updates every pulse after that, so
        // this will clock past the invalid window and take the byte after it. See the "POLLING METHODS" section on page 54.
        let used = (2 * count).div_ceil(8) + 1;

        let mut buffer = [[0u8; BLOCK_BYTES]; N];
        self.transact(
            command.frame(),
            Some(Operation::Read(&mut buffer.as_flattened_mut()[..used])),
        )
        .await?;

        Ok(buffer.as_flattened()[used - 1] == 0xFF)
    }

    /// Wakes `count` devices out of the idle or sleep state.
    ///
    /// This sends one pulse pair per device, since each device must wake up before it propagates the pulse to the
    /// next one. See the "Waking Up the Serial Interface" section on page 51 of the datasheet.
    pub async fn wakeup(&mut self, count: usize) -> Result<(), Error<SPI::Error>> {
        use embassy_time::Timer;

        // t_WAKE is 500 us max (from sleep), so the gap has to be at least that long for a device to
        // power up, and under t_IDLE (4.3 ms min) or devices that already woke drop back to idle
        // before the chain finishes.
        const PULSE_GAP_US: u64 = 500;

        // RDCFGA is the datasheet's suggested dummy. also it doesn't INC the command counter.
        for _ in 0..count {
            self.transact(commands::config::rdcfga().frame(), None)
                .await?;
            Timer::after_micros(PULSE_GAP_US).await;
        }

        Ok(())
    }

    /// Counts the devices reachable on this line. This stops at the first bad PEC.
    pub async fn detect_chips(&mut self) -> Result<usize, Error<SPI::Error>> {
        let mut blocks = [[0u8; BLOCK_BYTES]; N];
        self.transact(
            commands::misc::rdsid().frame(),
            Some(Operation::Read(blocks.as_flattened_mut())),
        )
        .await?;

        Ok(blocks
            .iter()
            .take_while(|block| {
                DataPecRx::from_bytes([block[GROUP_BYTES], block[GROUP_BYTES + 1]])
                    .verify(&block[..GROUP_BYTES])
            })
            .count())
    }
}

/// Conversion times from the datasheet (milliseconds).
pub mod conversion_times {
    /// C-ADC single shot conversion.
    pub const C_ADC_MS: u64 = 1;
    /// S-ADC conversion (and a redundant ADCV for RD = 1).
    pub const S_ADC_MS: u64 = 8;
    /// AUX ADC conversion (ADAX).
    pub const AUX_MS: u64 = 1;
    /// AUX2 ADC conversion (ADAX2).
    pub const AUX2_MS: u64 = 8;
    /// Added to any of the above when starting from the standby state (max).
    pub const REFUP_MS: u64 = 5;
}
