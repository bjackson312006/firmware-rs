//! Service for ADBMS6830B

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