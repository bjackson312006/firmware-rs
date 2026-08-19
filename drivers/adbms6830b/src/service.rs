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

/// The thing that sums PEC errors
/// and then resets every once and a while.
mod accumulator {
    use embassy_time::{Duration, Instant};
    use super::ChipState;

    /// How often the accumulator should reset itself.
    const SEGMENT_ISOSPI_ACCUM_PERIOD_MS: u64 = 4000;

    /// Threshold for accumulation timer.
    /// 
    /// Set just above the PEC error sum noise level per cycle,
    /// so random noise doesn’t start the accumulation window.
    const SEGMENT_ISOSPI_PEC_ACCUM_START_THRESH: usize = 5;

    /// Break detect threshold.
    /// 
    /// PEC errors > this value in the accumulation window indicate a break.
    const SEGMENT_ISOSPI_PEC_ERROR_THRESHOLD: usize = 25;

    enum State {
        /// Nothing unusual is going on. Watching for a spike in PEC errors.
        Idle,
        /// A spike in PEC errors started up the window. Summing PEC errors until `until`.
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

    pub struct Accumulator<const N: usize> {
        state: State,

        /// Accumulator's PEC counts for each chip.
        chips: [usize; N],

        /// Each chip's `pec_failed_count` as of the last update.
        /// 
        /// This is used because what gets accumulated is the difference in errors
        /// between updates.
        last_seen: [usize; N],

        /// Whether `last_seen` holds a real reading yet.
        /// 
        /// The service can start up long after the application has been reading, so the
        /// first update only takes a baseline instead of counting all of history as one delta.
        seeded: bool,
    }
    impl<const N: usize> Accumulator<N> {
        /// Default initialization for the accumulator. Will be idle by default.
        pub(crate) const fn new() -> Self {
            Self {
                state: State::Idle,
                chips: [0; N],
                last_seen: [0; N],
                seeded: false,
            }
        }

        /// PRIVATE! Helper that zeroes the PEC count for each chip.
        fn reset_chips(&mut self) {
            for chip in &mut self.chips {
                *chip = 0;
            }
        }

        /// PRIVATE! Accumulates each chip's PEC failures since the last update.
        fn update_chips(&mut self, chips: &[ChipState; N]) {
            for (chip, state) in chips.iter().enumerate() {
                let count = state.pec_failed_count();
                if self.seeded {
                    self.chips[chip] += count - self.last_seen[chip];
                }
                self.last_seen[chip] = count;
            }

            self.seeded = true;
        }

        /// Updates the PEC error accumulator state, and detects if a break should be set.
        /// 
        /// This should be called in every iteration of the Service runner. This will return either `UpdateResult::Okay`, meaning that
        /// the service runner doesn't need to do anything regarding a break right now, or `UpdateResult::BreakDetected`, after which the
        /// service runner must handle the break accordingly.
        pub(crate) fn update(&mut self, chips: &[ChipState; N]) -> UpdateResult {
            self.update_chips(chips);

            match self.state {
                // case: accumulator is currently idling. So, we need to check if there are more PEC errors
                // than normal, and if so, start up an accumulation session. If there aren't more PEC errors than normal, we can stay idling.
                State::Idle => {
                    if self.chips.iter().any(|c| *c > SEGMENT_ISOSPI_PEC_ACCUM_START_THRESH) {
                        self.state = State::Accumulating {
                            until: Instant::now() + Duration::from_millis(SEGMENT_ISOSPI_ACCUM_PERIOD_MS),
                        };
                    } else {
                        // we have not exceeded the theshold so nothing out of the ordinary. can reset
                        self.reset_chips();
                    }
                }

                // case: we are actively accumulating, and the Instant::now() is now past our original `until`.
                // TLDR the accumulation period is up, so we need to check if what we accumulated constitutes a line break
                State::Accumulating { until } if Instant::now() >= until => {
                    self.state = State::Idle;

                    // a break will mess up every chip past it. so, the first chip over the threshold
                    // only means a break if all the chips after it are over it too.
                    if let Some(idx) = self.chips.iter().position(|c| *c > SEGMENT_ISOSPI_PEC_ERROR_THRESHOLD) {
                        if self.chips[idx..].iter().all(|c| *c > SEGMENT_ISOSPI_PEC_ERROR_THRESHOLD) {
                            self.reset_chips();
                            self.state = State::Latched;
                            return UpdateResult::BreakDetected { break_chip_index: idx };
                        }
                    }

                    self.reset_chips();
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
            match accumulator.update(&self.chips().await) {
                UpdateResult::BreakDetected { break_chip_index } => {
                    self.handle_break_detected(break_chip_index).await
                },
                UpdateResult::Okay => {},
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
    async fn handle_break_detected(&self, break_chip_index: usize) {

    }

    /// PRIVATE! Detects chips that have slept, re-baselines their command counters,
    /// restores the configuration they lost.
    async fn handle_sleep_detection(&self) -> Result<(), Error<SPI::Error>> {
        use crate::chip::registers:: {
            status::StatusC,
            clear::ClearFlags,
            clear::types::ClearAction,
            status::types::c::SleepModeDetection,
        };

        let mut api = self.api.lock().await;

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