//! Service for ADBMS6830B

use embassy_time::{Timer, Duration};
use embedded_hal_async::spi::SpiDevice;
use crate::{
    chip::{
        registers::ReadableGroup,
        registers::WritableGroup,
        registers::config_a::ConfigA,
        registers::config_b::ConfigB,
        commands,
    },
    manager::{
        Api,
        Responses,
        ChipState,
    },
    line::{
        Line,
        Error,
        conversion_times
    }
};
use embassy_sync::mutex::Mutex;
use embassy_sync::blocking_mutex::raw::RawMutex;

/// How often the service should run, in ms.
const SERVICE_FREQUENCY_MS: u64 = 300;

/// Tracks each chip's PEC failure rate and reports where a break has opened.
mod accumulator {
    use embassy_time::{Duration, Instant};
    use super::ChipState;

    /// How long each evaluation window lasts.
    const SEGMENT_ISOSPI_EVAL_PERIOD_MS: u64 = 4000;

    /// Fewest reads a chip must have taken part in before its failure rate is actually considered as meaning anything.
    ///
    /// this is here to protect against a tiny sample size in the accumulator incorrectly flagging a break. Basically,
    /// if an accumulation window is less than this, there is not enough data to conclude that a PCT of failed PECs actually
    /// indicates a break. So, if a window has less than this, we ignore that window.
    const SEGMENT_ISOSPI_MIN_ATTEMPTS: usize = 16;

    /// Percentage of reads that must fail their PEC for a chip to look unreachable.
    ///
    /// A break will cause the affected chips' reads to fail essentially every
    /// time. So, this value is meant to be quite high. 
    /// This sits well above any plausible noise level but leaves margin for a link
    /// that is failing intermittently rather than completely.
    const SEGMENT_ISOSPI_PEC_FAILURE_RATIO_PCT: usize = 75;

    /// Fewest reads in a single update before that update's failure rate can open a window.
    ///
    /// This is kinda meant to take the place of `SEGMENT_ISOSPI_PEC_ACCUM_START_THRESH` from the C code. It serves
    /// a similar-ish function (in that it is a blocker for an accumulator window being allowed to start), but it uses sample
    /// size rather than absolute error count.
    const SEGMENT_ISOSPI_ARM_MIN_ATTEMPTS: usize = 2;


    enum State {
        /// There is no window open. Watching each update for a chip whose reads are failing.
        /// Basically, everything is normal!
        /// 
        /// This is the default state at startup.
        Idle,
        /// Summing PEC outcomes until `until`, at which point the window is evaluated.
        Accumulating { until: Instant },
        /// A break was reported and the split has been moved.
        /// 
        /// There's only one split point since we only have two isospi lines. So we can't
        /// set up multiple breaks after the first one.
        /// 
        /// u_TODO: add in the verification stuff here for this state
        Latched,
    }

    /// result of an `.update()` call. Basically just here to
    /// instruct the caller if they need to do anything.
    pub enum UpdateResult {
        /// we are good. caller doesn't need to do anything
        Okay,

        /// uh oh! We have a break to process
        BreakDetected {
            /// Index of the chip at which the break should be set
            break_chip_index: usize 
        },
    }

    // u_TODO - probably a good idea to expose the diagnostics being tracked by accumulator just for
    // debugging purposes, maybe we could sent it over a CAN message or something.
    // also, probably a good idea to track the number of times the accumulator chain of events gets blocked
    // by `SEGMENT_ISOSPI_ARM_MIN_ATTEMPTS` or `SEGMENT_ISOSPI_MIN_ATTEMPTS`, since that could help diagnose if our
    // message rate is too slow for recovery to work properly

    pub struct Accumulator<const N: usize> {
        state: State,

        /// PEC failures counted for each chip during this window.
        failed: [usize; N],

        /// Total read attempts for this chip during this window. This is just a sum of pec_success + pec_failed.
        attempts: [usize; N],

        /// Each chip's `pec_failed_count` as of the last update.
        /// 
        /// This is used because what gets accumulated is the difference in errors
        /// between updates.
        last_failed: [usize; N],

        /// Each chip's total PEC outcomes as of the last update. Paired with `last_failed`.
        last_attempts: [usize; N],

        /// Whether `last_failed` and `last_attempts` hold a real reading yet.
        /// 
        /// This is used to make sure the first reading is saved as a baseline rather than
        /// trying to calculate a change from it.
        seeded: bool,
    }
    impl<const N: usize> Accumulator<N> {
        /// Default initialization for the accumulator. Will be idle by default.
        pub(crate) const fn new() -> Self {
            Self {
                state: State::Idle,
                failed: [0; N],
                attempts: [0; N],
                last_failed: [0; N],
                last_attempts: [0; N],
                seeded: false,
            }
        }

        /// PRIVATE! Helper that zeroes this window's tallies for each chip.
        fn reset_chips(&mut self) {
            self.failed = [0; N];
            self.attempts = [0; N];
        }

        /// PRIVATE! Adds each chip's PEC outcomes since the last update to this window.
        fn update_chips(&mut self, chips: &[ChipState; N]) {
            for (chip, state) in chips.iter().enumerate() {
                let failed = state.pec_failed_count();
                let attempts = failed + state.pec_success_count();

                if self.seeded {
                    self.failed[chip] += failed - self.last_failed[chip];
                    self.attempts[chip] += attempts - self.last_attempts[chip];
                }

                self.last_failed[chip] = failed;
                self.last_attempts[chip] = attempts;
            }

            self.seeded = true;
        }

        /// PRIVATE! Whether this chip failed the ratio threshold over at least `min_attempts` reads.
        /// 
        /// Arming and the final verdict use the same failure rate and differ only in how much
        /// evidence they ask for.
        fn failure_rate_exceeded(&self, chip: usize, min_attempts: usize) -> bool {
            self.attempts[chip] >= min_attempts
                && (self.failed[chip] * 100)
                    >= (self.attempts[chip] * SEGMENT_ISOSPI_PEC_FAILURE_RATIO_PCT)
        }

        /// PRIVATE! Whether this chip's reads since the last update are bad enough to open a window.
        fn is_chip_arming(&self, chip: usize) -> bool {
            self.failure_rate_exceeded(chip, SEGMENT_ISOSPI_ARM_MIN_ATTEMPTS)
        }

        /// PRIVATE! Whether this chip failed enough of this window's reads to pass our threshold for problematic.
        fn is_chip_failed(&self, chip: usize) -> bool {
            self.failure_rate_exceeded(chip, SEGMENT_ISOSPI_MIN_ATTEMPTS)
        }

        /// PRIVATE! Opens an accumulation window.
        /// 
        /// this purposefully keeps whatever is already tallied so the update that armed the
        /// window counts towards it.
        fn open_window(&mut self) {
            self.state = State::Accumulating {
                until: Instant::now() + Duration::from_millis(SEGMENT_ISOSPI_EVAL_PERIOD_MS),
            };
        }

        /// Updates the PEC error accumulator state, and detects if a break should be set.
        /// 
        /// This should be called in every iteration of the Service runner. This will return either `UpdateResult::Okay`, meaning that
        /// the service runner doesn't need to do anything regarding a break right now, or `UpdateResult::BreakDetected`, after which the
        /// service runner must handle the break accordingly.
        pub(crate) fn update(&mut self, chips: &[ChipState; N]) -> UpdateResult {
            self.update_chips(chips);

            match self.state {
                // case: no window is open, so the tallies hold only this update's reads (they get
                // cleared every update we stay here). A chip failing its reads right now is what
                // opens a window, so the window lines up with the start of the problem.
                State::Idle => {
                    if (0..N).any(|chip| self.is_chip_arming(chip)) {
                        self.open_window();
                    } else {
                        self.reset_chips();
                    }
                }

                // case: the window is up, so see whether what it caught looks like a break.
                State::Accumulating { until } if Instant::now() >= until => {
                    // A break takes out every chip past it, so the first unreachable chip only
                    // means a break if every chip after it is unreachable too.
                    let first = (0..N).find(|&chip| self.is_chip_failed(chip));

                    if let Some(idx) = first {
                        if (idx..N).all(|chip| self.is_chip_failed(chip)) {
                            self.reset_chips();
                            self.state = State::Latched;
                            return UpdateResult::BreakDetected { break_chip_index: idx };
                        }
                    }

                    // Whatever the window caught wasn't a break, so go back to watching for
                    // a fresh spike rather than measuring straight through.
                    self.reset_chips();
                    self.state = State::Idle;
                }

                // case: we are still in the middle of filling the window so don't need to do anything rn
                State::Accumulating { .. } => {}

                // case: the one break we can route around has already been handled, so stop counting.
                State::Latched => self.reset_chips(),
            }

            // if we get here we are okay
            UpdateResult::Okay
        }
    }
}

pub struct Service<MUTEX: RawMutex, SPI: SpiDevice, const N: usize> {
    api: Mutex<MUTEX, Api<SPI, N>>,
}
impl<MUTEX: RawMutex, SPI: SpiDevice, const N: usize> Service<MUTEX, SPI, N> {
    /// Creates a new service.
    pub const fn new(line_a: Line<SPI, N>, line_b: Line<SPI, N>) -> Self {
        Self {
            api: Mutex::new(Api::new(line_a, line_b)),
        }
    }

    /// Runs the service.
    /// 
    /// This function will never return and is intended to be ran in a
    /// dedicated task.
    pub async fn run(&self) -> ! {
        use crate::service::accumulator::{Accumulator, UpdateResult};

        // The runner is the only thing that uses this so it doesn't need to be part of `Service`.
        let mut accumulator = Accumulator::<N>::new();

        loop {
            {
                let mut api = self.api.lock().await;

                // this should run first so the sleep detection reads count towards the accumulator update break detection
                let _ = self.handle_sleep_detection(&mut api).await;

                let chips = *api.chips();
                if let UpdateResult::BreakDetected { break_chip_index } = accumulator.update(&chips) {
                    self.handle_break_detected(&mut api, break_chip_index).await;
                }
            }

            Timer::after(Duration::from_millis(SERVICE_FREQUENCY_MS)).await
        }
    }
}

/// # Interaction
/// 
/// This impl block contains the methods that let users send commands to the
/// API. For context: `.run()` just manages the isoSPI detection, recovery, and split
/// state stuff. The actual data gathering and configuration is still managed by the user.
/// These functions allow the user to send different commands at their own frequencies as
/// required by their specific application.
/// 
/// These functions exist (rather than just exposing the raw `api` directly) mainly
/// just to bound the lifetime of the mutex. If callers were the ones managing the
/// MutexGaurd directly they might accidentally hold it for too long or something. So
/// this provides a nice grouping of all the places the mutex is locked.
impl<MUTEX: RawMutex, SPI: SpiDevice, const N: usize> Service<MUTEX, SPI, N> {
    /// Reads a register group from every chip.
    pub(crate) async fn read<G: ReadableGroup>(&self) -> Responses<G, SPI::Error, N> {
        let mut api = self.api.lock().await;
        api.read().await
    }

    /// Writes one register group per chip. `groups` is indexed in logical chip order.
    pub(crate) async fn write<G: WritableGroup>(&self, groups: &[G; N]) -> Result<(), Error<SPI::Error>> {
        let mut api = self.api.lock().await;
        api.write(groups).await
    }

    /// Sends a command to every chip on both lines.
    pub(crate) async fn command(&self, command: commands::Command) -> Result<(), Error<SPI::Error>> {
        let mut api = self.api.lock().await;
        api.command(command).await
    }

    /// Sends a poll command to both lines and reports whether every chip has finished.
    ///
    /// This does not wait! It just returns a bool, so you need to keep polling it until it returns true (indicating that every chip has finished).
    /// If you want that done automatically for you, see the `*_autoconvert()` helpers.
    pub async fn poll(&self, command: commands::Command) -> Result<bool, Error<SPI::Error>> {
        let mut api = self.api.lock().await;
        api.poll(command).await
    }

    /// Starts a cell voltage conversion (ADCV) and waits for it to finish.
    pub async fn adcv_autoconvert(&self, redundancy: commands::adc::AdcvRedundancy, acquisition: commands::adc::AutoAcquisition, reset_filter: commands::adc::ResetFilter, open_wire: commands::adc::OpenWire, timeout_ms: u64) -> Result<(), Error<SPI::Error>> {
        let mut api = self.api.lock().await;
        api.adcv_autoconvert(redundancy, acquisition, reset_filter, open_wire, timeout_ms).await
    }

    /// Starts an S-ADC conversion (ADSV) and waits for it to finish.
    pub async fn adsv_autoconvert(&self, acquisition: commands::adc::AutoAcquisition, open_wire: commands::adc::OpenWire, timeout_ms: u64) -> Result<(), Error<SPI::Error>> {
        let mut api = self.api.lock().await;
        api.adsv_autoconvert(acquisition, open_wire, timeout_ms).await
    }

    /// Starts an AUX conversion (ADAX) and waits for it to finish.
    /// 
    /// This doesn't account for time added by SOAKON (see Config A). It should still work if SOAKON is
    /// enabled, it just might not be as efficient since it will poll at the same frequency as when SOAKON
    /// is not enabled.
    pub(crate) async fn adax_autoconvert(&self, open_wire: commands::adc::OpenWireAux, pull: commands::adc::Pull, channel: commands::adc::Aux1InputSelection, timeout_ms: u64, ) -> Result<(), Error<SPI::Error>> {
        let mut api = self.api.lock().await;
        api.adax_autoconvert(open_wire, pull, channel, timeout_ms).await
    }

    /// Starts an AUX2 conversion (ADAX2) and waits for it to finish.
    pub(crate) async fn adax2_autoconvert(&self, channel: commands::adc::Aux2InputSelection, timeout_ms: u64) -> Result<(), Error<SPI::Error>> {
        let mut api = self.api.lock().await;
        api.adax2_autoconvert(channel, timeout_ms).await
    }

    /// Per-chip metadata.
    pub(crate) async fn chips(&self) -> [ChipState; N] {
        let mut api = self.api.lock().await;
        let copy: [ChipState; N] = *api.chips();
        copy
    }
}

/// # Helpers
/// 
/// Internal helpers for the service.
impl<MUTEX: RawMutex, SPI: SpiDevice, const N: usize> Service<MUTEX, SPI, N> {
    /// PRIVATE! Logic for when a break has been detected.
    /// 
    /// This should be called when a break is detected. `break_chip_index` should be
    /// passed in here.
    async fn handle_break_detected(&self, api: &mut Api<SPI, N>, break_chip_index: usize) {

    }

    /// PRIVATE! Detects chips that have slept, re-baselines their command counters,
    /// restores the configuration they lost.
    async fn handle_sleep_detection(&self, api: &mut Api<SPI, N>) -> Result<(), Error<SPI::Error>> {
        use crate::chip::registers:: {
            status::StatusC,
            clear::ClearFlags,
            clear::types::ClearAction,
            status::types::c::SleepModeDetection,
        };

        // RDSTATC doesn't increment the command counter, so this doesn't perturb what we're measuring.
        let mut statuses = api.read::<StatusC>().await;
        if statuses.all_ok() && statuses.iter().flatten().all(|r| r.data().sleep() == SleepModeDetection::SleepModeNotDetected) {
            return Ok(());
        }

        // Something is off, so wake the chain and take the observation we can trust.
        api.wakeup().await?;
        statuses = api.read::<StatusC>().await;

        let mut any_slept = false;
        let mut clears = [ClearFlags::new(); N];

        for (chip, response) in statuses.iter().enumerate() {
            let Some(response) = response else { continue };
            if response.pec().is_failed() { continue; }

            if response.data().sleep() == SleepModeDetection::SleepModeDetected {
                any_slept = true;
                api.chips[chip].reset_command_count(response.command_counter());
                // Waking into standby also sets both rail UV flags, so clear them alongside SLEEP
                // or the application sees undervoltage faults that never happened.
                clears[chip] = clears[chip]
                    .with_cl_sleep(ClearAction::Clear)
                    .with_cl_vauv(ClearAction::Clear)
                    .with_cl_vduv(ClearAction::Clear);
            } else {
                api.chips[chip].command_count.resync();
            }
        }

        // Sleep resets every register to its default. so we need to put back what the application last asked for.
        if any_slept {
            let config_a: [ConfigA; N] = core::array::from_fn(|i| api.chips[i].config_a.unwrap_or(ConfigA::new()));
            api.write(&config_a).await?;

            let config_b: [ConfigB; N] = core::array::from_fn(|i| api.chips[i].config_b.unwrap_or(ConfigB::new()));
            api.write(&config_b).await?;

            // write the clears
            api.write(&clears).await
        } else {
            Ok(())
        }
    }
}