//! Register Layouts and Bit Descriptions for Configuration Register Group B. 
//! 
//! The "main" struct here (i.e., the struct representing the overall register group) is `ConfigB`.
//! `ConfigB` can be translated into full protocol frames (for sending) via `ConfigBReadRequest`, `ConfigBReadResponse`, and `ConfigBWriteFrame`.
//! 
//! For more info about these registers, see Table 103 on page 71 of the datasheet
//! and Table 56 on page 61 of the datasheet.

use bitfield_struct::{bitfield, bitenum};
use adbms6830b_macros::BitfieldEnumDefault;

use super::register_group;
use super::super::commands;

/// Field types relavent to Configuration Register B. See Table 103 on page 71 of the datasheet.
pub mod types {
    use super::{bitenum, bitfield, BitfieldEnumDefault};

    /// UV comparison voltage (VUV). 12-bit field. Default is 0x800.
    /// 
    /// Cell undervoltage threshold = VUV x 16 x 150uV + 1.5V
    #[bitfield(u16)]
    pub struct UvComparisonVoltage {
        /// Cell undervoltage threshold = VUV x 16 x 150uV + 1.5V. Corresponds to `VUV[11:0]`.
        #[bits(12, default = 0x800)]    pub value: u16,
        #[bits(4, default = 0)]         _reserved: u8,
    }
    impl UvComparisonVoltage {  pub const DEFAULT: Self = Self::new(); }

    /// OV comparison voltage (VOV). 12-bit field. Default is 0x7FF.
    /// 
    /// Cell overvoltage threshold = VOV × 16 × 150 μV + 1.5 V. 
    #[bitfield(u16)]
    pub struct OvComparisonVoltage {
        /// Cell overvoltage threshold = VOV × 16 × 150 μV + 1.5 V. Corresponds to `VOV[11:0]`.
        #[bits(12, default = 0x7FF)]     pub value: u16,
        #[bits(4, default = 0)]         _reserved: u8,
    }
    impl OvComparisonVoltage { pub const DEFAULT: Self = Self::new(); }

    /// Lets you enable/disable the discharge timer monitor (DTMEN). One-bit field.
    #[repr(u8)]
    #[bitenum]
    #[derive(BitfieldEnumDefault, Copy, Clone, Debug, PartialEq, Eq, Default)]
    pub enum DischargeTimerMonitor {
        /// Disables the discharge timer monitor function (default).
        #[default]
        #[fallback]
        Disabled = 0,
        /// Enables the discharge timer monitor function if the device transitions to the extended balancing state.
        Enabled = 1,
    }

    /// Lets you configure the discharge timer range (DTRNG). One-bit field.
    #[repr(u8)]
    #[bitenum]
    #[derive(BitfieldEnumDefault, Copy, Clone, Debug, PartialEq, Eq, Default)]
    pub enum DischargeTimerRange {
        /// Uses the short discharge timer range (0 to 63 minutes in 1 minute increments). (default)
        #[default]
        #[fallback]
        ShortRange = 0,
        /// Uses the long discharge timer range (0 to 16.8 hours in 16 minute increments).
        LongRange = 1,
    }

    /// Lets you configure the discharge timeout value, or read back the timer's status (DCTO). Six-bit field.
    /// 
    /// Note: This setting causes different behaviors depending on write/read.
    /// - Write = Set new value, 16-minute or 1-minute increments according to DTRNG read.
    /// - Read = remaining value, 16-minute or 1-minute increments according to DTRNG.
    /// 
    /// To convert between increments and minutes, you need to match on your current `DischargeTimerRange` setting:
    /// - If `DischargeTimerRange::ShortRange`, minutes = increments() * 1 (since 1 increment = 1 minute)
    /// - If `DischargeTimerRange::LongRange`, minutes = increments() * 16 (since 1 increment = 16 minutes)
    #[bitfield(u8)]
    pub struct DischargeTimerStatus {
        /// Number of increments remaining on the timer. This value can range between 0 and 63. Defaults to 0.
        /// 
        /// Note: The meaning of an increment depends on the `DischargeTimerRange`/DTRNG value set.
        #[bits(6, default = 0)]     pub increments: u8,
        #[bits(2, default = 0)]     _reserved: u8,
    }
    impl DischargeTimerStatus {
        pub const DEFAULT: Self = Self::new();

        /// Indicates whether or not we are timed out. In other words, this checks if our remaining increments == 0 or not.
        /// 
        /// - `false` means that there's one or more increments remaining.
        /// - `true` means that there's zero increments remaining. This means that we've timed out, or that 
        /// `DischargeTimerStatus` simply hasn't been set yet (since it defaults to 0).
        pub const fn timed_out(&self) -> bool {
            self.increments() == 0
        }
    }

    /// Lets you configure the shorting switch for a discharge cell `x`. One-bit field.
    #[repr(u8)]
    #[bitenum]
    #[derive(BitfieldEnumDefault, Copy, Clone, Debug, PartialEq, Eq, Default)]
    pub enum DischargeCellConfig {
        /// Continuously turns off shorting switch for Cell `x` (default).
        #[default]
        #[fallback]
        ShortingSwitchOff = 0,
        /// Continuously turns on shorting switch for Cell `x`.
        ShortingSwitchOn = 1,
    }
}

/// Configuration Register Group B (CFGB). Contains six 1-byte registers (so 6 bytes total).
/// 
/// See Table 56 on page 61 of the datasheet.
#[register_group(
    bytes = 6,
    write = Some(commands::config::wrcfgb().frame()),
    read = commands::config::rdcfgb().frame(),
)]
#[bitfield(u64)]
pub struct ConfigB {
    /// UV comparison voltage (VUV). Cell undervoltage threshold = VUV x 16 x 150uV + 1.5V. 12-bit field. Corresponds to `VUV[11:0]`.
    #[bits(12, default = types::UvComparisonVoltage::DEFAULT)]  pub vuv: types::UvComparisonVoltage,
    /// OV comparison voltage (VOV). Cell overvoltage threshold = VOV × 16 × 150 μV + 1.5 V. 12-bit field. Corresponds to `VOV[11:0]`.
    #[bits(12, default = types::OvComparisonVoltage::DEFAULT)]  pub vov: types::OvComparisonVoltage,
    /// Status of the discharge timer. Corresponds to `DCTO[5:0]`.
    #[bits(6, default = types::DischargeTimerStatus::DEFAULT)]  pub dcto: types::DischargeTimerStatus,
    /// Range of the discharge timer.
    #[bits(1, default = types::DischargeTimerRange::DEFAULT)]   pub dtrng: types::DischargeTimerRange,
    /// Enable/disable discharge timer monitoring.
    #[bits(1, default = types::DischargeTimerMonitor::DEFAULT)] pub dtmen: types::DischargeTimerMonitor,
    /// Discharge Cell 1 Configuration
    #[bits(1, default = types::DischargeCellConfig::DEFAULT)]   pub dcc1: types::DischargeCellConfig,
    /// Discharge Cell 2 Configuration
    #[bits(1, default = types::DischargeCellConfig::DEFAULT)]   pub dcc2: types::DischargeCellConfig,
    /// Discharge Cell 3 Configuration
    #[bits(1, default = types::DischargeCellConfig::DEFAULT)]   pub dcc3: types::DischargeCellConfig,
    /// Discharge Cell 4 Configuration
    #[bits(1, default = types::DischargeCellConfig::DEFAULT)]   pub dcc4: types::DischargeCellConfig,
    /// Discharge Cell 5 Configuration
    #[bits(1, default = types::DischargeCellConfig::DEFAULT)]   pub dcc5: types::DischargeCellConfig,
    /// Discharge Cell 6 Configuration
    #[bits(1, default = types::DischargeCellConfig::DEFAULT)]   pub dcc6: types::DischargeCellConfig,
    /// Discharge Cell 7 Configuration
    #[bits(1, default = types::DischargeCellConfig::DEFAULT)]   pub dcc7: types::DischargeCellConfig,
    /// Discharge Cell 8 Configuration
    #[bits(1, default = types::DischargeCellConfig::DEFAULT)]   pub dcc8: types::DischargeCellConfig,
    /// Discharge Cell 9 Configuration
    #[bits(1, default = types::DischargeCellConfig::DEFAULT)]   pub dcc9: types::DischargeCellConfig,
    /// Discharge Cell 10 Configuration
    #[bits(1, default = types::DischargeCellConfig::DEFAULT)]   pub dcc10: types::DischargeCellConfig,
    /// Discharge Cell 11 Configuration
    #[bits(1, default = types::DischargeCellConfig::DEFAULT)]   pub dcc11: types::DischargeCellConfig,
    /// Discharge Cell 12 Configuration
    #[bits(1, default = types::DischargeCellConfig::DEFAULT)]   pub dcc12: types::DischargeCellConfig,
    /// Discharge Cell 13 Configuration
    #[bits(1, default = types::DischargeCellConfig::DEFAULT)]   pub dcc13: types::DischargeCellConfig,
    /// Discharge Cell 14 Configuration
    #[bits(1, default = types::DischargeCellConfig::DEFAULT)]   pub dcc14: types::DischargeCellConfig,
    /// Discharge Cell 15 Configuration
    #[bits(1, default = types::DischargeCellConfig::DEFAULT)]   pub dcc15: types::DischargeCellConfig,
    /// Discharge Cell 16 Configuration
    #[bits(1, default = types::DischargeCellConfig::DEFAULT)]   pub dcc16: types::DischargeCellConfig,
    #[bits(16, default = 0)]                                    _padding: u16,

}