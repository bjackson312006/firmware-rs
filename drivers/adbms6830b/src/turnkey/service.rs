//! Service for ADBMS6830B.

use embassy_time::{Timer, Duration, Instant};
use embedded_hal_async::spi::SpiDevice;
use crate::{
    chip::{
        commands, registers::{ReadableGroup, WritableGroup, config_a::ConfigA, config_b::ConfigB},
    }, line::{
        Error, Line
    },
};
use super::{
    api::{
        Api, ChipState, OnLineA, Responses,
    },
    diagnostics::{ChipStateDiagnostics, TimingDiagnostics},
    accumulator::{Accumulator, UpdateResult},
};
use embassy_sync::blocking_mutex::raw::RawMutex;
use core::cell::Cell;

/// Configuration parameters for the Service. This also includes constant defaults.
pub mod config {
    /// Default value for [ServiceConfig::service_frequency_ms].
    pub const SERVICE_FREQUENCY_MS: u64 = 300;
    /// Default value for [ServiceConfig::segment_isospi_eval_period_ms].
    pub const SEGMENT_ISOSPI_EVAL_PERIOD_MS: u64 = 4000;
    /// Default value for [ServiceConfig::segment_isospi_min_attempts_for_fail].
    pub const SEGMENT_ISOSPI_MIN_ATTEMPTS_FOR_FAIL: usize = 8;
    /// Default value for [ServiceConfig::segment_isospi_pec_failure_ratio_pct].
    pub const SEGMENT_ISOSPI_PEC_FAILURE_RATIO_PCT: u8 = 75;
    /// Default value for [ServiceConfig::segment_isospi_min_attempts_to_open_window].
    pub const SEGMENT_ISOSPI_MIN_ATTEMPTS_TO_OPEN_WINDOW: usize = 2;
    /// Default value for [ServiceConfig::segment_isospi_max_split_attempts].
    pub const SEGMENT_ISOSPI_MAX_SPLIT_ATTEMPTS: usize = 5;
    /// Default value for [ServiceConfig::segment_isospi_max_failed_verification_attempts].
    pub const SEGMENT_ISOSPI_MAX_FAILED_VERIFICATION_ATTEMPTS: usize = 5;

    /// Configuration constants for a Service. Probably just use `ServiceConfig::default()`
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    #[cfg_attr(feature = "defmt", derive(defmt::Format))]
    pub struct ServiceConfig {
        /// How often the service should run, in ms.
        /// 
        /// This defaults to [SERVICE_FREQUENCY_MS] when you use `ServiceConfig::default()`.
        pub service_frequency_ms: u64,
        /// PEC accumulator setting! How long each evaluation window lasts.
        /// 
        /// This defaults to [SEGMENT_ISOSPI_EVAL_PERIOD_MS] when you use `ServiceConfig::default()`.
        pub segment_isospi_eval_period_ms: u64,
        /// PEC accumulator setting! Fewest reads a chip must have taken part in before its failure rate is actually considered as meaning anything.
        ///
        /// This is here to protect against a tiny sample size in the accumulator incorrectly flagging a break. Basically,
        /// if an accumulation window is less than this, there is not enough data to conclude that a PCT of failed PECs actually
        /// indicates a break. So, if a window has less than this, we ignore that window.
        /// 
        /// This defaults to [SEGMENT_ISOSPI_MIN_ATTEMPTS_FOR_FAIL] when you use `ServiceConfig::default()`.
        /// 
        /// ### WARNING:
        /// You should make sure that, in normal operation, your application isn't reading at a frequency below this setting. If you set this
        /// above the number of reads your application makes during an accumulator window (see `segment_isospi_eval_period_ms`), you will effectively
        /// be disabling the isoSPI recovery routine because you will never reach the minimum number of PEC attempts to declare a failure.
        /// 
        /// As such, it is recommended to set this as low as possible, but above your PEC noise threshold. If this is set too low, you may experience false
        /// positives in regards to break detection due to a low sample size being susceptible to noise. However, if you set this too high, isoSPI recovery may never
        /// be able to occur.
        /// 
        /// Note: If you are unsure of the frequency at which your application makes read attempts during a PEC window, or you want to double-check that
        /// the Service's isoSPI recovery isn't skipping detection due to a read frequency lower than this setting, you can monitor the ServiceDiagnostics data
        /// that gets updated every Service cycle.
        pub segment_isospi_min_attempts_for_fail: usize,
        /// PEC accumulator setting! Percentage of reads that must fail their PEC for a chip to look unreachable.
        /// 
        /// This defaults to [SEGMENT_ISOSPI_PEC_FAILURE_RATIO_PCT] when you use `ServiceConfig::default()`.
        ///
        /// A break will cause the affected chips' reads to fail essentially every
        /// time. So, this value is meant to be quite high. 
        /// This should sit well above any plausible noise level but leaves margin for a link
        /// that is failing intermittently rather than completely.
        pub segment_isospi_pec_failure_ratio_pct: u8,
        /// PEC accumulator setting! Fewest reads in a single update before that update's failure rate can open a window.
        ///
        /// This is kinda meant to take the place of `SEGMENT_ISOSPI_PEC_ACCUM_START_THRESH` from the C code. It serves
        /// a similar-ish function (in that it is a blocker for an accumulator window being allowed to start), but it uses sample
        /// size rather than absolute error count.
        /// 
        /// This defaults to [SEGMENT_ISOSPI_MIN_ATTEMPTS_TO_OPEN_WINDOW] when you use `ServiceConfig::default()`.
        /// 
        /// ### WARNING:
        /// This should be set quite low. If you set this higher than the number of reads your application sends within `service_frequency_ms`, you may end up blocking
        /// the Service from ever opening an isoSPI recovery detection window. So, the safest value is probably something like 1 or 2. It should be set right above the PEC error sum noise level per cycle.
        /// 
        /// Note: If you are unsure of the frequency at which your application makes read attempts during a PEC window, or you want to double-check that
        /// the Service's isoSPI recovery isn't skipping detection due to a read frequency lower than this setting, you can monitor the ServiceDiagnostics data
        /// that gets updated every Service cycle.
        pub segment_isospi_min_attempts_to_open_window: usize,
        /// IsoSPI recovery setting! How many times the Service will try to apply a split before
        /// giving up on recovery.
        /// 
        /// This defaults to [SEGMENT_ISOSPI_MAX_SPLIT_ATTEMPTS] when you use `ServiceConfig::default()`.
        pub segment_isospi_max_split_attempts: usize,
        /// IsoSPI recovery setting! How many evaluation windows the Service will spend checking
        /// whether a split worked before giving up on recovery.
        ///
        /// This is the equivalent of `ISOSPI_RECOVERY_VERIFICATION_READS` from the C code.
        /// 
        /// This defaults to [SEGMENT_ISOSPI_MAX_FAILED_VERIFICATION_ATTEMPTS] when you use `ServiceConfig::default()`.
        pub segment_isospi_max_failed_verification_attempts: usize,
    }
    impl Default for ServiceConfig {
        fn default() -> Self {
            Self {
                service_frequency_ms: SERVICE_FREQUENCY_MS,
                segment_isospi_eval_period_ms: SEGMENT_ISOSPI_EVAL_PERIOD_MS,
                segment_isospi_min_attempts_for_fail: SEGMENT_ISOSPI_MIN_ATTEMPTS_FOR_FAIL,
                segment_isospi_pec_failure_ratio_pct: SEGMENT_ISOSPI_PEC_FAILURE_RATIO_PCT,
                segment_isospi_min_attempts_to_open_window: SEGMENT_ISOSPI_MIN_ATTEMPTS_TO_OPEN_WINDOW,
                segment_isospi_max_split_attempts: SEGMENT_ISOSPI_MAX_SPLIT_ATTEMPTS,
                segment_isospi_max_failed_verification_attempts: SEGMENT_ISOSPI_MAX_FAILED_VERIFICATION_ATTEMPTS,
            }
        }
    }
}

use super::diagnostics::ServiceDiagnostics;

pub struct Service<MUTEX: RawMutex, SPI: SpiDevice, const N: usize> {
    api: embassy_sync::mutex::Mutex<MUTEX, Api<SPI, N>>,
    diagnostics: embassy_sync::blocking_mutex::Mutex<MUTEX, Cell<Option<ServiceDiagnostics<N>>>>,
    
    /// The config this service holds. this is never meant to be mutated after
    /// construction time. The fields are all public tho so the user can declare the
    /// config with the nice declarative syntax. Just internally, service.rs isn't supposed to
    /// modify the config after we store it
    config: config::ServiceConfig,
}

impl<MUTEX: RawMutex, SPI: SpiDevice, const N: usize> Service<MUTEX, SPI, N> {
    /// Creates a new service.
    pub const fn new(line_a: Line<SPI, N>, line_b: Line<SPI, N>, config: config::ServiceConfig) -> Self {
        Self {
            api: embassy_sync::mutex::Mutex::new(Api::new(line_a, line_b)),
            diagnostics: embassy_sync::blocking_mutex::Mutex::new(Cell::new(None)),
            config,
        }
    }

    /// Runs the service.
    /// 
    /// This function will never return and is intended to be ran in a
    /// dedicated task.
    /// 
    /// See `run_with_diagnostics()` if you want to do something with the `ServiceDiagnostics` on
    /// every cycle, like reporting them over CAN.
    pub async fn run(&self) -> ! {
        self.run_with_diagnostics(async |_| {}).await
    }

    /// Runs the service, handing the cycle's `ServiceDiagnostics` to `on_diagnostics` each time.
    /// 
    /// This function will never return and is intended to be ran in a
    /// dedicated task. Use `run()` instead if you don't need the diagnostics.
    /// 
    /// This is like `run()`, but it allows you to read diagnostic data from each loop of the service.
    /// You can use this to send a CAN message with the diagnostic data every time the service runs:
    /// ```rust,no_run
    /// service.run_with_diagnostics(async |diagnostics| {
    ///     let _ = can_sender.try_send(function_that_turns_a_diagnostics_into_a_can_frame(diagnostics));
    /// }).await
    /// ```
    /// 
    /// Note: you can still access the diagnostics via the `.diagnostics()` method on `Service`, but this closure
    /// is useful if you want to access the diagnostics and run subsequent code at the same exact frequency the service
    /// is running (makes timing easier sometimes)
    /// 
    /// Other note: Whatever you put in the closure can affect the frequency of the service if there's a lot
    /// of awaiting going on
    pub async fn run_with_diagnostics(&self, mut on_diagnostics: impl AsyncFnMut(&ServiceDiagnostics<N>)) -> ! {
        // The runner is the only thing that uses this so it doesn't need to be part of `Service`.
        let mut accumulator = Accumulator::<N>::new(self.config);

        let mut sleep_detection_spi_error_count: usize = 0;
        let mut cycles_count: usize = 0;
        let mut break_detection_spi_error_count: usize = 0;

        // stuff for calculating how often the service runs actually
        let mut previous_loop_timestamp: Option<Instant> = None; // timestamp the service loop last ran
        let mut max_period = Duration::MIN; // the highest period between two service loops we have observed so far
        let mut max_lock_wait = Duration::MIN; // the highest wait time we have observed between a Service loop starting and us actually getting the mutex
        let mut max_work = Duration::MIN; // the highest time we have observed the work of the service loop taking

        let mut diagnostics: ServiceDiagnostics<N>;

        loop {
            let loop_started_timestamp = Instant::now(); // timestamp at the start of loop
            let period = previous_loop_timestamp.map(|prev| loop_started_timestamp.saturating_duration_since(prev));
            previous_loop_timestamp = Some(loop_started_timestamp);
            if let Some(period) = period {
                max_period = max_period.max(period);
            }

            {
                let mut api = self.api.lock().await;

                let locked_at_timestamp = Instant::now(); // timestamp at which we locked the mutex
                let lock_wait = locked_at_timestamp.saturating_duration_since(loop_started_timestamp);
                max_lock_wait = max_lock_wait.max(lock_wait);

                // AREA WHERE WE DO THE ACTUAL WORK OF THE SERVICE LOOP

                // this should run first so the sleep detection reads count towards the accumulator update break detection
                match self.handle_sleep_detection(&mut api).await {
                    Ok(()) => {},
                    Err(err) => {
                        sleep_detection_spi_error_count += 1;
                        #[cfg(feature = "defmt")]
                        // we need to use `Debug2Format` because `Error<SPI::Error>` only implements `Format` when the
                        // SPI error type does, and we can't gaurauntee that the SPI error type will. `Debug` is guaranteed tho
                        // since `embedded_hal::spi::Error` requires it.
                        defmt::error!("ADBMS6830B: Service: `handle_sleep_detection()` failed with error: {}", defmt::Debug2Format(&err));
                    }
                }

                let chips = *api.chips();

                let (update_result, accumulator_diagnostics) = accumulator.update(&chips);
                match update_result {
                    UpdateResult::BreakDetected { break_chip_index } => {
                        let applied = match self.handle_break_detected(&mut api, break_chip_index).await {
                            Ok(()) => true,
                            Err(err) => {
                                break_detection_spi_error_count += 1;
                                #[cfg(feature = "defmt")]
                                // we need to use `Debug2Format` because `Error<SPI::Error>` only implements `Format` when the
                                // SPI error type does, and we can't gaurauntee that the SPI error type will. `Debug` is guaranteed tho
                                // since `embedded_hal::spi::Error` requires it.
                                defmt::error!("ADBMS6830B: Service: `handle_break_detection()` failed with error: {}", defmt::Debug2Format(&err));
                                false
                            }
                        };
                        // report back to the accumulator if the split was successful or not so it know if it needs to keep trying or can move on
                        accumulator.was_split_applied(applied);
                    },
                    UpdateResult::Okay => {},
                }

                // END AREA WHERE WE DO THE ACTUAL WORK OF THE SERVICE LOOP

                let work = Instant::now().saturating_duration_since(locked_at_timestamp);
                max_work = max_work.max(work);

                // this is counted before we update the diagnostics so the diagnostics include the cycle being reported!!
                cycles_count += 1;
                
                diagnostics = ServiceDiagnostics {
                    accumulator_diagnostics: accumulator_diagnostics,
                    timing_diagnostics: TimingDiagnostics {
                        period, max_period, work, max_work, lock_wait, max_lock_wait, service_frequency: self.config.service_frequency_ms,
                    },
                    chip_state_diagnostics: ChipStateDiagnostics {
                        // this calls `*api.chips()` again instead of just using the already-read `chips`
                        // to make sure the diagnostics gets the absolute final ChipState at the end of the
                        // Service cycle
                        chip_state: *api.chips(),
                        chip_line: core::array::from_fn(|i| api.line_of(i)),
                    },
                    split: api.split(),
                    sleep_detection_spi_error_count,
                    break_detection_spi_error_count,
                    cycles_count,
                    segment_isospi_max_split_attempts: self.config.segment_isospi_max_split_attempts,
                    segment_isospi_max_failed_verification_attempts: self.config.segment_isospi_max_failed_verification_attempts,
                };

                // update service diagnostics
                self.diagnostics.lock(|cell| cell.set(Some(diagnostics)));
            }

            // the mutex gaurd is dropped by this point! so we can call the closure
            on_diagnostics(&diagnostics).await;

            Timer::after(Duration::from_millis(self.config.service_frequency_ms)).await
        }
    }

    /// Reads the Service's current `ServiceDiagnostics`. This provides various
    /// diagnostic and debugging information about the current state of the Service.
    /// 
    /// This will return `None` if the service has not run yet.
    /// 
    /// Note: The Service's `ServiceDiagnostics` gets updated each time the service runs (see `SERVICE_FREQUENCY_MS`).
    /// If you want instantaneous chip data (i.e., data that is fetched exactly as you call the function), the `.chips()` function may be of interest.
    pub fn diagnostics(&self) -> Option<ServiceDiagnostics<N>> {
        self.diagnostics.lock(|cell| cell.get())
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
    pub async fn read<G: ReadableGroup>(&self) -> Responses<G, SPI::Error, N> {
        let mut api = self.api.lock().await;
        api.read().await
    }

    /// Writes one register group per chip. `groups` is indexed in logical chip order.
    pub async fn write<G: WritableGroup>(&self, groups: &[G; N]) -> Result<(), Error<SPI::Error>> {
        let mut api = self.api.lock().await;
        api.write(groups).await
    }

    /// Sends a command to every chip on both lines.
    pub async fn command(&self, command: commands::Command) -> Result<(), Error<SPI::Error>> {
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
    pub async fn adax_autoconvert(&self, open_wire: commands::adc::OpenWireAux, pull: commands::adc::Pull, channel: commands::adc::Aux1InputSelection, timeout_ms: u64, ) -> Result<(), Error<SPI::Error>> {
        let mut api = self.api.lock().await;
        api.adax_autoconvert(open_wire, pull, channel, timeout_ms).await
    }

    /// Starts an AUX2 conversion (ADAX2) and waits for it to finish.
    pub async fn adax2_autoconvert(&self, channel: commands::adc::Aux2InputSelection, timeout_ms: u64) -> Result<(), Error<SPI::Error>> {
        let mut api = self.api.lock().await;
        api.adax2_autoconvert(channel, timeout_ms).await
    }

    /// Per-chip metadata.
    pub async fn chips(&self) -> [ChipState; N] {
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
    async fn handle_break_detected(&self, api: &mut Api<SPI, N>, break_chip_index: usize) -> Result<(), Error<SPI::Error>> {
        match api.split_at(OnLineA(break_chip_index)).await {
            Ok(()) => {
                #[cfg(feature = "defmt")] {
                    defmt::info!(
                        "ADBMS6830B: Service: isoSPI break at chip {}. Chips {}..{} moved to line B.",
                        break_chip_index, break_chip_index, N
                    );
                }
                Ok(())
            },
            Err(err) => {
                #[cfg(feature = "defmt")] {
                    defmt::error!(
                        "ADBMS6830B: Service: failed to split the chain at chip {}: {}",
                        break_chip_index, defmt::Debug2Format(&err)
                    );
                }
                Err(err)
            }
        }
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

        // Adopt what every reachable chip just reported. Chips that actually slept get a full
        // reset below instead, which supersedes this!!
        api.resync_command_counts(&statuses);

        for (chip, response) in statuses.iter().enumerate() {
            let Some(response) = response else { continue };
            if response.pec().is_failed() { continue; }

            if response.data().sleep() == SleepModeDetection::SleepModeDetected {
                any_slept = true;
                api.chips[chip].reset_command_count(response.command_counter());
                // Waking into standby also sets both rail UV flags so need to clear them alongside SLEEP or else it will look like undervoltage happened
                clears[chip] = clears[chip]
                    .with_cl_sleep(ClearAction::Clear)
                    .with_cl_vauv(ClearAction::Clear)
                    .with_cl_vduv(ClearAction::Clear);
            }
        }

        // Sleep resets every register to its default. so we need to put back what the application last asked for.
        if any_slept {
            let config_a: [ConfigA; N] = core::array::from_fn(|i| api.config_a[i].unwrap_or(ConfigA::new()));
            api.write(&config_a).await?;

            let config_b: [ConfigB; N] = core::array::from_fn(|i| api.config_b[i].unwrap_or(ConfigB::new()));
            api.write(&config_b).await?;

            // write the clears
            api.write(&clears).await
        } else {
            Ok(())
        }
    }
}
