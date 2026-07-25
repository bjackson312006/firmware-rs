//! Register Layouts and Bit Descriptions for the Result Register groups (cell voltages, averaged cell voltages, filtered cell voltages, etc).
//! 
//! For more info about these registers, see Table 104 on page 71 of the datasheet
//! and Tables 57 through 88 on pages 61 through 67 of the datasheet.

use bitfield_struct::{bitfield, bitenum};
use adbms6830b_macros::BitfieldEnumDefault;

use super::register_group;
use super::super::commands;

/// Field types relavent to the Result Register groups. See Table 104 on page 71 of the datasheet.
pub mod types {
    use super::{bitenum, bitfield, BitfieldEnumDefault};

    /// Microvolts per CxV code increment.
    const LSB_MICROVOLTS: i32 = 150;
    /// The offset of the measurement formula (+1.5 V).
    const OFFSET_MICROVOLTS: i32 = 1_500_000;

    /// Max voltage the register can represent in microvolts.
    /// This is around +6,415,050 uV / +6.41505 V.
    const MAX_MICROVOLTS: i32 = 6_415_050;

    /// Min voltage the register can represent. This is around -3,415,200 uV, or around -3.4152 V.
    const MIN_MICROVOLTS: i32 = -3_415_200;

    /// Takes in a desired result voltage in microvolts, and returns the raw 16-bit code.
    /// This can be used for `CxV`, `ACxV`, `FCxV`, `SxV`, `GxV`, and `R_GxV`. See Table 104 on page 71 of the datasheet.
    /// 
    /// This is based on the equation from the datasheet:
    /// Cell voltage = CxV x 150uV + 1.5V.
    /// - The voltage is in Volts.
    /// - CxV is a signed 16-bit (two's complement) code that maps to a cell voltage.
    /// 
    /// The inverse (with `x` in microvolts) is:
    /// CxV(x) = (x - 1_500_000) / 150)     (with the result being rounded cus 2400 probably wont divide cleanly most the time)
    /// 
    /// This will return `None` if the microvolts input is outside the representable range.
    const fn result_voltage_from_microvolts(microvolts: i32) -> Option<u16> {
        if microvolts < MIN_MICROVOLTS || microvolts > MAX_MICROVOLTS { return None; }

        let numerator = microvolts - OFFSET_MICROVOLTS;
        let code = if numerator >= 0 {
            (numerator + LSB_MICROVOLTS / 2) / LSB_MICROVOLTS
        } else {
            (numerator - LSB_MICROVOLTS / 2) / LSB_MICROVOLTS
        };

        // Store the two's complement representation.
        Some(code as i16 as u16)
    }

    /// Converts a raw 16-bit two's complement voltage code back into a cell voltage in microvolts.
    /// This can be used for `CxV`, `ACxV`, `FCxV`, `SxV`, `GxV`, and `R_GxV`. See Table 104 on page 71 of the datasheet.
    const fn result_voltage_to_microvolts(code: u16) -> i32 {
        let signed = code as i16; // turn the raw code into a signed i16
        signed as i32 * LSB_MICROVOLTS + OFFSET_MICROVOLTS
    }

    /// Represents a cell voltage result (CxV). The voltage represented by this struct can be returned via `as_microvolts()`.
    /// 
    /// This is a 16-bit ADC measurement value for Cell `x`. Cell voltage for Cell `x` = CxV x 150uV + 1.5V.
    /// CxV is reset to 0x8000 on power-up and after clear command (CLRCELL), which corresponds to -3,415,200 uV / -3.4152 V.
    #[bitfield(u16)]
    pub struct CellVoltage { #[bits(16, default = 0x8000)] inner: u16 }
    impl CellVoltage {
        pub const DEFAULT: CellVoltage = Self::new();

        /// Min voltage the register can represent. This is around -3,415,200 uV, or around -3.4152 V.
        pub const MIN_MICROVOLTS: i32 = MIN_MICROVOLTS;

        /// Max voltage the register can represent in microvolts.
        /// This is around +6,415,050 uV / +6.41505 V.
        pub const MAX_MICROVOLTS: i32 = MAX_MICROVOLTS;

        /// Converts a `CellVoltage` to microvolts.
        pub const fn as_microvolts(&self) -> i32 {
            result_voltage_to_microvolts(self.inner())
        }

        /// Creates a new `CellVoltage` from an input value in uV.
        /// 
        /// `CellVoltage` is read-only so you probably shouldn't need to use this but it's here just in case.
        /// 
        /// ### Parameters
        /// - `microvolts`: Cell voltage, in uV. May be negative.
        /// 
        /// This function will return `None` if your input is outside the `MIN_MICROVOLTS`
        /// to `MAX_MICROVOLTS` range.
        pub const fn from_microvolts(microvolts: i32) -> Option<Self> {
            match result_voltage_from_microvolts(microvolts) {
                Some(inner) => Some(Self::new().with_inner(inner)),
                None => None,
            }
        }
    }

    /// Represents an average cell voltage result (ACxV). The voltage represented by this struct can be returned via `as_microvolts()`.
    /// 
    /// This is a 16-bit average of 8 conversion results for value Cell `x`. Averaged Cell voltage for Cell `x` = CxV x 150uV + 1.5V.
    /// ACxV is reset to 0x8000 on power-up and after clear command (CLRCELL), which corresponds to -3,415,200 uV / -3.4152 V.
    #[bitfield(u16)]
    pub struct AverageCellVoltage { #[bits(16, default = 0x8000)] inner: u16 }
    impl AverageCellVoltage {
        pub const DEFAULT: AverageCellVoltage = Self::new();

        /// Min voltage the register can represent. This is around -3,415,200 uV, or around -3.4152 V.
        pub const MIN_MICROVOLTS: i32 = MIN_MICROVOLTS;

        /// Max voltage the register can represent in microvolts.
        /// This is around +6,415,050 uV / +6.41505 V.
        pub const MAX_MICROVOLTS: i32 = MAX_MICROVOLTS;

        /// Converts a `AverageCellVoltage` to microvolts.
        pub const fn as_microvolts(&self) -> i32 {
            result_voltage_to_microvolts(self.inner())
        }

        /// Creates a new `AverageCellVoltage` from an input value in uV.
        /// 
        /// `AverageCellVoltage` is read-only so you probably shouldn't need to use this but it's here just in case.
        /// 
        /// ### Parameters
        /// - `microvolts`: Average cell voltage, in uV. May be negative.
        /// 
        /// This function will return `None` if your input is outside the `MIN_MICROVOLTS`
        /// to `MAX_MICROVOLTS` range.
        pub const fn from_microvolts(microvolts: i32) -> Option<Self> {
            match result_voltage_from_microvolts(microvolts) {
                Some(inner) => Some(Self::new().with_inner(inner)),
                None => None,
            }
        }
    }

    /// Represents an filtered cell voltage result (FCxV). The voltage represented by this struct can be returned via `as_microvolts()`.
    /// 
    /// This is a 16-bit IIR filtered measurement value for Cell `x`. Filtered Cell voltage for Cell `x` = CxV x 150uV + 1.5V.
    /// FCxV is reset to 0x8000 on power-up and after clear command (CLRCELL), which corresponds to -3,415,200 uV / -3.4152 V.
    #[bitfield(u16)]
    pub struct FilteredCellVoltage { #[bits(16, default = 0x8000)] inner: u16 }
    impl FilteredCellVoltage {
        pub const DEFAULT: FilteredCellVoltage = Self::new();

        /// Min voltage the register can represent. This is around -3,415,200 uV, or around -3.4152 V.
        pub const MIN_MICROVOLTS: i32 = MIN_MICROVOLTS;

        /// Max voltage the register can represent in microvolts.
        /// This is around +6,415,050 uV / +6.41505 V.
        pub const MAX_MICROVOLTS: i32 = MAX_MICROVOLTS;

        /// Converts a `FilteredCellVoltage` to microvolts.
        pub const fn as_microvolts(&self) -> i32 {
            result_voltage_to_microvolts(self.inner())
        }

        /// Creates a new `FilteredCellVoltage` from an input value in uV.
        /// 
        /// `FilteredCellVoltage` is read-only so you probably shouldn't need to use this but it's here just in case.
        /// 
        /// ### Parameters
        /// - `microvolts`: Filtered cell voltage, in uV. May be negative.
        /// 
        /// This function will return `None` if your input is outside the `MIN_MICROVOLTS`
        /// to `MAX_MICROVOLTS` range.
        pub const fn from_microvolts(microvolts: i32) -> Option<Self> {
            match result_voltage_from_microvolts(microvolts) {
                Some(inner) => Some(Self::new().with_inner(inner)),
                None => None,
            }
        }
    }

    /// Represents an S-pin voltage result (SxV). The voltage represented by this struct can be returned via `as_microvolts()`.
    /// 
    /// This is a 16-bit ADC measurement value for Sx pin from ADSV or ADCV commands. S-pin voltage for channel `x` = SxV x 150uV + 1.5V.
    /// SxV is reset to 0x8000 on power-up and after clear command (CLRCELL), which corresponds to -3,415,200 uV / -3.4152 V.
    #[bitfield(u16)]
    pub struct SPinVoltage { #[bits(16, default = 0x8000)] inner: u16 }
    impl SPinVoltage {
        pub const DEFAULT: SPinVoltage = Self::new();

        /// Min voltage the register can represent. This is around -3,415,200 uV, or around -3.4152 V.
        pub const MIN_MICROVOLTS: i32 = MIN_MICROVOLTS;

        /// Max voltage the register can represent in microvolts.
        /// This is around +6,415,050 uV / +6.41505 V.
        pub const MAX_MICROVOLTS: i32 = MAX_MICROVOLTS;

        /// Converts a `SPinVoltage` to microvolts.
        pub const fn as_microvolts(&self) -> i32 {
            result_voltage_to_microvolts(self.inner())
        }

        /// Creates a new `SPinVoltage` from an input value in uV.
        /// 
        /// `SPinVoltage` is read-only so you probably shouldn't need to use this but it's here just in case.
        /// 
        /// ### Parameters
        /// - `microvolts`: Filtered cell voltage, in uV. May be negative.
        /// 
        /// This function will return `None` if your input is outside the `MIN_MICROVOLTS`
        /// to `MAX_MICROVOLTS` range.
        pub const fn from_microvolts(microvolts: i32) -> Option<Self> {
            match result_voltage_from_microvolts(microvolts) {
                Some(inner) => Some(Self::new().with_inner(inner)),
                None => None,
            }
        }
    }

    /// Represents a Redundant GPIO voltage result (GxV, R_GxV). The voltage represented by this struct can be returned via `as_microvolts()`.
    /// 
    /// This is a 16-bit ADC measurement value for (redundant) GPIOx voltage for GPIOx = GxV x 150 uV + 1.5 V.
    #[bitfield(u16)]
    pub struct RedundantGpioVoltage { #[bits(16, default = 0x8000)]  inner: u16 }
    impl RedundantGpioVoltage {
        pub const DEFAULT: RedundantGpioVoltage = Self::new();

        /// Min voltage the register can represent. This is around -3,415,200 uV, or around -3.4152 V.
        pub const MIN_MICROVOLTS: i32 = MIN_MICROVOLTS;

        /// Max voltage the register can represent in microvolts.
        /// This is around +6,415,050 uV / +6.41505 V.
        pub const MAX_MICROVOLTS: i32 = MAX_MICROVOLTS;

        /// Converts a `RedundantGpioVoltage` to microvolts.
        pub const fn as_microvolts(&self) -> i32 {
            result_voltage_to_microvolts(self.inner())
        }

        /// Creates a new `RedundantGpioVoltage` from an input value in uV.
        /// 
        /// `RedundantGpioVoltage` is read-only so you probably shouldn't need to use this but it's here just in case.
        /// 
        /// ### Parameters
        /// - `microvolts`: Filtered cell voltage, in uV. May be negative.
        /// 
        /// This function will return `None` if your input is outside the `MIN_MICROVOLTS`
        /// to `MAX_MICROVOLTS` range.
        pub const fn from_microvolts(microvolts: i32) -> Option<Self> {
            match result_voltage_from_microvolts(microvolts) {
                Some(inner) => Some(Self::new().with_inner(inner)),
                None => None,
            }
        }
    }
}

/// Cell Voltage Register Group A (CVA). Contains six 1-byte registers (so 6 bytes total).
/// 
/// See Table 57 on page 61 of the datasheet.
#[register_group(
    bytes = 6,
    write = None,
    read = commands::cell_voltage::rdcva().frame(),
)]
#[bitfield(u64)]
pub struct CellVoltagesA {
    /// Cell 1 Voltage Result. Corresponds to `C1V[15:0]`.
    #[bits(16, default = types::CellVoltage::DEFAULT)]  pub c1v: types::CellVoltage,
    /// Cell 2 Voltage Result. Corresponds to `C2V[15:0]`.
    #[bits(16, default = types::CellVoltage::DEFAULT)]  pub c2v: types::CellVoltage,
    /// Cell 3 Voltage Result. Corresponds to `C3V[15:0]`.
    #[bits(16, default = types::CellVoltage::DEFAULT)]  pub c3v: types::CellVoltage,
    #[bits(16, default = 0)]                            _padding: u16,
}

/// Cell Voltage Register Group B (CVB). Contains six 1-byte registers (so 6 bytes total).
/// 
/// See Table 58 on page 61 of the datasheet.
#[register_group(
    bytes = 6,
    write = None,
    read = commands::cell_voltage::rdcvb().frame(),
)]
#[bitfield(u64)]
pub struct CellVoltagesB {
    /// Cell 4 Voltage Result. Corresponds to `C4V[15:0]`.
    #[bits(16, default = types::CellVoltage::DEFAULT)]  pub c4v: types::CellVoltage,
    /// Cell 5 Voltage Result. Corresponds to `C5V[15:0]`.
    #[bits(16, default = types::CellVoltage::DEFAULT)]  pub c5v: types::CellVoltage,
    /// Cell 6 Voltage Result. Corresponds to `C6V[15:0]`.
    #[bits(16, default = types::CellVoltage::DEFAULT)]  pub c6v: types::CellVoltage,
    #[bits(16, default = 0)]                            _padding: u16,
}

/// Cell Voltage Register Group C (CVC). Contains six 1-byte registers (so 6 bytes total).
/// 
/// See Table 59 on page 62 of the datasheet.
#[register_group(
    bytes = 6,
    write = None,
    read = commands::cell_voltage::rdcvc().frame(),
)]
#[bitfield(u64)]
pub struct CellVoltagesC {
    /// Cell 7 Voltage Result. Corresponds to `C7V[15:0]`.
    #[bits(16, default = types::CellVoltage::DEFAULT)]  pub c7v: types::CellVoltage,
    /// Cell 8 Voltage Result. Corresponds to `C8V[15:0]`.
    #[bits(16, default = types::CellVoltage::DEFAULT)]  pub c8v: types::CellVoltage,
    /// Cell 9 Voltage Result. Corresponds to `C9V[15:0]`.
    #[bits(16, default = types::CellVoltage::DEFAULT)]  pub c9v: types::CellVoltage,
    #[bits(16, default = 0)]                            _padding: u16,
}

/// Cell Voltage Register Group D (CVD). Contains six 1-byte registers (so 6 bytes total).
/// 
/// See Table 60 on page 62 of the datasheet.
#[register_group(
    bytes = 6,
    write = None,
    read = commands::cell_voltage::rdcvd().frame(),
)]
#[bitfield(u64)]
pub struct CellVoltagesD {
    /// Cell 10 Voltage Result. Corresponds to `C10V[15:0]`.
    #[bits(16, default = types::CellVoltage::DEFAULT)]  pub c10v: types::CellVoltage,
    /// Cell 11 Voltage Result. Corresponds to `C11V[15:0]`.
    #[bits(16, default = types::CellVoltage::DEFAULT)]  pub c11v: types::CellVoltage,
    /// Cell 12 Voltage Result. Corresponds to `C12V[15:0]`.
    #[bits(16, default = types::CellVoltage::DEFAULT)]  pub c12v: types::CellVoltage,
    #[bits(16, default = 0)]                            _padding: u16,
}

/// Cell Voltage Register Group E (CVE). Contains six 1-byte registers (so 6 bytes total).
/// 
/// See Table 61 on page 62 of the datasheet.
#[register_group(
    bytes = 6,
    write = None,
    read = commands::cell_voltage::rdcve().frame(),
)]
#[bitfield(u64)]
pub struct CellVoltagesE {
    /// Cell 13 Voltage Result. Corresponds to `C13V[15:0]`.
    #[bits(16, default = types::CellVoltage::DEFAULT)]  pub c13v: types::CellVoltage,
    /// Cell 14 Voltage Result. Corresponds to `C14[15:0]`.
    #[bits(16, default = types::CellVoltage::DEFAULT)]  pub c14v: types::CellVoltage,
    /// Cell 15 Voltage Result. Corresponds to `C15V[15:0]`.
    #[bits(16, default = types::CellVoltage::DEFAULT)]  pub c15v: types::CellVoltage,
    #[bits(16, default = 0)]                            _padding: u16,
}

/// Cell Voltage Register Group F (CVF). Contains six 1-byte registers (so 6 bytes total).
/// 
/// See Table 62 on page 62 of the datasheet.
#[register_group(
    bytes = 6,
    write = None,
    read = commands::cell_voltage::rdcvf().frame(),
)]
#[bitfield(u64)]
pub struct CellVoltagesF {
    /// Cell 16 Voltage Result. Corresponds to `C16V[15:0]`.
    #[bits(16, default = types::CellVoltage::DEFAULT)]  pub c16v: types::CellVoltage,
    #[bits(32, default = u32::MAX)]                     _reserved: u32,
    #[bits(16, default = 0)]                            _padding: u16,
}