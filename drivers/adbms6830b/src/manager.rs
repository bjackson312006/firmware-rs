//! Manages `N` ADBMS6830B chips reachable from two isoSPI lines.
//! 
//! This driver uses the term "logical order" a lot because that's what claude kept using. For these
//! purposes, "logical order" is the order of chips from the POV of Line A.

use embedded_hal_async::spi::SpiDevice;

use crate::chip::commands::{self, Command, CommandFrame};
use crate::chip::registers::{ReadableGroup, WritableGroup};
use crate::docs;
use crate::line::{conversion_times, ChipResponse, Error, Line};
use crate::chip::registers::{
    config_a::ConfigA,
    config_b::ConfigB,
};

/// Highest value `CCNT[5:0]` reaches before rolling over.
const CCNT_MAX: u8 = 63;

/// Which isoSPI line a chip is reached from.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum LineId {
    /// Reaches chips from logical index `0` upwards.
    A,
    /// Reaches chips from logical index `N - 1` downwards.
    B,
}

/// Helper struct for `ChipState` that stores command count data.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct CommandCount {
    /// Expected command count.
    expected: u8,
    /// Reported command count (reported via reads).
    reported: u8,
    /// Times the command count has been manually re-synced. 
    resync_count: usize,
}
impl CommandCount {
    /// New CommandCount with initial state.
    pub(crate) const fn new() -> Self {
        CommandCount {
            expected: 0,
            reported: 0,
            resync_count: 0,
        }
    }

    /// Re-syncs the command counter.
    /// 
    /// This is different to `reset()`.
    pub(crate) const fn resync(&mut self) {
        if !self.in_sync() {
            self.expected = self.reported;
            self.resync_count += 1;
        }
    }

    /// What this chip's counter "should" be. This is tracked from the commands sent to it.
    pub const fn expected(&self) -> u8 {
        self.expected
    }

    /// What this chip reported on the last read of it that passed its PEC.
    pub const fn reported(&self) -> u8 {
        self.reported
    }

    /// Times this chip's command count has been manually re-synced.
    pub const fn resync_count(&self) -> usize {
        self.resync_count
    }

    /// Whether the reported counter matches the expected one.
    ///
    /// A mismatch means the chip missed a command, reset, or slept since the last resync.
    pub const fn in_sync(&self) -> bool {
        self.expected == self.reported
    }
}

/// Command counter state for one chip.
#[derive(Copy, Clone, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct ChipState {
    /// Command Count metadata.
    pub(crate) command_count: CommandCount,
    /// Number of times the command count has been reset.
    /// 
    /// This is different to the `resync_count` metadata inside
    /// the CommandCount struct itself. This counts how many times the entire
    /// `CommandCount` struct has been reset for this chip due to a sleep.
    pub(crate) command_count_resets: usize,
    /// Last time we heard from this chip with a good PEC.
    /// 
    /// `None` means we haven't heard from this chip yet.
    pub(crate) last_contacted: Option<embassy_time::Instant>,
    /// Number of times this chip has read in a successful PEC. 
    pub(crate) pec_success_count: usize,
    /// Number of times this chip has read in a failed PEC.
    pub(crate) pec_failed_count: usize,
}

impl ChipState {
    /// Command Count metadata.
    pub const fn command_count(&self) -> CommandCount {
        self.command_count
    }

    /// Resets an already-initialized command counter. This is meant to be called
    /// after a sleep state is detected.
    /// 
    /// ### Parameters
    /// - `count`: The `count` you want to initialize both `expected` and `reported` to. This should
    /// be read directly from the chip. This way, `expected` and `reported` are starting from the exact same
    /// known state. In other words, this basically lets you start up the command counting with a blank slate.
    pub(crate) const fn reset_command_count(&mut self, count: u8) {
        self.command_count = CommandCount {
            expected: count,
            reported: count,
            resync_count: 0
        };
        self.command_count_resets += 1;
    }

    /// Last time we heard from this chip with a good PEC.
    /// 
    /// `None` means we haven't heard from this chip yet.
    pub const fn last_contacted(&self) -> Option<embassy_time::Instant> {
        self.last_contacted
    }

    /// Returns the number of times the command counter for this chip has
    /// been reset due to a sleep.
    /// 
    /// Because this only increments when a sleep is detected, this field
    /// can be interpereted as a "number of times a sleep has been detected" for
    /// this chip as well.
    pub const fn command_count_resets(&self) -> usize {
        self.command_count_resets
    }

    /// Number of times this chip has read in a successful PEC. 
    pub const fn pec_success_count(&self) -> usize {
        self.pec_success_count
    }

    /// Number of times this chip has read in a failed PEC.
    pub const fn pec_failed_count(&self) -> usize {
        self.pec_failed_count
    }
}

/// Per-chip results of a read.
#[derive(Copy, Clone, Debug)]
pub struct Responses<G, E, const N: usize> {
    chips: [Option<ChipResponse<G>>; N],
    errors: [Option<Error<E>>; 2],
}

impl<G: Copy, E, const N: usize> Responses<G, E, N> {
    /// The response from one chip.
    /// 
    /// If the chip's line transaction failed, this will return `None` (because that entire line would not be
    /// carrying any data). Upon getting a `None` response, you may look up the culprit SPI error via the `line_error()` function.
    pub fn chip(&self, chip: usize) -> Option<ChipResponse<G>> {
        self.chips[chip]
    }

    /// Every chip's response in logical order.
    pub fn iter(&self) -> impl Iterator<Item = Option<ChipResponse<G>>> + '_ {
        self.chips.iter().copied()
    }

    /// Returns the error that failed a line's transaction, if there was one.
    /// If the chip's line had a successful read, this will return `None` (since there's no error to report).
    pub fn line_error(&self, line: LineId) -> Option<&Error<E>> {
        self.errors[line as usize].as_ref()
    }

    /// Whether every chip answered and passed its PEC check.
    pub fn all_ok(&self) -> bool {
        self.chips
            .iter()
            .all(|chip| chip.is_some_and(|chip| chip.pec().is_success()))
    }
}

#[cfg(feature = "defmt")]
impl<G: Copy, E, const N: usize> defmt::Format for Responses<G, E, N> {
    fn format(&self, f: defmt::Formatter) {
        defmt::write!(
            f,
            "Responses {{ chips: {=usize}, all_ok: {=bool}, line_a_failed: {=bool}, line_b_failed: {=bool} }}",
            N,
            self.all_ok(),
            self.errors[0].is_some(),
            self.errors[1].is_some()
        )
    }
}

/// Newtype representing how many chips are on Line A.
/// This newtype is kinda pointless but it is useful for docs since it
/// is somewhat hard to remember what this value actually means.
/// 
/// TLDR: The inner represents the number of chips that are on Line A. In other words, chips `0..self.0` are reached from Line A, with `self.0` not being inclusive.
/// This defaults to `N`, where `N` is the total number of chips there are across both lines, no matter what the split is. So if `N = 10`, `OnLineA(10)` would mean that
/// all 10 chips are on Line A. "All 10 chips" corresponds to Chip 0 through Chip 9 (since the chips are 0-indexed).
/// Likewise, `OnLineA(0)` would mean that all 10 chips are on Line B.
/// 
/// Examples:
/// - `OnLineA(10)`: All 10 chips are on Line A
/// - `OnLineA(0)`: All 10 chips are on Line B
/// - `OnLineA(4)`: Chips `0..4` are on Line A, and chips `4..10` are on Line B. Aka: Chip 0 through Chip 3 are on Line A, and Chip 4 through Chip 9 are on Line B.
/// - `OnLineA(6)`: Chips `0..6` are on Line A, and chips `6..10` are on Line B. Aka: Chip 0 through Chip 5 are on Line A, and Chip 6 through Chip 9 are on Line B.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct OnLineA(pub usize);
impl From<usize> for OnLineA {
    fn from(num: usize) -> Self {
        Self(num)
    }
}
impl From<OnLineA> for usize {
    fn from(num: OnLineA) -> Self {
        num.0
    }
}

/// Two isoSPI lines reaching `N` chips, plus the state tracked for those chips.
///
#[doc = docs::isospi_indexing_example!()]
pub struct Api<SPI, const N: usize> {
    line_a: Line<SPI, N>,
    line_b: Line<SPI, N>,
    on_line_a: OnLineA,
    /// State metadata for each chip
    pub(crate) chips: [ChipState; N],

    /// Each chip's cached Config A register state. An entry is `None` until that register
    /// is written to.
    /// 
    /// This is used to automatically reconfigure the registers if a sleep is detected.
    pub(crate) config_a: [Option<ConfigA>; N],
    /// Same thing as `config_a` but for Config B !!!!
    pub(crate) config_b: [Option<ConfigB>; N],
}

#[cfg(feature = "defmt")]
impl<SPI, const N: usize> defmt::Format for Api<SPI, N> {
    fn format(&self, f: defmt::Formatter) {
        defmt::write!(
            f,
            "Api {{ chips: {=usize}, on_line_a: {=usize} }}",
            N,
            usize::from(self.on_line_a)
        )
    }
}

impl<SPI: SpiDevice, const N: usize> Api<SPI, N> {
    /// Builds a Api. This defaults to every chip routed to line A.
    pub const fn new(line_a: Line<SPI, N>, line_b: Line<SPI, N>) -> Self {
        Self {
            line_a,
            line_b,
            on_line_a: OnLineA(N),
            chips: [ChipState {
                command_count: CommandCount::new(),
                command_count_resets: 0,
                last_contacted: None,
                pec_success_count: 0,
                pec_failed_count: 0,
            }; N],
            config_a: [None; N],
            config_b: [None; N],
        }
    }

    /// Releases both lines' SPI devices.
    pub(crate) fn release(self) -> (SPI, SPI) {
        (self.line_a.release(), self.line_b.release())
    }

    /// Per-chip metadata.
    pub(crate) const fn chips(&mut self) -> &[ChipState; N] {
        &mut self.chips
    }

    /// CRATE PRIVATE! Adopts every chip's reported counter as its expected one.
    ///
    /// This should be called after a wakeup or any time the chips may have slept (which happens automatically when the watchdog expires).
    /// u_TODO: it might be a good idea to have `Api` own a thread that detects the SLEEP bit from the status register and calls this automatically. I guess probably just wherever the isospi recovery thread works
    pub(crate) fn resync_command_counters(&mut self) {
        for chip in &mut self.chips {
            chip.command_count().resync()
        }
    }

    /// CRATE PRIVATE! How many chips are currently routed to line A.
    pub(crate) const fn split(&self) -> OnLineA {
        self.on_line_a
    }

    /// CRATE PRIVATE! Routes chips `0..on_line_a` to line A and the rest to line B.
    ///
    /// This only changes routing. You have to assert `COMM_BK` on the chips on either side of the break first, or
    /// commands will keep moving across it.
    pub(crate) fn set_split(&mut self, on_line_a: OnLineA) -> Result<(), Error<SPI::Error>> {
        if usize::from(on_line_a) > N {
            return Err(Error::TooManyDevices);
        }
        self.on_line_a = on_line_a;
        Ok(())
    }

    /// Which line a chip is currently reached from.
    pub fn line_of(&self, chip: usize) -> LineId {
        if chip < self.on_line_a.into() {
            LineId::A
        } else {
            LineId::B
        }
    }

    /// PRIVATE! A chip's index from the point of view of its line.
    fn index_of(&self, chip: usize) -> usize {
        if chip < self.on_line_a.into() {
            chip
        } else {
            N - 1 - chip
        }
    }

    /// PRIVATE! How many chips are on a line.
    fn count(&self, line: LineId) -> usize {
        match line {
            LineId::A => self.on_line_a.into(),
            LineId::B => N - usize::from(self.on_line_a),
        }
    }

    /// PRIVATE! Gets a mut reference to a Line
    fn line_mut(&mut self, line: LineId) -> &mut Line<SPI, N> {
        match line {
            LineId::A => &mut self.line_a,
            LineId::B => &mut self.line_b,
        }
    }

    /// PRIVATE! Applies a dispatched frame to the tracked counters of every chip on `line`.
    fn note(&mut self, line: LineId, frame: CommandFrame) {
        if !frame.increments() && !frame.resets_counter() {
            return;
        }

        let chips = match line {
            LineId::A => 0..self.on_line_a.into(),
            LineId::B => self.on_line_a.into()..N,
        };

        for chip in chips {
            let expected = &mut self.chips[chip].command_count.expected;
            // 0 is reserved for resets, so the counter rolls over to 1. See page 53 of the datasheet.
            *expected = match () {
                _ if frame.resets_counter() => 0,
                _ if *expected >= CCNT_MAX => 1,
                _ => *expected + 1,
            };
        }
    }

    /// PRIVATE! Reads a register group from every chip on one line.
    async fn read_line<G: ReadableGroup>(
        &mut self,
        line: LineId,
    ) -> Result<crate::line::Responses<G, N>, Error<SPI::Error>> {
        let count = self.count(line);
        if count == 0 {
            return Ok(crate::line::Responses::empty());
        }

        self.note(line, G::READ_COMMAND);
        self.line_mut(line).read::<G>(count).await
    }

    /// Reads a register group from every chip.
    pub(crate) async fn read<G: ReadableGroup>(&mut self) -> Responses<G, SPI::Error, N> {
        let line_a = self.read_line::<G>(LineId::A).await;
        let line_b = self.read_line::<G>(LineId::B).await;

        let mut chips = [None; N];

        for (chip, slot) in chips.iter_mut().enumerate() {
            let responses = match self.line_of(chip) {
                LineId::A => &line_a,
                LineId::B => &line_b,
            };

            let Ok(responses) = responses else { continue };
            let Some(response) = responses.get(self.index_of(chip)) else {
                continue;
            };

            // A failed PEC corrupts the counter bits too, so only believe a counter that checked out.
            if response.pec().is_success() {
                self.chips[chip].command_count.reported = response.command_counter();
                self.chips[chip].last_contacted = Some(embassy_time::Instant::now());
                self.chips[chip].pec_success_count += 1;
            } else {
                self.chips[chip].pec_failed_count += 1;
            }
            *slot = Some(*response);
        }

        Responses {
            chips,
            errors: [line_a.err(), line_b.err()],
        }
    }

    /// Writes one register group per chip. `groups` is indexed in logical chip order.
    pub(crate) async fn write<G: WritableGroup>(
        &mut self,
        groups: &[G; N],
    ) -> Result<(), Error<SPI::Error>> {
        let (count_a, count_b) = (self.count(LineId::A), self.count(LineId::B));

        let line_a = if count_a > 0 {
            self.note(LineId::A, G::WRITE_COMMAND);
            self.line_a.write(&groups[..count_a]).await
        } else {
            Ok(())
        };

                let line_b = if count_b > 0 {
            // Line B reaches its chips in reverse logical order.
            let reversed: [G; N] = core::array::from_fn(|i| groups[N - 1 - i]);
            self.note(LineId::B, G::WRITE_COMMAND);
            self.line_b.write(&reversed[..count_b]).await
        } else {
            Ok(())
        };

        // Cache the config groups so a chip that sleeps (and resets its registers to defaults)
        // can be restored to what the application last asked for.
        if G::WRITE_COMMAND == ConfigA::WRITE_COMMAND {
            for (chip, group) in groups.iter().enumerate() {
                self.config_a[chip] = Some(ConfigA::from_bytes(group.to_bytes()));
            }
        } else if G::WRITE_COMMAND == ConfigB::WRITE_COMMAND {
            for (chip, group) in groups.iter().enumerate() {
                self.config_b[chip] = Some(ConfigB::from_bytes(group.to_bytes()));
            }
        }

        line_a.and(line_b)
    }

    /// Sends a command to every chip on both lines.
    pub(crate) async fn command(&mut self, command: Command) -> Result<(), Error<SPI::Error>> {
        let line_a = self.command_line(LineId::A, command).await;
        let line_b = self.command_line(LineId::B, command).await;
        line_a.and(line_b)
    }

    /// CRATE PRIVATE! Sends a command to the chips on one line.
    pub(crate) async fn command_line(
        &mut self,
        line: LineId,
        command: Command,
    ) -> Result<(), Error<SPI::Error>> {
        if self.count(line) == 0 {
            return Ok(());
        }

        self.note(line, command.frame());
        self.line_mut(line).command(command).await
    }

    /// Sends a poll command to both lines and reports whether every chip has finished.
    ///
    /// This does not wait! It just returns a bool, so you need to keep polling it until it returns true (indicating that every chip has finished).
    /// If you want that done automatically for you, see the `*_autoconvert()` helpers.
    pub(crate) async fn poll(&mut self, command: Command) -> Result<bool, Error<SPI::Error>> {
        let line_a = self.poll_line(LineId::A, command).await;
        let line_b = self.poll_line(LineId::B, command).await;
        Ok(line_a? && line_b?)
    }

    /// PRIVATE! Sends a poll command to one line. A line with no chips reports `true`.
    async fn poll_line(
        &mut self,
        line: LineId,
        command: Command,
    ) -> Result<bool, Error<SPI::Error>> {
        let count = self.count(line);
        if count == 0 {
            return Ok(true);
        }

        self.note(line, command.frame());
        self.line_mut(line).poll(command, count).await
    }

    /// CRATE PRIVATE! Wakes every chip on both lines out of the idle or sleep state.
    ///
    /// Chips that were asleep come back with their counters at 0. This is not detected automatically! So, it is a
    /// good idea to follow this with a read and `sync_command_counters()`.
    pub(crate) async fn wakeup(&mut self) -> Result<(), Error<SPI::Error>> {
        let (count_a, count_b) = (self.count(LineId::A), self.count(LineId::B));
        self.line_a.wakeup(count_a).await?;
        self.line_b.wakeup(count_b).await
    }

    /// CRATE PRIVATE! Counts the chips reachable on one line. This stops at the first bad PEC (note: this means that comm
    /// errors that result in a bad PEC could look like the line end, so probably call this a few times).
    ///
    /// Useful for locating a break. This doesn't change the split.
    pub(crate) async fn detect_chips(&mut self, line: LineId) -> Result<usize, Error<SPI::Error>> {
        self.line_mut(line).detect_chips().await
    }
}

/// # Automatic conversions
///
/// Each helper starts a conversion, waits the time it is expected to take, and then polls until every
/// chip reports that it's finished.
impl<SPI: SpiDevice, const N: usize> Api<SPI, N> {
    /// Sends the `start` command, waits `conversion_ms` milliseconds, and then polls until done (or `timeout_ms` elapses).
    async fn autoconvert(
        &mut self,
        start: Command,
        poll: Command,
        conversion_ms: u64,
        timeout_ms: u64,
    ) -> Result<(), Error<SPI::Error>> {
        use embassy_time::{Duration, Instant, Timer};

        /// How long to wait between polls once the expected conversion time has elapsed.
        const POLL_INTERVAL_MS: u64 = 1;

        let deadline = Instant::now() + Duration::from_millis(timeout_ms);

        self.command(start).await?;
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

    /// Starts a cell voltage conversion (ADCV) and waits for it to finish.
    pub(crate) async fn adcv_autoconvert(
        &mut self,
        redundancy: commands::adc::AdcvRedundancy,
        acquisition: commands::adc::AutoAcquisition,
        reset_filter: commands::adc::ResetFilter,
        open_wire: commands::adc::OpenWire,
        timeout_ms: u64,
    ) -> Result<(), Error<SPI::Error>> {
        let conversion_ms = match redundancy {
            commands::adc::AdcvRedundancy::Enabled => conversion_times::S_ADC_MS,
            commands::adc::AdcvRedundancy::Disabled => conversion_times::C_ADC_MS,
        };

        self.autoconvert(
            commands::adc::adcv(redundancy, acquisition.into(), reset_filter, open_wire),
            commands::adc::plcadc(),
            conversion_ms,
            timeout_ms,
        )
        .await
    }

    /// Starts an S-ADC conversion (ADSV) and waits for it to finish.
    pub(crate) async fn adsv_autoconvert(
        &mut self,
        acquisition: commands::adc::AutoAcquisition,
        open_wire: commands::adc::OpenWire,
        timeout_ms: u64,
    ) -> Result<(), Error<SPI::Error>> {
        self.autoconvert(
            commands::adc::adsv(acquisition.into(), open_wire),
            commands::adc::plsadc(),
            conversion_times::S_ADC_MS,
            timeout_ms,
        )
        .await
    }

    /// Starts an AUX conversion (ADAX) and waits for it to finish.
    /// 
    /// This doesn't account for time added by SOAKON (see Config A). It should still work if SOAKON is
    /// enabled, it just might not be as efficient since it will poll at the same frequency as when SOAKON
    /// is not enabled.
    pub(crate) async fn adax_autoconvert(
        &mut self,
        open_wire: commands::adc::OpenWireAux,
        pull: commands::adc::Pull,
        channel: commands::adc::Aux1InputSelection,
        timeout_ms: u64,
    ) -> Result<(), Error<SPI::Error>> {
        self.autoconvert(
            commands::adc::adax(open_wire, pull, channel),
            commands::adc::plaux(),
            conversion_times::AUX_MS,
            timeout_ms,
        )
        .await
    }

    /// Starts an AUX2 conversion (ADAX2) and waits for it to finish.
    pub(crate) async fn adax2_autoconvert(
        &mut self,
        channel: commands::adc::Aux2InputSelection,
        timeout_ms: u64,
    ) -> Result<(), Error<SPI::Error>> {
        self.autoconvert(
            commands::adc::adax2(channel),
            commands::adc::plaux2(),
            conversion_times::AUX2_MS,
            timeout_ms,
        )
        .await
    }
}
