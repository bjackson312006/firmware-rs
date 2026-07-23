//! Register Layouts and Bit Descriptions for Configuration Register A. See Table 102 of the datasheet.

use bitfield_struct::{bitfield, bitenum};
use super::{Register, RegisterKind};

/// Reference powered up (REFON). One-bit field.
#[repr(u8)]
#[bitenum]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum ReferenceOn {
    /// Reference remains powered up until watchdog timeout.
    On = 1,
    /// Reference shuts down after conversions (default).
    #[default]
    #[fallback]
    Off = 0,
}

/// C-ADC vs. S-ADC comparison voltage theshold. Three-bit field.
#[repr(u8)]
#[bitenum]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum ComparisonThresholdVoltage {
    /// 5.1 mV
    Mv5_1 = 0b000,
    /// 8.1 mV (default)
    #[default]
    #[fallback]
    Mv8_1 = 0b001,
    /// 9.0 mV
    Mv9_0 = 0b010,
    /// 10.05 mV
    Mv10_05 = 0b011,
    /// 15.00 mV
    Mv15_00 = 0b100,
    /// 19.95 mV
    Mv19_95 = 0b101,
    /// 25.05 mV
    Mv25_05 = 0b110,
    /// 40.05 mV
    Mv40_05 = 0b111,
}

/// First register in Configuration Register Group A (CFGAR0). 1 byte. See Table 55 on page 61 of the datasheet.
#[bitfield(u8)]
pub struct ConfigA0 {
    #[bits(3)]
    pub cth: ComparisonThresholdVoltage,
    #[bits(4)]
    _reserved: u8,
    #[bits(1)]
    pub refon: ReferenceOn,
}
impl Register for ConfigA0 {
    fn kind(&self) -> RegisterKind {
        RegisterKind::ReadWrite
    }
}

/// Second register in Configuration Register Group A (CFGAR1). 1 byte. See Table 55 on page 61 of the datasheet.
#[bitfield(u8)]
pub struct ConfigA1 {
    /// Forces oscillator counter fast.
    #[bits(1)]
    pub force_oscillator_counter_fast: bool,
    /// Forces oscillator counter slow.
    #[bits(1)]
    pub force_oscillator_counter_slow: bool,
    /// Forces supply error detection.
    #[bits(1)]
    pub force_supply_error_detection: bool,
    /// `true` selects supply OV and delta detection. `false` selects UV.
    #[bits(1)]
    pub ov_uv: bool,
    /// Sets THSD.
    #[bits(1)]
    pub thsd: bool,
    /// Forces nonvolatile memory (NVM) error detection (ED). Sets CED and SED.
    #[bits(1)]
    pub force_nvmed: bool,
    /// Forces NVM multiple error detection (MED). Sets CMED and SMED.
    #[bits(1)]
    pub force_nvmmed: bool,
    /// Forces TMODCHK.
    #[bits(1)]
    pub force_tomdchk: bool,
}

