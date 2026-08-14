//! Driver for talking to a single line of daisy-chained ADBMS6830B devices over SPI.
//!
//! See the `Line` struct. That is the main guy here

use embedded_hal_async::spi::{Operation, SpiDevice};

use crate::chip::commands;
use crate::chip::pec::{DataPecRx, DataPecTx};
use crate::chip::registers::{ReadableGroup, WritableGroup, GROUP_BYTES};
use crate::docs;

/// Size in bytes of one device's data block.
const BLOCK_BYTES: usize = GROUP_BYTES + 2;

/// Size of a command frame (CMD0, CMD1, PEC0, PEC1).
#[allow(dead_code)]
const COMMAND_BYTES: usize = 4;

/// Largest chip count any `Line` can be configured for.
///
/// This is a compile-time upper bound on the `Line` length. The actual "current" length of a `Line` can be
/// modified dynamically at runtime in case your application needs to.
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
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error<E> {
    /// More devices were asked for than the `Line` holds.
    TooManyDevices,
    /// The `Line` currently has no devices associated with it, so any reads/writes 
    /// are not possible right now.
    NoDevices,
    /// The underlying SPI transaction failed.
    Spi(E),
    /// A user-provided timeout was elapsed before the function returned. This might
    /// indicate unresponsive hardware, or a malformed command that never triggered the
    /// expected reply. Or possibly the timeout was just too short
    Timeout,
}

/// Errors when initializing the driverl
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum InitError {
    /// More devices were asked for than the `Line` holds.
    TooManyDevices,
}

/// Result of the data PEC check on an individual chip's response.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum PecStatus {
    /// The data PEC matched, so the data is trustworthy.
    Success,
    /// The data PEC did not match, so the data was corrupted somewhere on the way here.
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

/// An individual chip's response to a read on a `Line`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ChipResponse<G> {
    data: G,
    command_counter: u8,
    pec_status: PecStatus,
}

impl<G: Copy> ChipResponse<G> {
    /// The requested data sent back by this chip.
    /// 
    /// Note: It is probably not a good idea to trust this data if `.pec()` returns `Failed`.
    pub const fn data(&self) -> G {
        self.data
    }

    /// Command counter (`CCNT[5:0]`) for this chip.
    /// 
    /// Note: It is probably not a good idea to trust this data if `.pec()` returns `Failed`.
    pub const fn command_counter(&self) -> u8 {
        self.command_counter
    }

    /// Whether this device's data PEC check was successful.
    pub const fn pec(&self) -> PecStatus {
        self.pec_status
    }
}

impl<G: ReadableGroup> ChipResponse<G> {
    /// Decodes one device's block into a ChipResponse.
    fn from_block(block: &[u8; BLOCK_BYTES]) -> Self {
        let mut data = [0u8; GROUP_BYTES];
        data.copy_from_slice(&block[..GROUP_BYTES]);
        let pec = DataPecRx::from_bytes([block[GROUP_BYTES], block[GROUP_BYTES + 1]]);

        Self {
            data: G::from_bytes(data),
            command_counter: pec.ccnt(),
            pec_status: if pec.verify(&data) {
                PecStatus::Success
            } else {
                PecStatus::Failed
            },
        }
    }
}

/// Logs a device's data alongside its command counter and PEC status.
#[cfg(feature = "defmt")]
impl<G: defmt::Format> defmt::Format for ChipResponse<G> {
    fn format(&self, f: defmt::Formatter) {
        defmt::write!(
            f,
            "ChipResponse {{ data: {}, command_counter: {=u8}, pec: {} }}",
            self.data,
            self.command_counter,
            self.pec_status
        )
    }
}

/// Responses from each chip on a `Line` after a read.
#[doc = docs::isospi_indexing_example!()]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Responses<G> {
    chips: [ChipResponse<G>; MAX_CHIPS],
    used: usize,
}

impl<G: ReadableGroup> Responses<G> {
    /// Decodes the first `used` blocks that came in from SPI.
    fn from_blocks(blocks: &[[u8; BLOCK_BYTES]; MAX_CHIPS], used: usize) -> Self {
        Self {
            chips: core::array::from_fn(|index| {
                if index < used {
                    ChipResponse::from_block(&blocks[index])
                } else {
                    ChipResponse {
                        data: G::from_bytes([0; GROUP_BYTES]),
                        command_counter: 0,
                        pec_status: PecStatus::Failed,
                    }
                }
            }),
            used,
        }
    }
}

impl<G> Responses<G> {
    /// Returns this `Responses` as a slice of `ChipResponses`.
    /// 
    /// Note: This is indexed with the closest chip to the host first. So, index 0
    /// would be the response from the closest chip to the host.
    pub fn as_slice(&self) -> &[ChipResponse<G>] {
        &self.chips[..self.used]
    }

    /// Number of chips this response covers.
    pub fn len(&self) -> usize {
        self.used
    }

    /// Whether this response covers no chips.
    pub fn is_empty(&self) -> bool {
        self.used == 0
    }

    /// Iterator for every chip's ChipResponse.
    /// 
    /// Note: This is indexed with the closest chip to the host first. So, index 0
    /// would be the response from the closest chip to the host.
    pub fn iter(&self) -> core::slice::Iter<'_, ChipResponse<G>> {
        self.as_slice().iter()
    }

    /// Whether every chip passed its PEC check.
    pub fn all_ok(&self) -> bool {
        self.iter().all(|chip| chip.pec_status.is_success())
    }

    /// Indices of the chip whose data PEC failed.
    pub fn failures(&self) -> impl Iterator<Item = usize> + '_ {
        self.iter()
            .enumerate()
            .filter_map(|(index, chip)| chip.pec_status.is_failed().then_some(index))
    }
}

/// Lets a `Responses` be used anywhere a `&[ChipResponse<G>]` works (indexing, `.get()`, `.first()`, etc).
impl<G> core::ops::Deref for Responses<G> {
    type Target = [ChipResponse<G>];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl<'a, G> IntoIterator for &'a Responses<G> {
    type Item = &'a ChipResponse<G>;
    type IntoIter = core::slice::Iter<'a, ChipResponse<G>>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<G> IntoIterator for Responses<G> {
    type Item = ChipResponse<G>;
    type IntoIter = core::iter::Take<core::array::IntoIter<ChipResponse<G>, MAX_CHIPS>>;

    fn into_iter(self) -> Self::IntoIter {
        let used = self.used;
        self.chips.into_iter().take(used)
    }
}

/// Logs how many devices the response covers and whether they all passed their PEC check.
#[cfg(feature = "defmt")]
impl<G> defmt::Format for Responses<G> {
    fn format(&self, f: defmt::Formatter) {
        defmt::write!(
            f,
            "Responses {{ len: {=usize}, all_ok: {=bool} }}",
            self.used,
            self.all_ok()
        )
    }
}

/// A single SPI/isoSPI line reaching some number of daisy-chained ADBMS6830B devices.
pub struct Line<SPI> {
    spi: SPI,
    num_chips: usize,
}

/// Logs the line's current chip count.
#[cfg(feature = "defmt")]
impl<SPI> defmt::Format for Line<SPI> {
    fn format(&self, f: defmt::Formatter) {
        defmt::write!(f, "Line {{ num_chips: {=usize} }}", self.num_chips)
    }
}

impl<SPI: SpiDevice> Line<SPI> {
    /// Builds a line reaching `num_chips` devices on `spi`.
    pub fn new(spi: SPI, num_chips: usize) -> Result<Self, InitError> {
        if num_chips > MAX_CHIPS {
            return Err(InitError::TooManyDevices);
        }
        Ok(Self { spi, num_chips })
    }

    /// Number of devices currently reachable on this line.
    pub fn num_chips(&self) -> usize {
        self.num_chips
    }

    /// Detects how many devices are reachable on this line.
    ///
    /// RDSID doesn't increment the command counter, so this won't mess with the `CCNT[5:0]`
    /// values that the devices report back on other reads. This also doesn't update `num_chips()` for you.
    /// If you want the line to actually use the detected count, pass it to `set_num_chips()`.
    ///
    /// ### Caveats
    /// - This counts all chips in the line until it encounters an invalid PEC. Because of this, a device that answers
    /// with a corrupted PEC (noise on the line, etc.) will look like the end of the chain. It's a good idea to treat
    /// a surprising result as "something is wrong" rather than as a hard fact, and you should maybe read it a couple
    /// times before believing it. In other words, this function is meant to be an optional or occasionally useful sanity
    /// check to your application's manually-tracked chip number, rather than something that is a primary source of truth.
    /// - This function can never see more than `MAX_CHIPS` devices, since that is all the buffer space there is.
    /// If it returns `MAX_CHIPS` there could technically still be more chips further down the line if your `MAX_CHIPS` isn't
    /// correctly configured for your setup.
    pub async fn detect_num_chips(&mut self) -> Result<usize, Error<SPI::Error>> {
        let mut blocks = [[0u8; BLOCK_BYTES]; MAX_CHIPS];

        self.spi
            .transaction(&mut [
                Operation::Write(&commands::misc::rdsid().frame().to_bytes()),
                Operation::Read(blocks.as_flattened_mut()),
            ])
            .await
            .map_err(Error::Spi)?;

        // Count blocks until one fails its PEC. That first failure is the end of the chain.
        let detected = blocks
            .iter()
            .take_while(|block| {
                let pec = DataPecRx::from_bytes([block[GROUP_BYTES], block[GROUP_BYTES + 1]]);
                pec.verify(&block[..GROUP_BYTES])
            })
            .count();

        Ok(detected)
    }

    /// Changes how many devices this line reaches.
    ///
    /// You can use this after a COMM_BK split moves devices onto the other line. Returns `TooManyDevices`
    /// if `num_chips` exceeds `MAX_CHIPS`. Setting `0` is allowed and just means nothing is routed here
    /// at the moment.
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
    /// a chip on this line. `devices[0]` is the device nearest to this end of the line, so tou should
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
    /// let configs = [config_b; MAX_CHIPS]; // (Index 0 is the chip closest to this end of the line.)
    ///
    /// // Write all chips' ConfigB registers.
    /// match line.write(&configs).await {
    ///     Ok(()) => info!("Wrote ConfigB to {} chips", configs.len()),
    ///     Err(err) => { warn!("evil error: {}", err); return; }
    /// }
    /// ```
    ///
    /// If you only want to write to some of the devices this line reaches, you can pass a shorter slice:
    /// ```rust,no_run
    /// // Three chips reachable on this segment of the chain.
    /// let configs: [PwmB; 3] = [
    ///     PwmB::default().with_pwm13(PwmDutyCycleConfig::Pct26_4),
    ///     PwmB::default().with_pwm15(PwmDutyCycleConfig::Pct39_6),
    ///     PwmB::default().with_pwm14(PwmDutyCycleConfig::Pct72_6),
    /// ];
    ///
    /// // Write these configs to the three chips on this segment.
    /// match line.write(&configs).await {
    ///     Ok(()) => info!("Wrote PwmB to {} chips", configs.len()),
    ///     Err(err) => { warn!("evil error: {}", err); return; }
    /// }
    /// ```
    ///
    #[doc = docs::isospi_indexing_example!()]
    pub async fn write<G: WritableGroup>(&mut self, devices: &[G]) -> Result<(), Error<SPI::Error>> {
        let n = devices.len();

        if self.num_chips == 0 {
            return Err(Error::NoDevices);
        }

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
            .await
            .map_err(Error::Spi)
    }

    /// Reads a register group from the devices on this line.
    ///
    /// ### Parameters
    /// - `count`: The number of devices you want to read. `1` would mean that you
    /// only want to read the closest chip to this end of the line. `2` would mean that
    /// you want to read the first two closest chips. `3` would mean you want to read
    /// the first three closest chips. You get the idea
    ///
    /// ### Examples
    /// Here's an example of how this function might be used:
    /// ```rust,no_run
    /// // Read the three closest chips' CellVoltagesA registers.
    /// let responses: Responses<CellVoltagesA> = match line.read::<CellVoltagesA>(3).await {
    ///     Ok(responses) => responses,
    ///     Err(err) => { warn!("evil error: {}", err); return; }
    /// };
    ///
    /// // Loop through the returned responses for each chip.
    /// for (index, response) in responses.iter().enumerate() {
    ///     // Check each chip for PEC errors.
    ///     if response.pec().is_failed() {
    ///         warn!("PEC error when reading chip {}!!!", index);
    ///         return;
    ///     }
    ///
    ///     // Log the data from each chip's CellVoltagesA register.
    ///     let cells_a: CellVoltagesA = response.data();
    ///     info!("Chip {}: Cell 1 voltage: {} uV", index, cells_a.c1v().as_microvolts());
    ///     info!("Chip {}: Cell 2 voltage: {} uV", index, cells_a.c2v().as_microvolts());
    ///     info!("Chip {}: Cell 3 voltage: {} uV", index, cells_a.c3v().as_microvolts());
    /// }
    /// ```
    ///
    #[doc = docs::isospi_indexing_example!()]
    pub async fn read<G: ReadableGroup>(&mut self, count: usize) -> Result<Responses<G>, Error<SPI::Error>> {
        if self.num_chips == 0 {
            return Err(Error::NoDevices);
        }

        if count > self.num_chips {
            return Err(Error::TooManyDevices);
        }

        let mut blocks = [[0u8; BLOCK_BYTES]; MAX_CHIPS];
        self.spi
            .transaction(&mut [
                Operation::Write(&G::READ_COMMAND.to_bytes()),
                Operation::Read(&mut blocks.as_flattened_mut()[..count * BLOCK_BYTES]),
            ])
            .await
            .map_err(Error::Spi)?;

        Ok(Responses::from_blocks(&blocks, count))
    }

    /// Reads one register group from every device this line reaches.
    ///
    /// ### Examples
    /// Here's an example of how this function might be used:
    /// ```rust,no_run
    /// // Read all chips' StatusB registers.
    /// let responses: Responses<StatusB> = match line.read_all::<StatusB>().await {
    ///     Ok(responses) => responses,
    ///     Err(err) => { warn!("evil error: {}", err); return; }
    /// };
    ///
    /// // Loop through the returned responses for each chip.
    /// for (index, response) in responses.iter().enumerate() {
    ///     // Check each chip for PEC errors.
    ///     if response.pec().is_failed() {
    ///         warn!("PEC error when reading chip {}!!!", index);
    ///         return;
    ///     }
    ///
    ///     // Log the data from each chip's StatusB register.
    ///     let status_b: StatusB = response.data();
    ///     info!("Chip {}: Digital power supply voltage: {} uV", index, status_b.vd().as_microvolts());
    ///     info!("Chip {}: Analog power supply voltage: {} uV", index, status_b.va().as_microvolts());
    ///     info!("Chip {}: VREF2 across resistor: {} uV", index, status_b.vres().as_microvolts());
    /// }
    /// ```
    ///
    #[doc = docs::isospi_indexing_example!()]
    pub async fn read_all<G: ReadableGroup>(&mut self) -> Result<Responses<G>, Error<SPI::Error>> {
        self.read::<G>(self.num_chips).await
    }

    /// Wakes every device on this line out of the idle or sleep state.
    ///
    /// This generates the isoSPI pulse pairs the devices' wake-up
    /// circuits detect. There is one pair per device, since each device must wake before it will propagate
    /// the pulse to the next one.
    ///
    /// Sending more pairs than necessary is harmless, and this is safe to call on a line that's
    /// already awake.
    ///
    /// See the "Waking Up the Serial Interface" section on page 51 of the datasheet.
    pub async fn wakeup(&mut self) -> Result<(), Error<SPI::Error>> {
        use embassy_time::Timer;

        // Gap between pulses. t_WAKE is 500 us max (from sleep), so this has to be at least that
        // long for a device to power up. It also has to stay under t_IDLE (4.3 ms min), or the
        // devices that are already awake drop back to idle before the chain finishes waking.
        const PULSE_GAP_US: u64 = 500;

        // RDCFGA is the dummy command the datasheet suggests for waking, and it doesn't increment the command counter.
        // We're doing this instead of manually asserting the CS pins because its possible that not all implementations of SpiDevice
        // would assert/deassert CS on a Operation::DelayNs transaction
        let dummy = commands::config::rdcfga().frame().to_bytes();

        for _ in 0..self.num_chips {
            self.spi
                .transaction(&mut [Operation::Write(&dummy)])
                .await
                .map_err(Error::Spi)?;
            Timer::after_micros(PULSE_GAP_US).await;
        }

        Ok(())
    }

}

/// # Simple Commands
/// 
/// This impl block is for all the simple oneshot chip commands.
/// These commands have no parameters, don't carry any payload data, and don't read back any data.
impl<SPI: SpiDevice> Line<SPI> {
    /// Private helper that sends a command frame. This is used by the public/exposed command functions.
    async fn command(&mut self, command: commands::Command) -> Result<(), Error<SPI::Error>> {
        self.spi.transaction(&mut [Operation::Write(&commands::CommandFrame::from_command(&command).to_bytes())])
        .await
        .map_err(Error::Spi)
    }

    /// Snapshot command (SNAP).
    /// 
    /// This will freeze all result and status registers in the `Line`
    /// at a given moment. This is useful if you want to make sure data isn't
    /// actively updating as you are reading it, especially if you want to make sure
    /// all data in a single read corresponds to the exact same timestamp. To release the
    /// freeze, use the `.unsnap()` function.
    /// 
    /// For more information, see the "SNAPSHOT COMMANDS" section on page 60 of the datasheet.
    pub async fn snap(&mut self) -> Result<(), Error<SPI::Error>> {
        self.command(commands::snapshot::snap()).await
    }

    /// Un-snapshot command (UNSNAP).
    /// 
    /// This will unfreeze the chips on the line if they were previously frozen by the `.snap()` command.
    /// 
    /// For more information, see the "SNAPSHOT COMMANDS" section on page 60 of the datasheet.
    pub async fn unsnap(&mut self) -> Result<(), Error<SPI::Error>> {
        self.command(commands::snapshot::unsnap()).await
    }

    /// Mute discharge command (MUTE).
    /// 
    /// This command will disable all discharge until you re-enable it via the `.unmute()` command.
    /// Note that a mute is cleared automatically upon a watchdog timeout.
    pub async fn mute(&mut self) -> Result<(), Error<SPI::Error>> {
        self.command(commands::discharge::mute()).await
    }

    /// Unmute discharge command (UNMUTE).
    /// 
    /// This command command re-enables discharge if it was previously disabled via the `.mute()` command.
    pub async fn unmute(&mut self) -> Result<(), Error<SPI::Error>> {
        self.command(commands::discharge::unmute()).await
    }

    /// Clear cell registers command (CLRCELL).
    /// 
    /// This command will clear Cell Voltage Register A through
    /// Cell Voltage Register F, alongside the averaged cell voltage registers.
    /// All bytes in these registers are set to 0x8000 (their default) after this command.
    pub async fn clrcell(&mut self) -> Result<(), Error<SPI::Error>> {
        self.command(commands::clear::clrcell()).await
    }

    /// Clear filtered cell registers command (CLRFC).
    /// 
    /// This command will clear Filtered Cell Voltage Register A through
    /// Filtered Cell Voltage Register F. 
    /// All bytes in these registers are set to 0x8000 (their default) after this command.
    pub async fn clrfc(&mut self) -> Result<(), Error<SPI::Error>> {
        self.command(commands::clear::clrfc()).await
    }

    /// Clear aux cell registers command (CLRAUX).
    /// 
    /// This command will clear Auxiliary Register Group A through
    /// Auxiliary Register Group D, the Redundant Auxiliary Register
    /// Group A through Redundant Auxiliary Register Group D, and Status
    /// Register Group A and Status Register Group B.
    /// 
    /// All bytes in these registers are set to 0x8000 by this command. Note that
    /// the register value of 0x8000 resulting from this command is,
    /// for some registers, different than their default value after power-up.
    pub async fn clraux(&mut self) -> Result<(), Error<SPI::Error>> {
        self.command(commands::clear::clraux()).await
    }

    /// Clear spin voltage registers command (CLRSPIN).
    /// 
    /// This command will clear S-Voltage Register A through S-Voltage Register F. All bytes in
    /// these registers are set to 0x8000 by this command.
    pub async fn clrspin(&mut self) -> Result<(), Error<SPI::Error>> {
        self.command(commands::clear::clrspin()).await
    }

    /// Reset command counter command (RSTCC).
    /// 
    /// This command resets the chips' hardware-level command counters to 0.
    pub async fn rstcc(&mut self) -> Result<(), Error<SPI::Error>> {
        self.command(commands::misc::rstcc()).await
    }

    /// Soft reset command (SRST).
    /// 
    /// The soft reset command (SRST) quickly puts all the devices in
    /// the daisy chain into the sleep state. The soft reset command only
    /// needs sufficient time to propagate the command up the stack to
    /// the next device, after which the device enters sleep. This command
    /// achieves two functions: a quick transition to the low power state,
    /// and the ability to reset all of the switched power digital logic.
    pub async fn srst(&mut self) -> Result<(), Error<SPI::Error>> {
        self.command(commands::misc::srst()).await
    }

    /// LCPM enable command (CMEN).
    /// 
    /// This command enables the Low Power Cell Monitoring feature.
    /// For more information, see the "LCPM OPERATION" section on page 32 of the datasheet,
    /// and probably other datasheet sections as well.
    pub async fn cmen(&mut self) -> Result<(), Error<SPI::Error>> {
        self.command(commands::lpcm::cmen()).await
    }

    /// LCPM disable command (CMDIS).
    /// 
    /// This command disables the Low Power Cell Monitoring feature.
    /// For more information, see the "LCPM OPERATION" section on page 32 of the datasheet,
    /// and probably other datasheet sections as well.
    pub async fn cmdis(&mut self) -> Result<(), Error<SPI::Error>> {
        self.command(commands::lpcm::cmdis()).await
    }
}

/// Module holding constants for the various ADC conversion times.
pub mod conversion_times {
    /// C-ADC single shot conversion time. Unit of ms.
    pub const C_ADC_MS: u32 = 1;
    /// S-ADC conversion time, and the time for a redundant ADCV (`RD = 1`). Unit of ms.
    pub const S_ADC_MS: u32 = 8;
    /// AUX ADC conversion time (ADAX). Unit of ms.
    pub const AUX_MS: u32 = 1;
    /// AUX2 ADC conversion time (ADAX2). Unit of ms.
    pub const AUX2_MS: u32 = 8;
    /// Added to any of the above when starting from the standby state (max). Unit of ms.
    pub const REFUP_MS: u32 = 5;
}

/// # ADC Commands
///
/// This impl block is for the oneshot ADC conversion commands. These carry no payload data
/// and read nothing back, but they do have some parameters that control their behavior.
impl<SPI: SpiDevice> Line<SPI> {
    /// Start Cell Voltage ADC Conversion and Poll Status (ADCV).
    ///
    /// ### Parameters
    /// - `redundancy`: Whether to also trigger the S-ADCs and compare their results against
    /// the C-ADC averages. A mismatch beyond the threshold set by `CTH[2:0]` in Config A sets
    /// that cell's `CSxFLT` flag in Status Register Group C.
    /// - `acquisition`: How the conversion runs, and whether PWM discharge continues through it.
    /// - `reset_filter`: Whether to reset the IIR filter.
    /// - `open_wire`: Which cell inputs to enable open wire excitation on.
    ///
    /// ### Caveats
    /// - Any ADCV interrupts the ongoing C-ADC conversions and restarts the C-ADCs. If you want
    /// periodic redundant measurements, the datasheet recommends using `.adsv()` with
    /// `Acquisition::Continuous` every fault tolerant time interval instead of re-issuing this.
    /// - An ADCV with `AdcvRedundancy::Enabled` resets the open wire switches to open so the
    /// C-ADC/S-ADC comparison is valid. Redundancy and open wire excitation therefore can't be
    /// combined in one ADCV command, and you would need to use `.adsv()` for the open wire measurement.
    ///
    /// For more information, see Table 19 on page 20 of the datasheet.
    pub async fn adcv(&mut self, redundancy: commands::adc::AdcvRedundancy, acquisition: commands::adc::Acquisition, reset_filter: commands::adc::ResetFilter, open_wire: commands::adc::OpenWire) -> Result<(), Error<SPI::Error>> {
        self.command(commands::adc::adcv(redundancy, acquisition, reset_filter, open_wire)).await
    }

    /// Start S-ADC Conversion and Poll Status (ADSV).
    ///
    /// ### Parameters
    /// - `acquisition`: How the conversion runs, and whether PWM discharge continues through it.
    /// - `open_wire`: Which cell inputs to enable open wire excitation on.
    ///
    /// Unlike `.adcv()`, this doesn't restart the C-ADCs. Issuing it with
    /// `Acquisition::Continuous` while the C-ADCs are already converting continuously
    /// synchronizes the S-ADCs to the C-ADC average of 8 conversions and compares the two,
    /// which is the datasheet's recommended way to take periodic redundant measurements.
    ///
    /// For more information, see Table 19 on page 20 of the datasheet.
    pub async fn adsv(&mut self, acquisition: commands::adc::Acquisition, open_wire: commands::adc::OpenWire) -> Result<(), Error<SPI::Error>> {
        self.command(commands::adc::adsv(acquisition, open_wire)).await
    }

    /// Start AUX ADC Conversions and Poll Status (ADAX).
    ///
    /// ### Parameters
    /// - `open_wire`: Whether to run this conversion with open wire excitation on the AUX inputs.
    /// - `pull`: Whether that excitation uses a pull-up or a pull-down current. This has no effect
    /// unless `open_wire` is `OpenWireAux::On`.
    /// - `channel`: Which AUX input to convert. `Aux1InputSelection::All` converts every one of them.
    ///
    /// For more information, see Table 52 on page 59 of the datasheet.
    pub async fn adax(&mut self, open_wire: commands::adc::OpenWireAux, pull: commands::adc::Pull, channel: commands::adc::Aux1InputSelection) -> Result<(), Error<SPI::Error>> {
        self.command(commands::adc::adax(open_wire, pull, channel)).await
    }

    /// Start AUX2 ADC Conversions and Poll Status (ADAX2).
    ///
    /// ### Parameters
    /// - `channel`: Which AUX input to convert. `Aux2InputSelection::All` converts every one of them.
    ///
    /// Unlike `.adax()`, this takes no open wire or pull parameters. Use `.adax()` if you need one of the internal
    /// measurements.
    ///
    /// For more information, see Table 52 on page 59 of the datasheet.
    pub async fn adax2(&mut self, channel: commands::adc::Aux2InputSelection) -> Result<(), Error<SPI::Error>> {
        self.command(commands::adc::adax2(channel)).await
    }

    /// Sends a poll command and reads back whether the line has finished converting.
    ///
    /// Returns `true` once every device on the line has completed the polled operation.
    /// 
    /// Note: This function does NOT manage a waker or anything to sleep until conversions are
    /// completed. You need to continuously poll this function until it returns `true` before you
    /// can reliably read your conversions. This driver provides helpers
    /// that start the conversion and do this waiting for you. These
    /// functions are `adcv_autoconvert()`, `adsv_autoconvert()`, `adax_autoconvert()`, and
    /// `adax2_autoconvert()`. This
    /// driver provides the raw constants for the expected conversion times according to the datasheet via the
    /// [`conversion_times`] module.
    async fn poll(&mut self, command: commands::Command) -> Result<bool, Error<SPI::Error>> {
        /// Dummy bytes to clock out when polling `num_chips` devices.
        ///
        /// According to the datasheet, poll status is only valid after 2 x N clock pulses and
        /// updates every clock pulse after that, so `ceil(2N/8)` bytes cover the invalid window
        /// and the byte after it is entirely valid status. See the "POLLING METHODS" and "NETWORK LAYER" sections on page 54.
        const fn poll_bytes(num_chips: usize) -> usize { (2 * num_chips + 7) / 8 + 1 }
        const MAX_POLL_BYTES: usize = poll_bytes(MAX_CHIPS);

        if self.num_chips == 0 {
            return Err(Error::NoDevices);
        }

        let mut status = [0u8; MAX_POLL_BYTES];
        let used = poll_bytes(self.num_chips);

        // CS has to stay asserted across both operations. A single `transaction` guarantees that.
        self.spi
            .transaction(&mut [
                Operation::Write(&commands::CommandFrame::from_command(&command).to_bytes()),
                Operation::Read(&mut status[..used]),
            ])
            .await
            .map_err(Error::Spi)?;

        Ok(status[used - 1] == 0xFF)
    }


    /// Poll Any ADC Status command (PLADC).
    /// 
    /// This command polls the status of all ADCs together, which is only meaningful 
    /// if only single shot measures have been triggered, because any ADC in continuous 
    /// mode prevents successful polling of the end of conversion of
    /// other ADCs.
    /// 
    /// Note: This function does NOT manage a waker or anything to sleep until conversions are
    /// completed. You need to continuously poll this function until it returns `true` before you
    /// can reliably read your conversions. This driver provides helpers
    /// that start the conversion and do this waiting for you. These
    /// functions are `adcv_autoconvert()`, `adsv_autoconvert()`, `adax_autoconvert()`, and
    /// `adax2_autoconvert()`. This
    /// driver provides the raw constants for the expected conversion times according to the datasheet via the
    /// [`conversion_times`] module.
    pub async fn pladc(&mut self) -> Result<bool, Error<SPI::Error>> {
        self.poll(commands::adc::pladc()).await
    }

    /// Poll C-ADC command (PLCADC).
    /// 
    /// This command polls the status of the Cell Voltage ADCs.
    /// 
    /// This command is typically used after starting C-ADC conversions via `.adcv()`.
    /// 
    /// Note: This function does NOT manage a waker or anything to sleep until conversions are
    /// completed. You need to continuously poll this function until it returns `true` before you
    /// can reliably read your conversions. This driver provides helpers
    /// that start the conversion and do this waiting for you. These
    /// functions are `adcv_autoconvert()`, `adsv_autoconvert()`, `adax_autoconvert()`, and
    /// `adax2_autoconvert()`. This
    /// driver provides the raw constants for the expected conversion times according to the datasheet via the
    /// [`conversion_times`] module.
    pub async fn plcadc(&mut self) -> Result<bool, Error<SPI::Error>> {
        self.poll(commands::adc::plcadc()).await
    }

    /// Poll S-ADC command (PLSADC).
    /// 
    /// This command polls the status of the S-ADCs.
    /// 
    /// This command is typically used after starting S-ADC conversions via `.adsv()`.
    /// 
    /// Note: This function does NOT manage a waker or anything to sleep until conversions are
    /// completed. You need to continuously poll this function until it returns `true` before you
    /// can reliably read your conversions. This driver provides helpers
    /// that start the conversion and do this waiting for you. These
    /// functions are `adcv_autoconvert()`, `adsv_autoconvert()`, `adax_autoconvert()`, and
    /// `adax2_autoconvert()`. This
    /// driver provides the raw constants for the expected conversion times according to the datasheet via the
    /// [`conversion_times`] module.
    pub async fn plsadc(&mut self) -> Result<bool, Error<SPI::Error>> {
        self.poll(commands::adc::plsadc()).await
    }

    /// Poll AUX ADC command (PLAUX).
    /// 
    /// This command polls the status of the AUX ADCs.
    /// 
    /// This command is typically used after starting AUX ADC conversions via `.adax()`.
    /// 
    /// Note: This function does NOT manage a waker or anything to sleep until conversions are
    /// completed. You need to continuously poll this function until it returns `true` before you
    /// can reliably read your conversions. This driver provides helpers
    /// that start the conversion and do this waiting for you. These
    /// functions are `adcv_autoconvert()`, `adsv_autoconvert()`, `adax_autoconvert()`, and
    /// `adax2_autoconvert()`. This
    /// driver provides the raw constants for the expected conversion times according to the datasheet via the
    /// [`conversion_times`] module.
    pub async fn plaux(&mut self) -> Result<bool, Error<SPI::Error>> {
        self.poll(commands::adc::plaux()).await
    }

    /// Poll AUX2 ADC command (PLAUX2).
    /// 
    /// This command polls the status of the AUX2 ADCs.
    /// 
    /// This command is typically used after starting AUX2 ADC conversions via `.adax2()`.
    /// 
    /// Note: This function does NOT manage a waker or anything to sleep until conversions are
    /// completed. You need to continuously poll this function until it returns `true` before you
    /// can reliably read your conversions. This driver provides helpers
    /// that start the conversion and do this waiting for you. These
    /// functions are `adcv_autoconvert()`, `adsv_autoconvert()`, `adax_autoconvert()`, and
    /// `adax2_autoconvert()`. This
    /// driver provides the raw constants for the expected conversion times according to the datasheet via the
    /// [`conversion_times`] module.
    pub async fn plaux2(&mut self) -> Result<bool, Error<SPI::Error>> {
        self.poll(commands::adc::plaux2()).await
    }

}

/// # Automatic Conversions
///
/// This impl block is for the `*_autoconvert()` functions. Each one starts a conversion, waits the
/// time that conversion is expected to take, and then polls until the line reports that every
/// device has finished. They only return once the conversion is actually complete, so when one of
/// them returns `Ok(())` you can go straight to reading the result registers.
impl<SPI: SpiDevice> Line<SPI> {
    /// Shared private helper for the `*_autoconvert()` helpers.
    ///
    /// Sends `start`, sleeps for `conversion_ms`, then polls with `poll` until the line reports
    /// that it has finished. `timeout_ms` bounds the whole operation, measured from just before
    /// `start` goes out.
    async fn autoconvert(&mut self, start: commands::Command, poll: commands::Command, conversion_ms: u64, timeout_ms: u64) -> Result<(), Error<SPI::Error>> {
        use embassy_time::{Duration, Instant, Timer};
        
        /// How long to wait between poll commands once the expected conversion time has already elapsed.
        const POLL_INTERVAL_MS: u64 = 1;

        let deadline = Instant::now() + Duration::from_millis(timeout_ms);

        self.command(start).await?;

        // Wait out the expected conversion time before polling at all. Polling immediately would
        // always come back busy.
        Timer::after_millis(conversion_ms).await;

        loop {
            if self.poll(poll).await? {
                return Ok(());
            }

            if Instant::now() >= deadline {
                return Err(Error::Timeout);
            }

            Timer::after_millis(POLL_INTERVAL_MS).await;
        }
    }

    /// Start a Cell Voltage ADC Conversion (ADCV) and wait for it to finish.
    ///
    /// This just a call to `.adcv()`, followed by an automatic `.plcadc()` poll loop. It returns once every
    /// device on the line reports that its C-ADCs are done, at which point the cell voltage result
    /// registers are ready to read.
    ///
    /// ### Parameters
    /// - `redundancy`: Whether to also trigger the S-ADCs and compare their results against
    /// the C-ADC averages. A mismatch beyond the threshold set by `CTH[2:0]` in Config A sets
    /// that cell's `CSxFLT` flag in Status Register Group C.
    /// - `acquisition`: How the conversion runs, and whether PWM discharge continues through it.
    /// - `reset_filter`: Whether to reset the IIR filter.
    /// - `open_wire`: Which cell inputs to enable open wire excitation on.
    /// - `timeout_ms`: How long to keep polling before giving up with `Error::Timeout`.
    ///
    /// ### Wait time
    /// The expected conversion time is derived from `redundancy`:
    /// - A standalone C-ADC conversion takes 1 ms.
    /// - On the other hand, a redundant ADCV also triggers
    /// the S-ADCs and compares the two results, which takes 8 ms.
    /// - Starting from the standby state adds up to `REFUP_MS` on top of either of the two above.
    ///
    /// ### Errors
    /// - `Error::Timeout` if the line never reported completion in `timeout_ms`.
    pub async fn adcv_autoconvert(&mut self, redundancy: commands::adc::AdcvRedundancy, acquisition: commands::adc::AutoAcquisition, reset_filter: commands::adc::ResetFilter, open_wire: commands::adc::OpenWire, timeout_ms: u64) -> Result<(), Error<SPI::Error>> {
        let acquisition: commands::adc::Acquisition = acquisition.into();

        let conversion_ms = match redundancy {
            // Triggers the S-ADCs too and compares them against the C-ADC averages, so this takes
            // a full S-ADC conversion rather than a single C-ADC one.
            commands::adc::AdcvRedundancy::Enabled => conversion_times::S_ADC_MS as u64,
            commands::adc::AdcvRedundancy::Disabled => conversion_times::C_ADC_MS as u64,
        };

        self.autoconvert(
            commands::adc::adcv(redundancy, acquisition, reset_filter, open_wire),
            commands::adc::plcadc(),
            conversion_ms,
            timeout_ms,
        )
        .await
    }

    /// Start an S-ADC Conversion (ADSV) and wait for it to finish.
    ///
    /// This is just a call to `.adsv()`, followed by an automatic `.plsadc()` poll loop. It returns once every
    /// device on the line reports that its S-ADCs are done, at which point the S-voltage result
    /// registers are ready to read.
    ///
    /// ### Parameters
    /// - `acquisition`: How the conversion runs, and whether PWM discharge continues through it.
    /// - `open_wire`: Which cell inputs to enable open wire excitation on.
    /// - `timeout_ms`: How long to keep polling before giving up with `Error::Timeout`.
    ///
    /// ### Wait time
    /// A normal single-shot S-ADC conversion takes 8 ms. 
    /// However, starting from the standby state adds up to `REFUP_MS` on top of this.
    ///
    /// ### Errors
    /// - `Error::Timeout` if the line never reported completion in `timeout_ms`.
    pub async fn adsv_autoconvert(&mut self, acquisition: commands::adc::AutoAcquisition, open_wire: commands::adc::OpenWire, timeout_ms: u64) -> Result<(), Error<SPI::Error>> {
        let acquisition: commands::adc::Acquisition = acquisition.into();

        self.autoconvert(
            commands::adc::adsv(acquisition, open_wire),
            commands::adc::plsadc(),
            conversion_times::S_ADC_MS as u64,
            timeout_ms,
        )
        .await
    }

    /// Start an AUX ADC Conversion (ADAX) and wait for it to finish.
    ///
    /// This is just a call to `.adax()`, followed by an automatic `.plaux()` poll loop. It returns once every
    /// device on the line reports that its AUX ADC is done, at which point the auxiliary result
    /// registers are ready to read.
    ///
    /// ### Parameters
    /// - `open_wire`: Whether to run this conversion with open wire excitation on the AUX inputs.
    /// - `pull`: Whether that excitation uses a pull-up or a pull-down current. This has no effect
    /// unless `open_wire` is `OpenWireAux::On`.
    /// - `channel`: Which AUX input to convert. `Aux1InputSelection::All` converts every one of them.
    /// - `timeout_ms`: How long to keep polling before giving up with `Error::Timeout`.
    ///
    /// ### Wait time
    /// An AUX conversion takes 1 ms per channel. Be aware that this figure doesn't account for
    /// soak time. If `SOAKON` is set in Config A, each channel can be delayed due to your configuraiton.
    ///
    /// Note that the datasheet warns that an ADAX with a long soak time can outlast the watchdog, and that valid
    /// commands have to keep arriving or the device interrupts the measurement and sleeps.
    ///
    /// ### Errors
    /// - `Error::Timeout` if the line never reported completion in `timeout_ms`.
    pub async fn adax_autoconvert(&mut self, open_wire: commands::adc::OpenWireAux, pull: commands::adc::Pull, channel: commands::adc::Aux1InputSelection, timeout_ms: u64) -> Result<(), Error<SPI::Error>> {
        self.autoconvert(
            commands::adc::adax(open_wire, pull, channel),
            commands::adc::plaux(),
            conversion_times::AUX_MS as u64,
            timeout_ms,
        )
        .await
    }

    /// Start an AUX2 ADC Conversion (ADAX2) and wait for it to finish.
    ///
    /// This is just a call to `.adax2()`, followed by an automatic `.plaux2()` poll loop. It returns once every
    /// device on the line reports that its AUX2 ADC is done, at which point the redundant
    /// auxiliary result registers are ready to read.
    ///
    /// ### Parameters
    /// - `channel`: Which AUX input to convert. `Aux2InputSelection::All` converts every one of them.
    /// - `timeout_ms`: How long to keep polling before giving up with `Error::Timeout`.
    ///
    /// ### Wait time
    /// An AUX2 conversion takes 8 ms. The same soak time caveat as `.adax_autoconvert()` applies
    /// here, so probably a good idea to read those docs and take a look at the datasheet.
    ///
    /// ### Errors
    /// - `Error::Timeout` if the line never reported completion in `timeout_ms`.
    pub async fn adax2_autoconvert(&mut self, channel: commands::adc::Aux2InputSelection, timeout_ms: u64) -> Result<(), Error<SPI::Error>> {
        self.autoconvert(
            commands::adc::adax2(channel),
            commands::adc::plaux2(),
            conversion_times::AUX2_MS as u64,
            timeout_ms,
        )
        .await
    }
}
