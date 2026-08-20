//! Service for ADBMS6830B

use embassy_time::{Timer, Duration, Instant};
use embedded_hal_async::spi::SpiDevice;
use crate::{
    chip::{
        commands, registers::{ReadableGroup, WritableGroup, config_a::ConfigA, config_b::ConfigB},
    }, line::{
        Error, Line, conversion_times
    }, manager::{
        Api, ChipState, LineId, OnLineA, Responses,
    }, service::diagnostics::{ChipStateDiagnostics, TimingDiagnostics}
};
use embassy_sync::mutex::Mutex;
use embassy_sync::blocking_mutex::raw::RawMutex;
use core::cell::Cell;

/// Structs for the diagnostics collected and reported by the Service.
pub mod diagnostics {
    use super::accumulator;
    use super::SpiDevice;
    use super::Error;
    use super::Duration;
    use super::Instant;
    use super::OnLineA;
    use super::ChipState;
    use super::LineId;

    pub use accumulator::State;

    /// Diagnostics from the Service's PEC error accumulator.
    #[derive(Copy, Clone)]
    pub struct AccumulatorDiagnostics<const N: usize> {
        /// Current state of the accumulator window.
        pub(crate) state: accumulator::State,
        /// Total PEC failures counted for each chip during this window.
        pub(crate) failed: [usize; N],
        /// Total read attempts for this chip during this window.
        /// 
        /// This is just a sum of the number of failed PECs and number of successful PECs for this chip
        /// during this window.
        pub(crate) attempts: [usize; N],
        /// Each chip's PEC failure rate over the current window as a percentage (0 - 100).
        pub(crate) failure_pct: [u8; N],
        /// Percentage of reads that must fail their PEC for a chip to be considered as "failing".
        /// 
        /// Note: This is a constant value! It is literally just an echo of
        /// `segment_isospi_pec_failure_ratio_pct` from the config. It's included in these diagnostics
        /// just for convenience, and so we can double-check this configured constant
        /// is in fact what we expect it to be.
        pub(crate) failure_pct_threshold: u8,
        /// How long the accumulator evaluation window lasts, in ms. Basically, after a window
        /// opens, this is how long the window stays open to gather PEC data.
        /// 
        /// Note: This is a constant value! It is literally just an echo of
        /// `segment_isospi_eval_period_ms` from the config. It's included in these diagnostics
        /// just for convenience, and so we can double-check this configured constant
        /// is in fact what we expect it to be.
        pub(crate) accumulator_window_period: u64,
        /// Fewest reads a chip must have taken part in before its failure rate is actually considered as meaning anything.
        /// 
        /// This is used to protect against a tiny sample size in the accumulator incorrectly flagging a break. Basically,
        /// if an accumulation window is less than this, there is not enough data to conclude that a PCT of failed PECs actually
        /// indicates a break. So, if a window has less than this, we ignore that window.
        /// 
        /// Note: This is a constant value! It is literally just an echo of
        /// `segment_isospi_min_attempts_for_fail` from the config. It's included in these diagnostics
        /// just for convenience, and so we can double-check this configured constant
        /// is in fact what we expect it to be.
        pub(crate) min_attempts_for_fail: usize,

        /// This counts the number of times a chip has passed over opening a window because
        /// it hadn't taken part in `SEGMENT_ISOSPI_MIN_ATTEMPTS_TO_OPEN_WINDOW` reads since the last time the Service has run.
        /// 
        /// When this increments, it means the number of reads within a single Service cycle is below `SEGMENT_ISOSPI_MIN_ATTEMPTS_TO_OPEN_WINDOW`. At
        /// such a frequency, the Service does not ever open any accumulator windows, because there is not enough PEC error data
        /// to reliably determine whether or not to open a window. Basically, the Service's isoSPI error detection does not run in that cycle.
        /// 
        /// This DOES NOT necessarily indicate an error. In periods of low traffic (where the application is not making any
        /// reads, meaning this Service is effectively the only thing making reads at all), this counter will increment. This is expected,
        /// as in periods where very few reads are being made, there is not enough data to base the PEC error accumulator off of. However, if this
        /// counter is incrementing in normal-to-high traffic periods (i.e., the application is making a lot of reads), there is probably an issue
        /// worth looking into.
        pub(crate) below_min_attempts_to_open_window_count: usize,

        /// This counts the number of times a chip could not be judged failed or not because it
        /// hadn't taken part in `SEGMENT_ISOSPI_MIN_ATTEMPTS_FOR_FAIL` reads over the whole window.
        /// 
        /// This is only checked when a window expires (i.e., at most once per `SEGMENT_ISOSPI_EVAL_PERIOD_MS`) instead of
        /// once per Service cycle. When it increments, a window did open (so something looked weird enough to open one up)
        /// but ran its full length without gathering enough PEC data to reach a verdict. This basically makes it so the
        /// window gets thrown away at the end.
        /// 
        /// Like `below_min_attempts_to_open_window_count`/`below_min_attempts_to_open_window_count()`, this consistently incrementing
        /// means that isoSPI break detection is basically not running since there are not enough reads being made to gather
        /// the necessary PEC data to detect anything.
        /// 
        /// Also like `below_min_attempts_to_open_window_count`/`below_min_attempts_to_open_window_count()`,
        /// this doesn't necessarily indicate an error. However, if this counter is incrementing when you are in a normal-to-high traffic
        /// period (or at least expect to be), then it is probably good to investigate. 
        /// 
        /// (Plus, remember that this can only increment when a window has been opened, meaning the Service is getting past the 
        /// check reported by `below_min_attempts_to_open_window_count`/`below_min_attempts_to_open_window_count()`. So if this is incrementing,
        /// there is a chance the configured threshold for opening a window may not be optimized that well. Again, it is up to the application
        /// to decide that though, or if they even care about this.)
        pub(crate) below_min_attempts_for_fail_count: usize,

        /// Fewest reads in a single update before that update's failure rate can open a window.
        /// 
        /// Note: This is a constant value! It is literally just an echo of
        /// `segment_isospi_min_attempts_to_open_window` from the config. It's included in these diagnostics
        /// just for convenience, and so we can double-check this configured constant
        /// is in fact what we expect it to be.
        pub(crate) min_attempts_to_open_window: usize,
    }
    impl<const N: usize> AccumulatorDiagnostics<N> {
        /// Current state of the accumulator window.
        pub const fn state(&self) -> accumulator::State { self.state }
        /// Total PEC failures counted for each chip during this window.
        pub const fn failed(&self) -> [usize; N] { self.failed }
        /// Total read attempts for this chip during this window.
        /// 
        /// This is just a sum of the number of failed PECs and number of successful PECs for this chip
        /// during this window.
        pub const fn attempts(&self) -> [usize; N] { self.attempts }
        /// Each chip's PEC failure rate over the current window as a percentage (0 - 100).
        pub const fn failure_pct(&self) -> [u8; N] { self.failure_pct }
        /// Percentage of reads that must fail their PEC for a chip to be considered as "failing".
        /// 
        /// Note: This is a constant value! It is literally just an echo of
        /// `segment_isospi_pec_failure_ratio_pct` from the config. It's included in these diagnostics
        /// just for convenience, and so we can double-check this configured constant
        /// is in fact what we expect it to be.
        pub const fn failure_pct_threshold(&self) -> u8 { self.failure_pct_threshold }
        /// How long the accumulator evaluation window lasts, in ms. Basically, after a window
        /// opens, this is how long the window stays open to gather PEC data.
        /// 
        /// Note: This is a constant value! It is literally just an echo of
        /// `segment_isospi_eval_period_ms` from the config. It's included in these diagnostics
        /// just for convenience, and so we can double-check this configured constant
        /// is in fact what we expect it to be.
        pub const fn accumulator_window_period(&self) -> u64 { self.accumulator_window_period }
        /// Fewest reads a chip must have taken part in before its failure rate is actually considered as meaning anything.
        /// 
        /// This is used to protect against a tiny sample size in the accumulator incorrectly flagging a break. Basically,
        /// if an accumulation window is less than this, there is not enough data to conclude that a PCT of failed PECs actually
        /// indicates a break. So, if a window has less than this, we ignore that window.
        /// 
        /// Note: This is a constant value! It is literally just an echo of
        /// `segment_isospi_min_attempts_for_fail` from the config. It's included in these diagnostics
        /// just for convenience, and so we can double-check this configured constant
        /// is in fact what we expect it to be.
        pub const fn min_attempts_for_fail(&self) -> usize { self.min_attempts_for_fail }

        /// This counts the number of times a chip has passed over opening a window because
        /// it hadn't taken part in `SEGMENT_ISOSPI_MIN_ATTEMPTS_TO_OPEN_WINDOW` reads since the last time the Service has run.
        /// 
        /// When this increments, it means the number of reads within a single Service cycle is below `SEGMENT_ISOSPI_MIN_ATTEMPTS_TO_OPEN_WINDOW`. At
        /// such a frequency, the Service does not ever open any accumulator windows, because there is not enough PEC error data
        /// to reliably determine whether or not to open a window. Basically, the Service's isoSPI error detection does not run in that cycle.
        /// 
        /// This DOES NOT necessarily indicate an error. In periods of low traffic (where the application is not making any
        /// reads, meaning this Service is effectively the only thing making reads at all), this counter will increment. This is expected,
        /// as in periods where very few reads are being made, there is not enough data to base the PEC error accumulator off of. However, if this
        /// counter is incrementing in normal-to-high traffic periods (i.e., the application is making a lot of reads), there is probably an issue
        /// worth looking into.
        pub const fn below_min_attempts_to_open_window_count(&self) -> usize { self.below_min_attempts_to_open_window_count }

        /// This counts the number of times a chip could not be judged failed or not because it
        /// hadn't taken part in `SEGMENT_ISOSPI_MIN_ATTEMPTS_FOR_FAIL` reads over the whole window.
        /// 
        /// This is only checked when a window expires (i.e., at most once per `SEGMENT_ISOSPI_EVAL_PERIOD_MS`) instead of
        /// once per Service cycle. When it increments, a window did open (so something looked weird enough to open one up)
        /// but ran its full length without gathering enough PEC data to reach a verdict. This basically makes it so the
        /// window gets thrown away at the end.
        /// 
        /// Like `below_min_attempts_to_open_window_count`/`below_min_attempts_to_open_window_count()`, this consistently incrementing
        /// means that isoSPI break detection is basically not running since there are not enough reads being made to gather
        /// the necessary PEC data to detect anything.
        /// 
        /// Also like `below_min_attempts_to_open_window_count`/`below_min_attempts_to_open_window_count()`,
        /// this doesn't necessarily indicate an error. However, if this counter is incrementing when you are in a normal-to-high traffic
        /// period (or at least expect to be), then it is probably good to investigate. 
        /// 
        /// (Plus, remember that this can only increment when a window has been opened, meaning the Service is getting past the 
        /// check reported by `below_min_attempts_to_open_window_count`/`below_min_attempts_to_open_window_count()`. So if this is incrementing,
        /// there is a chance the configured threshold for opening a window may not be optimized that well. Again, it is up to the application
        /// to decide that though, or if they even care about this.)
        pub const fn below_min_attempts_for_fail_count(&self) -> usize { self.below_min_attempts_for_fail_count }

        /// Fewest reads in a single update before that update's failure rate can open a window.
        /// 
        /// Note: This is a constant value! It is literally just an echo of
        /// `segment_isospi_min_attempts_to_open_window` from the config. It's included in these diagnostics
        /// just for convenience, and so we can double-check this configured constant
        /// is in fact what we expect it to be.
        pub const fn min_attempts_to_open_window(&self) -> usize { self.min_attempts_to_open_window }
    }

    /// Diagnostics for the frequency at which the Service is running.
    #[derive(Copy, Clone)]
    #[cfg_attr(feature = "defmt", derive(defmt::Format))]
    pub struct TimingDiagnostics {
        /// The difference in time between the most recent Service cycle, and the Service cycle before that.
        /// 
        /// This should generally be around the configured `SERVICE_FREQUENCY_MS`, but will be slightly longer
        /// due to the time spend awaiting (either during SPI transiactions or waiting for the mutex).
        /// 
        /// If only zero or one Service cycles have ran yet, this will be None.
        pub(crate) period: Option<Duration>,
        /// The maximum `period` the Service has observed while running.
        /// 
        /// This starts out as zero until the Service has ran a few times.
        pub(crate) max_period: Duration,
        /// How long the "work" of the Service took during the most recent Service cycle.
        /// "work" is defined as everything the Service actually does after getting the mutex (i.e., sleep detection, isoSPI break detection, etc)
        pub(crate) work: Duration,
        /// The maximum `work` the Service has observed while running.
        /// 
        /// This starts out as zero until the Service has ran a few times.
        pub(crate) max_work: Duration,
        /// How long the Service waited to acquire the mutex during the most recent cycle.
        pub(crate) lock_wait: Duration,
        /// The maximum `lock_wait` the Service has observed while running.
        /// 
        /// This starts out as zero until the Service has ran a few times.
        pub(crate) max_lock_wait: Duration,
        /// The configured service frequency. This represents how long the Service waits after a cycleto wake up an run again.
        /// 
        /// This is a const value! It literally is just an echo of `service_frequency_ms` from the config. This
        /// is reported as a diagnostic for convinience, so it can be compared with the actual period reported by this struct.
        pub(crate) service_frequency: u64,
    }
    impl TimingDiagnostics {
        /// The difference in time between the most recent Service cycle, and the Service cycle before that.
        /// 
        /// This should generally be around the configured `SERVICE_FREQUENCY_MS`, but will be slightly longer
        /// due to the time spend awaiting (either during SPI transiactions or waiting for the mutex).
        /// 
        /// If only zero or one Service cycles have ran yet, this will be None.
        pub const fn period(&self) -> Option<Duration> { self.period }
        /// The maximum `period` the Service has observed while running.
        /// 
        /// This starts out as zero until the Service has ran a few times.
        pub const fn max_period(&self) -> Duration { self.max_period }
        /// How long the "work" of the Service took during the most recent Service cycle.
        /// "work" is defined as everything the Service actually does after getting the mutex (i.e., sleep detection, isoSPI break detection, etc)
        pub const fn work(&self) -> Duration { self.work }
        /// The maximum `work` the Service has observed while running.
        /// 
        /// This starts out as zero until the Service has ran a few times.
        pub const fn max_work(&self) -> Duration { self.max_work }
        /// How long the Service waited to acquire the mutex during the most recent cycle.
        pub const fn lock_wait(&self) -> Duration { self.lock_wait }
        /// The maximum `lock_wait` the Service has observed while running.
        /// 
        /// This starts out as zero until the Service has ran a few times.
        pub const fn max_lock_wait(&self) -> Duration { self.max_lock_wait }
        /// The configured service frequency. This represents how long the Service waits after a cycleto wake up an run again.
        /// 
        /// This is a const value! It literally is just an echo of `service_frequency_ms` from the config. This
        /// is reported as a diagnostic for convinience, so it can be compared with the actual period reported by this struct.
        pub const fn service_frequency(&self) -> u64 { self.service_frequency }
    }

    /// Snapshot of per-chip diagnostics at the time the Service ran. This can help provide insight into
    /// the exact state of things at the instant the service ran.
    /// 
    /// This is generally just a snapshot of the `ChipState` properties for each chip, but with
    /// a few omitted values that aren't really relavent as diagnostics (like the cached configs),
    /// and a few extras added in due to the extra context Service provides.
    /// 
    /// For an instantaneous direct read of `ChipState`, `Service` provides the `.chips()` method.
    #[derive(Copy, Clone)]
    pub struct ChipStateDiagnostics<const N: usize> {
        /// Chip state.
        pub(crate) chip_state: [ChipState; N],
        /// Which line each chip is currently on.
        pub(crate) chip_line: [LineId; N],
    }
    impl<const N: usize> ChipStateDiagnostics<N> {
        /// Chip state.
        pub const fn chip_state(&self) -> [ChipState; N] { self.chip_state }
        /// Which line each chip is currently on.
        pub const fn chip_line(&self) -> [LineId; N] { self.chip_line }
    }

    /// Diagnostics for a Service.
    #[derive(Copy, Clone)]
    pub struct ServiceDiagnostics<const N: usize> {
        /// PEC error accumulator diagnostics.
        pub(crate) accumulator_diagnostics: AccumulatorDiagnostics<N>,
        /// Diagnostics for the frequency at which the Service is running.
        pub(crate) timing_diagnostics: TimingDiagnostics,
        /// Chip state diagnostics.
        pub(crate) chip_state_diagnostics: ChipStateDiagnostics<N>,
        /// Number of times sleep detection has failed due to a SPI communication error.
        pub(crate) sleep_detection_spi_error_count: usize,
        /// Number of times the service has ran so far. This increments on every loop the service makes.
        pub(crate) cycles_count: usize,
        /// The current split of chips between the isoSPI lines.
        /// 
        /// This is reported here as the raw OnLineA value. However, the service also
        /// derives the per-chip Line values as part of the per chip diagnostics in case that's easier to read.
        pub(crate) split: OnLineA,
    }
    impl<const N: usize> ServiceDiagnostics<N> {
        /// Diagnostics from the Service's PEC error accumulator.
        pub const fn accumulator(&self) -> AccumulatorDiagnostics<N> { self.accumulator_diagnostics }
        /// Diagnostics for the frequency at which the Service is running.
        pub const fn timing(&self) -> TimingDiagnostics { self.timing_diagnostics }
        /// Chip state diagnostics.
        pub const fn chip_state_diagnostics(&self) -> ChipStateDiagnostics<N> { self.chip_state_diagnostics }
        /// Number of times sleep detection has failed due to a SPI communication error.
        pub const fn sleep_detection_spi_error_count(&self) -> usize { self.sleep_detection_spi_error_count }
        /// Number of times the service has ran so far. This increments on every loop the service makes.
        pub const fn cycles_count(&self) -> usize { self.cycles_count }
        /// The current split of chips between the isoSPI lines.
        /// 
        /// This is reported here as the raw OnLineA value. However, the service also
        /// derives the per-chip Line values as part of the per chip diagnostics in case that's easier to read.
        pub const fn split(&self) -> OnLineA { self.split }
    }
}

/// Configuration parameters for the Service. This also includes constant defaults.
pub mod config {
    /// How often the service should run, in ms.
    pub const SERVICE_FREQUENCY_MS: u64 = 300;
    /// How long each evaluation window lasts.
    pub const SEGMENT_ISOSPI_EVAL_PERIOD_MS: u64 = 4000;
    /// Fewest reads a chip must have taken part in before its failure rate is actually considered as meaning anything.
    ///
    /// this is here to protect against a tiny sample size in the accumulator incorrectly flagging a break. Basically,
    /// if an accumulation window is less than this, there is not enough data to conclude that a PCT of failed PECs actually
    /// indicates a break. So, if a window has less than this, we ignore that window.
    pub const SEGMENT_ISOSPI_MIN_ATTEMPTS_FOR_FAIL: usize = 16;
    /// Percentage of reads that must fail their PEC for a chip to look unreachable.
    ///
    /// A break will cause the affected chips' reads to fail essentially every
    /// time. So, this value is meant to be quite high. 
    /// This sits well above any plausible noise level but leaves margin for a link
    /// that is failing intermittently rather than completely.
    pub const SEGMENT_ISOSPI_PEC_FAILURE_RATIO_PCT: u8 = 75;
    /// Fewest reads in a single update before that update's failure rate can open a window.
    ///
    /// This is kinda meant to take the place of `SEGMENT_ISOSPI_PEC_ACCUM_START_THRESH` from the C code. It serves
    /// a similar-ish function (in that it is a blocker for an accumulator window being allowed to start), but it uses sample
    /// size rather than absolute error count.
    pub const SEGMENT_ISOSPI_MIN_ATTEMPTS_TO_OPEN_WINDOW: usize = 2;

    /// Configuration constants for a Service. Probably just use `ServiceConfig::default()`
    #[derive(Clone, Copy)]
    pub struct ServiceConfig {
        /// How often the service should run, in ms.
        pub service_frequency_ms: u64,
        /// PEC accumulator setting! How long each evaluation window lasts.
        pub segment_isospi_eval_period_ms: u64,
        /// PEC accumulator setting! Fewest reads a chip must have taken part in before its failure rate is actually considered as meaning anything.
        ///
        /// this is here to protect against a tiny sample size in the accumulator incorrectly flagging a break. Basically,
        /// if an accumulation window is less than this, there is not enough data to conclude that a PCT of failed PECs actually
        /// indicates a break. So, if a window has less than this, we ignore that window.
        pub segment_isospi_min_attempts_for_fail: usize,
        /// PEC accumulator setting! Percentage of reads that must fail their PEC for a chip to look unreachable.
        ///
        /// A break will cause the affected chips' reads to fail essentially every
        /// time. So, this value is meant to be quite high. 
        /// This sits well above any plausible noise level but leaves margin for a link
        /// that is failing intermittently rather than completely.
        pub segment_isospi_pec_failure_ratio_pct: u8,
        /// PEC accumulator setting! Fewest reads in a single update before that update's failure rate can open a window.
        ///
        /// This is kinda meant to take the place of `SEGMENT_ISOSPI_PEC_ACCUM_START_THRESH` from the C code. It serves
        /// a similar-ish function (in that it is a blocker for an accumulator window being allowed to start), but it uses sample
        /// size rather than absolute error count.
        pub segment_isospi_min_attempts_to_open_window: usize,
    }
    impl Default for ServiceConfig {
        fn default() -> Self {
            Self {
                service_frequency_ms: SERVICE_FREQUENCY_MS,
                segment_isospi_eval_period_ms: SEGMENT_ISOSPI_EVAL_PERIOD_MS,
                segment_isospi_min_attempts_for_fail: SEGMENT_ISOSPI_MIN_ATTEMPTS_FOR_FAIL,
                segment_isospi_pec_failure_ratio_pct: SEGMENT_ISOSPI_PEC_FAILURE_RATIO_PCT,
                segment_isospi_min_attempts_to_open_window: SEGMENT_ISOSPI_MIN_ATTEMPTS_TO_OPEN_WINDOW,
            }
        }
    }
}

/// Tracks each chip's PEC failure rate and reports where a break has opened.
pub(crate) mod accumulator {
    use embassy_time::{Duration, Instant};
    use crate::service::{config, diagnostics::AccumulatorDiagnostics};
    use super::ChipState;

    #[derive(Copy, Clone, Debug)]
    pub enum State {
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

    /// Helper struct for Service that tracks/manages the PEC error accumulator state.
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

        /// Diagnostic counter. 
        /// This counts the number of times a chip has passed over opening a window because
        /// it hadn't taken part in `SEGMENT_ISOSPI_MIN_ATTEMPTS_TO_OPEN_WINDOW` reads since the last time the Service has run.
        /// 
        /// When this increments, it means the number of reads within a single Service cycle is below `SEGMENT_ISOSPI_MIN_ATTEMPTS_TO_OPEN_WINDOW`. At
        /// such a frequency, the Service does not ever open any accumulator windows, because there is not enough PEC error data
        /// to reliably determine whether or not to open a window. Basically, the Service's isoSPI error detection does not run in that cycle.
        /// 
        /// This DOES NOT necessarily indicate an error. In periods of low traffic (where the application is not making any
        /// reads, meaning this Service is effectively the only thing making reads at all), this counter will increment. This is expected,
        /// as in periods where very few reads are being made, there is not enough data to base the PEC error accumulator off of. However, if this
        /// counter is incrementing in normal-to-high traffic periods (i.e., the application is making a lot of reads), there is probably an issue
        /// worth looking into.
        below_min_attempts_to_open_window_count: usize,

        /// Diagnostic counter.
        /// This counts the number of times a chip could not be judged failed or not because it
        /// hadn't taken part in `SEGMENT_ISOSPI_MIN_ATTEMPTS_FOR_FAIL` reads over the whole window.
        /// 
        /// This is only checked when a window expires (i.e., at most once per `SEGMENT_ISOSPI_EVAL_PERIOD_MS`) instead of
        /// once per Service cycle. When it increments, a window did open (so something looked weird enough to open one up)
        /// but ran its full length without gathering enough PEC data to reach a verdict. This basically makes it so the
        /// window gets thrown away at the end.
        /// 
        /// Like `below_min_attempts_to_open_window_count`/`below_min_attempts_to_open_window_count()`, this consistently incrementing
        /// means that isoSPI break detection is basically not running since there are not enough reads being made to gather
        /// the necessary PEC data to detect anything.
        /// 
        /// Also like `below_min_attempts_to_open_window_count`/`below_min_attempts_to_open_window_count()`,
        /// this doesn't necessarily indicate an error. However, if this counter is incrementing when you are in a normal-to-high traffic
        /// period (or at least expect to be), then it is probably good to investigate. 
        /// 
        /// (Plus, remember that this can only increment when a window has been opened, meaning the Service is getting past the 
        /// check reported by `below_min_attempts_to_open_window_count`/`below_min_attempts_to_open_window_count()`. So if this is incrementing,
        /// there is a chance the configured threshold for opening a window may not be optimized that well. Again, it is up to the application
        /// to decide that though, or if they even care about this.)
        below_min_attempts_for_fail_count: usize,

        /// Whether `last_failed` and `last_attempts` hold a real reading yet.
        /// 
        /// This is used to make sure the first reading is saved as a baseline rather than
        /// trying to calculate a change from it.
        seeded: bool,

        /// Service config for the service.
        config: config::ServiceConfig,
    }
    impl<const N: usize> Accumulator<N> {
        /// Default initialization for the accumulator. Will be idle by default.
        pub(crate) const fn new(config: config::ServiceConfig) -> Self {
            Self {
                state: State::Idle,
                failed: [0; N],
                attempts: [0; N],
                last_failed: [0; N],
                last_attempts: [0; N],
                below_min_attempts_to_open_window_count: 0,
                below_min_attempts_for_fail_count: 0,
                seeded: false,
                config,
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

        /// PRIVATE! This chip's PEC failure rate over the current window, as a percentage.
        /// 
        /// Note: This doesn't check `SEGMENT_ISOSPI_ARM_MIN_ATTEMPTS` or `SEGMENT_ISOSPI_MIN_ATTEMPTS`. The caller
        /// is supposed to do that themselves.
        fn failure_pct(&self, chip: usize) -> u8 {
            // if no reads have happened yet we technically have a failure pct of 0%
            if self.attempts[chip] == 0 {
                return 0;
            }

            ((self.failed[chip] * 100) / self.attempts[chip]) as u8
        }

        /// PRIVATE! Opens an accumulation window.
        /// 
        /// this purposefully keeps whatever is already tallied so the update that armed the
        /// window counts towards it.
        fn open_window(&mut self) {
            self.state = State::Accumulating {
                until: Instant::now() + Duration::from_millis(self.config.segment_isospi_eval_period_ms),
            };
        }

        /// PRIVATE! Helper for `update()`. Checks whether or not `chip`'s PEC data warrants opening a window
        fn should_we_open_window_for_chip(&mut self, failure_pct: &[u8; N], chip: usize) -> bool {
            // first check if we have crossed the minimum amount of PEC attempts to even consider opening a window
            if self.attempts[chip] < self.config.segment_isospi_min_attempts_to_open_window {
                // increment diagnostic for how many times we have not opened a window just due to the min attempts
                self.below_min_attempts_to_open_window_count += 1;
                return false;
            }
            // okay at this point we know we have enough PEC attempt data to actually do the check reliably. So:
            // if this is true we should open a window (since we have exceeded the threshold for failing chips)
            failure_pct[chip] >= self.config.segment_isospi_pec_failure_ratio_pct
        }

        /// PRIVATE! Helper for `update()`. Checks whether or not `chip`'s PEC data passes our failure criteria
        fn is_chip_failed(&mut self, failure_pct: &[u8; N], chip: usize) -> bool {
            // first check if we have crossed the minimum amount of PEC attempts to even consider the chip as failed
            // if this is too low we don't have enough PEC data to reliably conclude that a chip has failed
            if self.attempts[chip] < self.config.segment_isospi_min_attempts_for_fail {
                // increment diagnostic for how many times a chip couldn't be judged just due to the min attempts
                self.below_min_attempts_for_fail_count += 1;
                return false;
            }
            // okay at this point we know we have enough PEC attempt data to actually do the check reliably. So:
            // if this is true we should flag this chip as failed (since we have exceeded the threshold for failing chips)
            failure_pct[chip] >= self.config.segment_isospi_pec_failure_ratio_pct
        }

        /// Updates the PEC error accumulator state, and detects if a break should be set.
        /// 
        /// This should be called in every iteration of the Service runner. This will return either `UpdateResult::Okay`, meaning that
        /// the service runner doesn't need to do anything regarding a break right now, or `UpdateResult::BreakDetected`, after which the
        /// service runner must handle the break accordingly.
        /// 
        /// This also returns a `AccumulatorDiagnostics` for the current update.
        pub(crate) fn update(&mut self, chips: &[ChipState; N]) -> (UpdateResult, AccumulatorDiagnostics<N>) {
            // update the accumulator's tracking of all the chip metadata!
            // need to do this at the beginning of update always
            self.update_chips(chips);

            // calculate failure_pct for all chips
            // this is used both for diagnostics and also how to progress
            // accumulator state
            let mut failure_pct: [u8; N] = [0; N];
            for chip in 0..N {
                failure_pct[chip] = self.failure_pct(chip)
            }

            // actually do the update/state handling stuff
            let update_result = 'update_result: {
                match self.state {
                    // case: no window is open, so the tallies hold only this update's reads (they get
                    // cleared every update we stay here). A chip failing its reads right now is what
                    // opens a window, so the window lines up with the start of the problem.
                    State::Idle => {
                        if (0..N).any(|chip| { self.should_we_open_window_for_chip(&failure_pct, chip) }) {
                            self.open_window();
                        } else {
                            self.reset_chips();
                        }
                    }

                    // case: the window is up, so see whether what it caught looks like a break.
                    State::Accumulating { until } if Instant::now() >= until => {
                        // find the first chip that exceeds our failure criteria
                        // A break takes out every chip past it, so the first unreachable chip only
                        // means a break if every chip after it is unreachable too.
                        let first = (0..N).find(|&chip| { self.is_chip_failed(&failure_pct, chip) });

                        if let Some(idx) = first {
                            // okay we found the `first` failing chip, so now we have to check if all the ones after `first`
                            // are also failing. if they are, this is a break
                            if (idx+1..N).all(|chip| self.is_chip_failed(&failure_pct, chip)) {
                                self.reset_chips();
                                self.state = State::Latched;
                                break 'update_result UpdateResult::BreakDetected { break_chip_index: idx };
                            }
                        }

                        // Whatever the window caught wasn't a break, so go back to watching for
                        // a fresh spike rather than measuring straight through.
                        self.reset_chips();
                        self.state = State::Idle;
                    }

                    // case: we are still in the middle of filling the window so don't need to do anything rn
                    State::Accumulating { .. } => {}

                    // case: the one break we can route around has already been handled. so, stop counting.
                    State::Latched => self.reset_chips(),
                }

                // if we get here we are okay
                UpdateResult::Okay
            };

            // create the diagnostics
            let diagnostics = AccumulatorDiagnostics {
                state: self.state,
                failed: self.failed,
                attempts: self.attempts,
                failure_pct: failure_pct,
                failure_pct_threshold: self.config.segment_isospi_pec_failure_ratio_pct,
                accumulator_window_period: self.config.segment_isospi_eval_period_ms,
                min_attempts_for_fail: self.config.segment_isospi_min_attempts_for_fail,
                below_min_attempts_to_open_window_count: self.below_min_attempts_to_open_window_count,
                below_min_attempts_for_fail_count: self.below_min_attempts_for_fail_count,
                min_attempts_to_open_window: self.config.segment_isospi_min_attempts_to_open_window,
            };

            (update_result, diagnostics)
        }
    }
}

use diagnostics::ServiceDiagnostics;

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
    pub async fn run(&self) -> ! {
        use crate::service::accumulator::{Accumulator, UpdateResult};

        // The runner is the only thing that uses this so it doesn't need to be part of `Service`.
        let mut accumulator = Accumulator::<N>::new(self.config);

        let mut sleep_detection_spi_error_count: usize = 0;
        let mut cycles_count: usize = 0;

        // stuff for calculating how often the service runs actually
        let mut previous_loop_timestamp: Option<Instant> = None; // timestamp the service loop last ran
        let mut max_period = Duration::MIN; // the highest period between two service loops we have observed so far
        let mut max_lock_wait = Duration::MIN; // the highest wait time we have observed between a Service loop starting and us actually getting the mutex
        let mut max_work = Duration::MIN; // the highest time we have observed the work of the service loop taking

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
                        defmt::error!("ADBMS6830B: Service: `handle_sleep_detection()` failed with error: {}", err);
                    }
                }

                let chips = *api.chips();

                let (update_result, accumulator_diagnostics) = accumulator.update(&chips);
                match update_result {
                    UpdateResult::BreakDetected { break_chip_index } => {
                        self.handle_break_detected(&mut api, break_chip_index).await;
                    },
                    UpdateResult::Okay => {}
                }

                // END AREA WHERE WE DO THE ACTUAL WORK OF THE SERVICE LOOP

                let work = Instant::now().saturating_duration_since(locked_at_timestamp);
                max_work = max_work.max(work);
                
                // update service diagnostics
                self.diagnostics.lock(|cell| cell.set(
                    Some(ServiceDiagnostics {
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
                        cycles_count,
                     })
                ));
            }

            cycles_count += 1;

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
                // Waking into standby also sets both rail UV flags so need to clear them alongside SLEEP or else it will look like undervoltage happened
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
