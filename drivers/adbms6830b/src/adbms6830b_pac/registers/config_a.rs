//! Register Layouts and Bit Descriptions for Configuration Register A. See Table 102 of the datasheet.

use bitfield_struct::{bitfield, bitenum};

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
    cth: ComparisonThresholdVoltage,
    #[bits(4)]
    _reserved: u8,
    #[bits(1)]
    refon: ReferenceOn,
}

