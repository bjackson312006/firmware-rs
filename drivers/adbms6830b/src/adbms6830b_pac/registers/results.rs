//! Register Layouts and Bit Descriptions for the Result Register groups (cell voltages, averaged cell voltages, filtered cell voltages, etc).
//! 
//! For more info about these registers, see Table 104 on page 71 of the datasheet
//! and Tables 57 through 88 on pages 61 through 67 of the datasheet.
//! 
//! ### Getting Started/Cool Tips
//! - If you want to read all cell voltages, send a `CellVoltagesAllReadRequest`, and then read in the response
//! as a `CellVoltagesAllReadResponse`.
//! - There's also `AverageCellVoltagesReadRequest`/`AverageCellVoltagesReadResponse`, `FilteredCellVoltagesReadRequest`/`FilteredCellVoltagesReadResponse`, etc.
//! - If you want to read a specific cell register group, there's types like `CellVoltagesAReadRequest`/`CellVoltagesAReadResponse`, `CellVoltagesBReadRequest`/`CellVoltagesBReadResponse`, etc.

use bitfield_struct::{bitfield, bitenum};
use adbms6830b_macros::BitfieldEnumDefault;

use super::{register_group, register_group_aggregate};
use super::super::commands;

/// Field types relavent to the Result Register groups. See Table 104 on page 71 of the datasheet.
pub mod types {
    use super::{bitenum, bitfield, BitfieldEnumDefault};

    /// Private type for the registers from the first row of Table 107.
    /// All these register types use this math:
    /// `CxV`, `SxV`, `ACxV`, `FCxV`, `GxV`, `R_GxV`, `VREF2`, `VD`, `VA`, `VRES`, `VMV`
    #[bitfield(u16)]
    #[derive(PartialEq, Eq)]
    struct FirstRowRegisters { #[bits(16, default = 0x8000)] inner: u16 }
    impl FirstRowRegisters {
        /// Microvolts per register code increment.
        pub const REGISTER_LSB_MICROVOLTS: i32 = 150;
        /// Width of this register in bits.
        pub const REGISTER_WIDTH_BITS: i32 = 16;
        /// Offset in microvolts of the measurement formula (1.5 V).
        pub const OFFSET_MICROVOLTS: i32 = 1_500_000;

        /// Max voltage the register can represent in microvolts.
        /// This is around +6,415,050 uV / +6.41505 V.
        pub const MAX_MICROVOLTS: i32 = 6_415_050;

        /// Min voltage the register can represent. This is around -3,415,200 uV, or around -3.4152 V.
        pub const MIN_MICROVOLTS: i32 = -3_415_200;

        /// Takes in a desired result voltage in microvolts, and returns the raw 16-bit code.
        /// See Table 104 on page 71 of the datasheet.
        /// 
        /// This is based on the equation from the datasheet:
        /// Cell voltage = CxV x 150uV + 1.5V.
        /// - The voltage is in Volts.
        /// - CxV is a signed 16-bit (two's complement) code that maps to a cell voltage.
        /// 
        /// The inverse (with `x` in microvolts) is:
        /// CxV(x) = ((x - 1_500_000) / 150))     (with the result being rounded cus 150 probably wont divide cleanly most the time)
        /// 
        /// This will return `None` if the microvolts input is outside the representable range.
        const fn firstrow_voltage_from_microvolts(microvolts: i32) -> Option<u16> {
            if microvolts < Self::MIN_MICROVOLTS || microvolts > Self::MAX_MICROVOLTS { return None; }

            let numerator = microvolts - Self::OFFSET_MICROVOLTS;
            let code = if numerator >= 0 {
                (numerator + Self::REGISTER_LSB_MICROVOLTS / 2) / Self::REGISTER_LSB_MICROVOLTS
            } else {
                (numerator - Self::REGISTER_LSB_MICROVOLTS / 2) / Self::REGISTER_LSB_MICROVOLTS
            };

            // Store the two's complement representation.
            Some(code as i16 as u16)
        }

        /// Converts a raw 16-bit two's complement voltage code back into a cell voltage in microvolts.
        /// See Table 104 on page 71 of the datasheet.
        const fn firstrow_voltage_to_microvolts(code: u16) -> i32 {
            let signed = code as i16; // turn the raw code into a signed i16
            signed as i32 * Self::REGISTER_LSB_MICROVOLTS + Self::OFFSET_MICROVOLTS
        }

        pub const DEFAULT: FirstRowRegisters = Self::new();

        // this stuff is needed for the "read all" command and the associated serialization via `#[register_group_aggregate]`:
        /// The number of protocol bytes this result value occupies.
        pub const BYTES: usize = 2;
        /// Serializes into protocol bytes (little-endian) since this is the datasheet register order.
        pub const fn to_bytes(self) -> [u8; 2] { self.into_bits().to_le_bytes() }
        /// Reconstructs from protocl bytes (little-endian).
        pub const fn from_bytes(bytes: [u8; 2]) -> Self { Self::from_bits(u16::from_le_bytes(bytes)) }

        /// Converts a `FirstRowRegisters` to microvolts.
        pub const fn as_microvolts(&self) -> i32 { Self::firstrow_voltage_to_microvolts(self.inner()) }

        /// Creates a new `FirstRowRegisters` from an input value in uV.
        /// 
        /// ### Parameters
        /// - `microvolts`: Voltage, in uV. May be negative.
        /// 
        /// This function will return `None` if your input is outside the `MIN_MICROVOLTS`
        /// to `MAX_MICROVOLTS` range.
        pub const fn from_microvolts(microvolts: i32) -> Option<Self> {
            match Self::firstrow_voltage_from_microvolts(microvolts) {
                Some(inner) => Some(Self::new().with_inner(inner)),
                None => None,
            }
        }
    }

    /// This macro implements a newtype around a `FirstRowRegister`. You can put docs in the macro to inject custom docs for the generated struct.
    macro_rules! impl_firstrowregister {
        (
            $(#[$outer:meta])*
            $structname:ident
        ) => {
            $(#[$outer])*
            #[derive(Debug, Copy, Clone, Eq, PartialEq)]
            pub struct $structname { inner: FirstRowRegisters }
            impl $structname {
                pub const DEFAULT: $structname = Self { inner: FirstRowRegisters::new() };

                // this stuff is needed for the "read all" command and the associated serialization via `#[register_group_aggregate]`:
                /// The number of protocol bytes this result value occupies.
                pub const BYTES: usize = FirstRowRegisters::BYTES;
                /// Serializes into protocol bytes (little-endian) since this is the datasheet register order.
                pub const fn to_bytes(self) -> [u8; 2] { self.inner.to_bytes() }
                /// Reconstructs from protocl bytes (little-endian).
                pub const fn from_bytes(bytes: [u8; 2]) -> Self { Self { inner: FirstRowRegisters::from_bytes(bytes) } }
                /// Convert from bits.
                pub const fn from_bits(bits: u16) -> Self { Self { inner: FirstRowRegisters::from_bits(bits) } }
                /// Convert into bits.
                pub const fn into_bits(self) -> u16 { self.inner.into_bits() }

                /// Min voltage the register can represent. This is around -3,415,200 uV, or around -3.4152 V.
                pub const MIN_MICROVOLTS: i32 = FirstRowRegisters::MIN_MICROVOLTS;

                /// Max voltage the register can represent in microvolts.
                /// This is around +6,415,050 uV / +6.41505 V.
                pub const MAX_MICROVOLTS: i32 = FirstRowRegisters::MAX_MICROVOLTS;

                #[doc = concat!("Converts a `", stringify!($structname), "` to microvolts.")]
                pub const fn as_microvolts(&self) -> i32 { self.inner.as_microvolts() }

                #[doc = concat!("Creates a new `", stringify!($structname), "` from an input value in uV.\n")]
                /// ### Parameters
                /// - `microvolts`: Voltage, in uV. May be negative.
                /// 
                /// This function will return `None` if your input is outside the `MIN_MICROVOLTS`
                /// to `MAX_MICROVOLTS` range.
                pub const fn from_microvolts(microvolts: i32) -> Option<Self> {
                    match FirstRowRegisters::from_microvolts(microvolts) {
                        Some(inner) => Some(Self { inner }),
                        None => None,
                    }
                }
            }
        }
    }

    /// Private type for the `VPV` register type from Table 107 (row 4).
    /// 
    /// This type doesn't need to exist probably since this logic is only used by one public type but I like the
    /// consistency of this pattern so why not
    #[bitfield(u16)]
    #[derive(Eq, PartialEq)]
    struct VpvInner { #[bits(16, default = 0x8000)] inner: u16 }
    impl VpvInner {
        /// Microvolts per VPV code increment. This is `25 × 150 µV` because of the VPV gain factor.
        const VPV_LSB_MICROVOLTS: i32 = 3_750;
        /// The offset of the VPV measurement formula (`25 × +1.5 V` = +37.5 V).
        const VPV_OFFSET_MICROVOLTS: i32 = 37_500_000;

        /// Max voltage VPV can represent in microvolts.
        /// This is around +160,376,250 uV / +160.37625 V (raw code 0x7FFF).
        const VPV_MAX_MICROVOLTS: i32 = 160_376_250;

        /// Min voltage VPV can represent. This is around -85,380,000 uV, or around -85.38 V (raw code 0x8000).
        const VPV_MIN_MICROVOLTS: i32 = -85_380_000;

        /// Takes in a desired `VPV` (V+ to V−) voltage in microvolts, and returns the raw 16-bit code.
        /// This is for `VPV` only. See Table 104 on page 71 of the datasheet.
        ///
        /// This is based on the equation from the datasheet (`VPV` is the raw signed 16-bit ADC code):
        /// V+ to V- = 25 x (VPV x 150uV + 1.5V) = VPV x 3750uV + 37.5V.
        /// - The voltage is in Volts.
        /// - VPV is a signed 16-bit (two's complement) ADC code, gained up by 25x.
        ///
        /// The inverse (with `x` in microvolts) is:
        /// VPV(x) = ((x - 37_500_000) / 3750)     (with the result being rounded cus 3750 probably wont divide cleanly most the time)
        ///
        /// This will return `None` if the microvolts input is outside the representable range.
        const fn vpv_voltage_from_microvolts(microvolts: i32) -> Option<u16> {
            if microvolts < Self::VPV_MIN_MICROVOLTS || microvolts > Self::VPV_MAX_MICROVOLTS { return None; }

            let numerator = microvolts - Self::VPV_OFFSET_MICROVOLTS;
            let code = if numerator >= 0 {
                (numerator + Self::VPV_LSB_MICROVOLTS / 2) / Self::VPV_LSB_MICROVOLTS
            } else {
                (numerator - Self::VPV_LSB_MICROVOLTS / 2) / Self::VPV_LSB_MICROVOLTS
            };

            // Store the two's complement representation.
            Some(code as i16 as u16)
        }

        /// Converts a raw 16-bit two's complement `VPV` code back into microvolts.
        /// This is for `VPV` only. See Table 104 on page 71 of the datasheet.
        ///
        /// V+ to V- = 25 x (VPV x 150uV + 1.5V) = VPV x 3750uV + 37.5V.
        const fn vpv_voltage_to_microvolts(code: u16) -> i32 {
            let signed = code as i16; // turn the raw code into a signed i16
            signed as i32 * Self::VPV_LSB_MICROVOLTS + Self::VPV_OFFSET_MICROVOLTS
        }

        // this stuff is usually needed for the "read all" command and the associated serialization via `#[register_group_aggregate]`, but
        // this type doesn't even need this since the register groups that use it don't have a "read all" command. Still gonna keep this stuff here though just in case.
        /// The number of protocol bytes this result value occupies.
        pub const BYTES: usize = 2;
        /// Serializes into protocol bytes (little-endian) since this is the datasheet register order.
        pub const fn to_bytes(self) -> [u8; 2] { self.into_bits().to_le_bytes() }
        /// Reconstructs from protocl bytes (little-endian).
        pub const fn from_bytes(bytes: [u8; 2]) -> Self { Self::from_bits(u16::from_le_bytes(bytes)) }

        /// Converts a `VPlusVoltage` to microvolts.
        pub const fn as_microvolts(&self) -> i32 {
            Self::vpv_voltage_to_microvolts(self.inner())
        }

        /// Creates a new `VPlusVoltage` from an input value in uV.
        /// 
        /// `VPlusVoltage` is read-only so you probably shouldn't need to use this but it's here just in case.
        /// 
        /// ### Parameters
        /// - `microvolts`: VPV voltage, in uV. May be negative.
        /// 
        /// This function will return `None` if your input is outside the `VPV_MIN_MICROVOLTS`
        /// to `VPV_MAX_MICROVOLTS` range.
        pub const fn from_microvolts(microvolts: i32) -> Option<Self> {
            match Self::vpv_voltage_from_microvolts(microvolts) {
                Some(inner) => Some(Self::new().with_inner(inner)),
                None => None,
            }
        }
    }

    /// This macro implements a newtype around a `VpvInner`. You can put docs in the macro to inject custom docs for the generated struct.
    macro_rules! impl_vpvinner {
        (
            $(#[$outer:meta])*
            $structname:ident
        ) => {
            $(#[$outer])*
            #[derive(Debug, Copy, Clone, Eq, PartialEq)]
            pub struct $structname { inner: VpvInner }
            impl $structname {
                pub const DEFAULT: $structname = Self { inner: VpvInner::new() };

                // this stuff is usually needed for the "read all" command and the associated serialization via `#[register_group_aggregate]`, but
                // this type doesn't even need this since the register groups that use it don't have a "read all" command. Still gonna keep this stuff here though just in case.
                /// The number of protocol bytes this result value occupies.
                pub const BYTES: usize = 2;
                /// Serializes into protocol bytes (little-endian) since this is the datasheet register order.
                pub const fn to_bytes(self) -> [u8; 2] { self.inner.to_bytes() }
                /// Reconstructs from protocl bytes (little-endian).
                pub const fn from_bytes(bytes: [u8; 2]) -> Self { Self { inner: VpvInner::from_bytes(bytes) } }
                /// Convert from bits.
                pub const fn from_bits(bits: u16) -> Self { Self { inner: VpvInner::from_bits(bits) } }
                /// Convert into bits.
                pub const fn into_bits(self) -> u16 { self.inner.into_bits() }

                /// Min voltage the register can represent. This is around -85,380,000 uV, or around -85.38 V.
                pub const VPV_MIN_MICROVOLTS: i32 = VpvInner::VPV_MIN_MICROVOLTS;

                /// Max voltage the register can represent in microvolts.
                /// This is around +160,376,250 uV / +160.37625 V.
                pub const VPV_MAX_MICROVOLTS: i32 = VpvInner::VPV_MAX_MICROVOLTS;

                #[doc = concat!("Converts a `", stringify!($structname), "` to microvolts.")]
                pub const fn as_microvolts(&self) -> i32 { self.inner.as_microvolts() }

                #[doc = concat!("Creates a new `", stringify!($structname), "` from an input value in uV.\n")]
                /// 
                /// ### Parameters
                /// - `microvolts`: VPV voltage, in uV. May be negative.
                /// 
                /// This function will return `None` if your input is outside the `VPV_MIN_MICROVOLTS`
                /// to `VPV_MAX_MICROVOLTS` range.
                pub const fn from_microvolts(microvolts: i32) -> Option<Self> {
                    match VpvInner::from_microvolts(microvolts) {
                        Some(inner) => Some(Self { inner: inner }),
                        None => None,
                    }
                }
            }
        }
    }

    impl_firstrowregister!(
        /// Represents a cell voltage result (CxV). The voltage represented by this struct can be returned via `as_microvolts()`.
        ///
        /// This is a 16-bit ADC measurement value for Cell `x`. Cell voltage for Cell `x` = CxV x 150uV + 1.5V.
        /// CxV is reset to 0x8000 on power-up and after clear command (CLRCELL), which corresponds to -3,415,200 uV / -3.4152 V.
        CellVoltage
    );

    impl_firstrowregister!(
        /// Represents an average cell voltage result (ACxV). The voltage represented by this struct can be returned via `as_microvolts()`.
        /// 
        /// This is a 16-bit average of 8 conversion results for value Cell `x`. Averaged Cell voltage for Cell `x` = CxV x 150uV + 1.5V.
        /// ACxV is reset to 0x8000 on power-up and after clear command (CLRCELL), which corresponds to -3,415,200 uV / -3.4152 V.
        AverageCellVoltage
    );

    impl_firstrowregister!(
        /// Represents an filtered cell voltage result (FCxV). The voltage represented by this struct can be returned via `as_microvolts()`.
        /// 
        /// This is a 16-bit IIR filtered measurement value for Cell `x`. Filtered Cell voltage for Cell `x` = CxV x 150uV + 1.5V.
        /// FCxV is reset to 0x8000 on power-up and after clear command (CLRCELL), which corresponds to -3,415,200 uV / -3.4152 V.
        FilteredCellVoltage
    );

    impl_firstrowregister!(
        /// Represents an S-pin voltage result (SxV). The voltage represented by this struct can be returned via `as_microvolts()`.
        /// 
        /// This is a 16-bit ADC measurement value for Sx pin from ADSV or ADCV commands. S-pin voltage for channel `x` = SxV x 150uV + 1.5V.
        /// SxV is reset to 0x8000 on power-up and after clear command (CLRCELL), which corresponds to -3,415,200 uV / -3.4152 V.
        SVoltage
    );

    impl_firstrowregister!(
        /// Represents a GPIO voltage result (GxV). The voltage represented by this struct can be returned via `as_microvolts()`.
        /// 
        /// This is a 16-bit ADC measurement value for GPIOx voltage for GPIOx = GxV x 150 uV + 1.5 V.
        /// 
        /// Note: This type is essentially identical to `RedundantGpioVoltage`, and only exists for verbosity and to be
        /// consistent with the datasheet. So, `GpioVoltage` and `RedundantGpioVoltage` both implement `From` so they can be converted to and from each other
        /// at will. Importantly, even though these two types are identical in what kind of value they represent, they are not identical in meaning and
        /// come from different register groups.
        /// 
        /// Also, if you're in a `const` context, prefer using `from_redundant()` instead of the `From`/`.into()` trait stuff.
        GpioVoltage
    );
    impl From<RedundantGpioVoltage> for GpioVoltage {
        fn from(value: RedundantGpioVoltage) -> Self {
            Self::from_redundant(value)
        }
    }
    impl GpioVoltage {
        /// Creates a `GpioVoltage` from a `RedundantGpioVoltage`.
        pub const fn from_redundant(value: RedundantGpioVoltage) -> Self {
            GpioVoltage::from_bits(value.into_bits())
        }
    }

    impl_firstrowregister!(
        /// Represents a Redundant GPIO voltage result (R_GxV). The voltage represented by this struct can be returned via `as_microvolts()`.
        /// 
        /// This is a 16-bit ADC measurement value for GPIOx voltage for GPIOx = GxV x 150 uV + 1.5 V.
        /// 
        /// Note: This type is essentially identical to `GpioVoltage`, and only exists for verbosity and to be
        /// consistent with the datasheet. So, `GpioVoltage` and `RedundantGpioVoltage` both implement `From` so they can be converted to and from each other
        /// at will. Importantly, even though these two types are identical in what kind of value they represent, they are not identical in meaning and
        /// come from different register groups.
        /// 
        /// Also, if you're in a `const` context, prefer using `from_standard()` instead of the `From`/`.into()` trait stuff.
        RedundantGpioVoltage
    );
    impl From<GpioVoltage> for RedundantGpioVoltage {
        fn from(value: GpioVoltage) -> Self {
            Self::from_standard(value)
        }
    }
    impl RedundantGpioVoltage {
        /// Creates a `RedundantGpioVoltage` from a standard `GpioVoltage`.
        pub const fn from_standard(value: GpioVoltage) -> Self {
            RedundantGpioVoltage::from_bits(value.into_bits())
        }
    }

    impl_firstrowregister!(
        /// Represents a voltage measurement from S1N to V- (VMV). The voltage represented by this struct can be returned via `as_microvolts()`.
        /// 
        /// This is a 16-bit ADC measurement value for S1N to V- = VMV x 150 uV + 1.5 V).
        /// 
        /// Reset to 0x8000 after power-up, sleep, or clear command (CLRAUX).
        VMinusVoltage
    );

    impl_vpvinner!(
        /// Represents a voltage measurement from V+ to V- voltage result (VPV). The voltage represented by this struct can be returned via `as_microvolts()`.
        /// 
        /// This is a 16-bit ADC measurement value for V+ to V- = 25 x (VPV x 150 uV + 1.5 V).
        /// 
        /// Reset to 0x8000 after power-up, sleep, or clear command (CLRAUX).
        VPlusVoltage
    );
}

/// Cell Voltage Register Group A (CVA). Contains six 1-byte registers (so 6 bytes total).
/// 
/// Contains cells 1 through 3.
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
/// Contains cells 4 through 6.
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
/// Contains cells 7 through 9.
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
/// Contains cells 10 through 12.
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
/// Contains cells 13 through 15.
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
/// Just contains cell 16.
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

/// All cell voltage results (Cell Voltage Register Group A through F).
#[register_group_aggregate(
    read = commands::cell_voltage::rdcvall().frame(),
)]
#[derive(Clone, Copy, Debug)]
pub struct CellVoltagesAll {
    /// Cells 1–3 (Cell Voltage Register Group A).
    pub a: CellVoltagesA,
    /// Cells 4–6 (Cell Voltage Register Group B).
    pub b: CellVoltagesB,
    /// Cells 7–9 (Cell Voltage Register Group C).
    pub c: CellVoltagesC,
    /// Cells 10–12 (Cell Voltage Register Group D).
    pub d: CellVoltagesD,
    /// Cells 13–15 (Cell Voltage Register Group E).
    pub e: CellVoltagesE,
    /// Cell 16 (Cell Voltage Register Group F).
    /// 
    /// There's only 1 cell in Cell Voltage Register Group F, so this type has to be `types::CellVoltage` instead of `CellVoltagesF`.
    /// Otherwise the serialization would get messed up. Sad! This is bothering me but I may have OCD.
    pub f: types::CellVoltage,
}

/// Avergage Cell Voltage Register Group A (ACA). Contains six 1-byte registers (so 6 bytes total).
/// 
/// Contains cells 1-3.
/// 
/// See Table 63 on page 62 of the datasheet.
#[register_group(
    bytes = 6,
    write = None,
    read = commands::avg_cell_voltage::rdaca().frame(),
)]
#[bitfield(u64)]
pub struct AverageCellVoltagesA {
    /// Cell 1 Average Voltage Result. Corresponds to `AC1V[15:0]`.
    #[bits(16, default = types::AverageCellVoltage::DEFAULT)]  pub ac1v: types::AverageCellVoltage,
    /// Cell 2 Average Voltage Result. Corresponds to `AC2V[15:0]`.
    #[bits(16, default = types::AverageCellVoltage::DEFAULT)]  pub ac2v: types::AverageCellVoltage,
    /// Cell 3 Average Voltage Result. Corresponds to `AC3V[15:0]`.
    #[bits(16, default = types::AverageCellVoltage::DEFAULT)]  pub ac3v: types::AverageCellVoltage,
    #[bits(16, default = 0)]                                   _padding: u16,
}

/// Avergage Cell Voltage Register Group B (ACB). Contains six 1-byte registers (so 6 bytes total).
/// 
/// Contains cells 4-6.
/// 
/// See Table 64 on page 63 of the datasheet.
#[register_group(
    bytes = 6,
    write = None,
    read = commands::avg_cell_voltage::rdacb().frame(),
)]
#[bitfield(u64)]
pub struct AverageCellVoltagesB {
    /// Cell 4 Average Voltage Result. Corresponds to `AC4V[15:0]`.
    #[bits(16, default = types::AverageCellVoltage::DEFAULT)]  pub ac4v: types::AverageCellVoltage,
    /// Cell 5 Average Voltage Result. Corresponds to `AC5V[15:0]`.
    #[bits(16, default = types::AverageCellVoltage::DEFAULT)]  pub ac5v: types::AverageCellVoltage,
    /// Cell 6 Average Voltage Result. Corresponds to `AC6V[15:0]`.
    #[bits(16, default = types::AverageCellVoltage::DEFAULT)]  pub ac6v: types::AverageCellVoltage,
    #[bits(16, default = 0)]                                   _padding: u16,
}

/// Avergage Cell Voltage Register Group C (ACC). Contains six 1-byte registers (so 6 bytes total).
/// 
/// Contains cells 7-9.
/// 
/// See Table 65 on page 63 of the datasheet.
#[register_group(
    bytes = 6,
    write = None,
    read = commands::avg_cell_voltage::rdacc().frame(),
)]
#[bitfield(u64)]
pub struct AverageCellVoltagesC {
    /// Cell 7 Average Voltage Result. Corresponds to `AC7V[15:0]`.
    #[bits(16, default = types::AverageCellVoltage::DEFAULT)]  pub ac7v: types::AverageCellVoltage,
    /// Cell 8 Average Voltage Result. Corresponds to `AC8V[15:0]`.
    #[bits(16, default = types::AverageCellVoltage::DEFAULT)]  pub ac8v: types::AverageCellVoltage,
    /// Cell 9 Average Voltage Result. Corresponds to `AC9V[15:0]`.
    #[bits(16, default = types::AverageCellVoltage::DEFAULT)]  pub ac9v: types::AverageCellVoltage,
    #[bits(16, default = 0)]                                   _padding: u16,
}

/// Avergage Cell Voltage Register Group D (ACD). Contains six 1-byte registers (so 6 bytes total).
/// 
/// Contains cells 10-12.
/// 
/// See Table 66 on page 63 of the datasheet.
#[register_group(
    bytes = 6,
    write = None,
    read = commands::avg_cell_voltage::rdacd().frame(),
)]
#[bitfield(u64)]
pub struct AverageCellVoltagesD {
    /// Cell 10 Average Voltage Result. Corresponds to `AC10V[15:0]`.
    #[bits(16, default = types::AverageCellVoltage::DEFAULT)]  pub ac10v: types::AverageCellVoltage,
    /// Cell 11 Average Voltage Result. Corresponds to `AC11V[15:0]`.
    #[bits(16, default = types::AverageCellVoltage::DEFAULT)]  pub ac11v: types::AverageCellVoltage,
    /// Cell 12 Average Voltage Result. Corresponds to `AC12V[15:0]`.
    #[bits(16, default = types::AverageCellVoltage::DEFAULT)]  pub ac12v: types::AverageCellVoltage,
    #[bits(16, default = 0)]                                   _padding: u16,
}

/// Avergage Cell Voltage Register Group E (ACE). Contains six 1-byte registers (so 6 bytes total).
/// 
/// Contains cells 13-15.
/// 
/// See Table 67 on page 63 of the datasheet.
#[register_group(
    bytes = 6,
    write = None,
    read = commands::avg_cell_voltage::rdace().frame(),
)]
#[bitfield(u64)]
pub struct AverageCellVoltagesE {
    /// Cell 13 Average Voltage Result. Corresponds to `AC13V[15:0]`.
    #[bits(16, default = types::AverageCellVoltage::DEFAULT)]  pub ac13v: types::AverageCellVoltage,
    /// Cell 14 Average Voltage Result. Corresponds to `AC14V[15:0]`.
    #[bits(16, default = types::AverageCellVoltage::DEFAULT)]  pub ac14v: types::AverageCellVoltage,
    /// Cell 15 Average Voltage Result. Corresponds to `AC15V[15:0]`.
    #[bits(16, default = types::AverageCellVoltage::DEFAULT)]  pub ac15v: types::AverageCellVoltage,
    #[bits(16, default = 0)]                                   _padding: u16,
}

/// Average Cell Voltage Register Group F (ACF). Contains six 1-byte registers (so 6 bytes total).
/// 
/// Just contains cell 16.
/// 
/// See Table 68 on page 63 of the datasheet.
#[register_group(
    bytes = 6,
    write = None,
    read = commands::avg_cell_voltage::rdacf().frame(),
)]
#[bitfield(u64)]
pub struct AverageCellVoltagesF {
    /// Cell 16 Average Voltage Result. Corresponds to `AC16V[15:0]`.
    #[bits(16, default = types::AverageCellVoltage::DEFAULT)]  pub ac16v: types::AverageCellVoltage,
    #[bits(32, default = u32::MAX)]                            _reserved: u32,
    #[bits(16, default = 0)]                                   _padding: u16,
}

/// All average cell voltage results (Average Cell Voltage Register Group A through F).
#[register_group_aggregate(
    read = commands::avg_cell_voltage::rdacall().frame(),
)]
#[derive(Clone, Copy, Debug)]
pub struct AverageCellVoltagesAll {
    /// Cells 1–3 (Average Cell Voltage Register Group A).
    pub a: AverageCellVoltagesA,
    /// Cells 4–6 (Average Cell Voltage Register Group B).
    pub b: AverageCellVoltagesB,
    /// Cells 7–9 (Average Cell Voltage Register Group C).
    pub c: AverageCellVoltagesC,
    /// Cells 10–12 (Average Cell Voltage Register Group D).
    pub d: AverageCellVoltagesD,
    /// Cells 13–15 (Average Cell Voltage Register Group E).
    pub e: AverageCellVoltagesE,
    /// Cell 16 (Average Cell Voltage Register Group F).
    /// 
    /// There's only 1 cell in Average Cell Voltage Register Group F, so this type has to be `types::AverageCellVoltage` instead of `AverageCellVoltagesF`.
    /// Otherwise the serialization would get messed up. Sad! This is bothering me but I may have OCD.
    pub f: types::AverageCellVoltage,
}

/// Filtered Cell Voltage Register Group A (FCA). Contains six 1-byte registers (so 6 bytes total).
/// 
/// Contains cells 1-3.
/// 
/// See Table 69 on page 63 of the datasheet.
#[register_group(
    bytes = 6,
    write = None,
    read = commands::filtered_cell_voltage::rdfca().frame(),
)]
#[bitfield(u64)]
pub struct FilteredCellVoltagesA {
    /// Cell 1 Filtered Voltage Result. Corresponds to `FC1V[15:0]`.
    #[bits(16, default = types::FilteredCellVoltage::DEFAULT)]  pub fc1v: types::FilteredCellVoltage,
    /// Cell 2 Filtered Voltage Result. Corresponds to `FC2V[15:0]`.
    #[bits(16, default = types::FilteredCellVoltage::DEFAULT)]  pub fc2v: types::FilteredCellVoltage,
    /// Cell 3 Filtered Voltage Result. Corresponds to `FC3V[15:0]`.
    #[bits(16, default = types::FilteredCellVoltage::DEFAULT)]  pub fc3v: types::FilteredCellVoltage,
    #[bits(16, default = 0)]                                    _padding: u16,
}

/// Filtered Cell Voltage Register Group B (FCB). Contains six 1-byte registers (so 6 bytes total).
/// 
/// Contains cells 4-6.
/// 
/// See Table 70 on page 64 of the datasheet.
#[register_group(
    bytes = 6,
    write = None,
    read = commands::filtered_cell_voltage::rdfcb().frame(),
)]
#[bitfield(u64)]
pub struct FilteredCellVoltagesB {
    /// Cell 4 Filtered Voltage Result. Corresponds to `FC4V[15:0]`.
    #[bits(16, default = types::FilteredCellVoltage::DEFAULT)]  pub fc4v: types::FilteredCellVoltage,
    /// Cell 5 Filtered Voltage Result. Corresponds to `FC5V[15:0]`.
    #[bits(16, default = types::FilteredCellVoltage::DEFAULT)]  pub fc5v: types::FilteredCellVoltage,
    /// Cell 6 Filtered Voltage Result. Corresponds to `FC6V[15:0]`.
    #[bits(16, default = types::FilteredCellVoltage::DEFAULT)]  pub fc6v: types::FilteredCellVoltage,
    #[bits(16, default = 0)]                                    _padding: u16,
}

/// Filtered Cell Voltage Register Group C (FCC). Contains six 1-byte registers (so 6 bytes total).
/// 
/// Contains cells 7-9.
/// 
/// See Table 71 on page 64 of the datasheet.
#[register_group(
    bytes = 6,
    write = None,
    read = commands::filtered_cell_voltage::rdfcc().frame(),
)]
#[bitfield(u64)]
pub struct FilteredCellVoltagesC {
    /// Cell 7 Filtered Voltage Result. Corresponds to `FC7V[15:0]`.
    #[bits(16, default = types::FilteredCellVoltage::DEFAULT)]  pub fc7v: types::FilteredCellVoltage,
    /// Cell 8 Filtered Voltage Result. Corresponds to `FC8V[15:0]`.
    #[bits(16, default = types::FilteredCellVoltage::DEFAULT)]  pub fc8v: types::FilteredCellVoltage,
    /// Cell 9 Filtered Voltage Result. Corresponds to `FC9V[15:0]`.
    #[bits(16, default = types::FilteredCellVoltage::DEFAULT)]  pub fc9v: types::FilteredCellVoltage,
    #[bits(16, default = 0)]                                    _padding: u16,
}

/// Filtered Cell Voltage Register Group D (FCD). Contains six 1-byte registers (so 6 bytes total).
/// 
/// Contains cells 10-12.
/// 
/// See Table 72 on page 64 of the datasheet.
#[register_group(
    bytes = 6,
    write = None,
    read = commands::filtered_cell_voltage::rdfcd().frame(),
)]
#[bitfield(u64)]
pub struct FilteredCellVoltagesD {
    /// Cell 10 Filtered Voltage Result. Corresponds to `FC10V[15:0]`.
    #[bits(16, default = types::FilteredCellVoltage::DEFAULT)]  pub fc10v: types::FilteredCellVoltage,
    /// Cell 11 Filtered Voltage Result. Corresponds to `FC11V[15:0]`.
    #[bits(16, default = types::FilteredCellVoltage::DEFAULT)]  pub fc11v: types::FilteredCellVoltage,
    /// Cell 12 Filtered Voltage Result. Corresponds to `FC12V[15:0]`.
    #[bits(16, default = types::FilteredCellVoltage::DEFAULT)]  pub fc12v: types::FilteredCellVoltage,
    #[bits(16, default = 0)]                                    _padding: u16,
}

/// Filtered Cell Voltage Register Group E (FCE). Contains six 1-byte registers (so 6 bytes total).
/// 
/// Contains cells 13-15.
/// 
/// See Table 73 on page 64 of the datasheet.
#[register_group(
    bytes = 6,
    write = None,
    read = commands::filtered_cell_voltage::rdfce().frame(),
)]
#[bitfield(u64)]
pub struct FilteredCellVoltagesE {
    /// Cell 13 Filtered Voltage Result. Corresponds to `FC13V[15:0]`.
    #[bits(16, default = types::FilteredCellVoltage::DEFAULT)]  pub fc13v: types::FilteredCellVoltage,
    /// Cell 14 Filtered Voltage Result. Corresponds to `FC14V[15:0]`.
    #[bits(16, default = types::FilteredCellVoltage::DEFAULT)]  pub fc14v: types::FilteredCellVoltage,
    /// Cell 15 Filtered Voltage Result. Corresponds to `FC15V[15:0]`.
    #[bits(16, default = types::FilteredCellVoltage::DEFAULT)]  pub fc15v: types::FilteredCellVoltage,
    #[bits(16, default = 0)]                                    _padding: u16,
}

/// Filtered Cell Voltage Register Group F (FCF). Contains six 1-byte registers (so 6 bytes total).
/// 
/// Just contains cell 16.
/// 
/// See Table 74 on page 64 of the datasheet.
#[register_group(
    bytes = 6,
    write = None,
    read = commands::filtered_cell_voltage::rdfcf().frame(),
)]
#[bitfield(u64)]
pub struct FilteredCellVoltagesF {
    /// Cell 16 Filtered Voltage Result. Corresponds to `FC16V[15:0]`.
    #[bits(16, default = types::FilteredCellVoltage::DEFAULT)]  pub fc16v: types::FilteredCellVoltage,
    #[bits(32, default = u32::MAX)]                     _reserved: u32,
    #[bits(16, default = 0)]                            _padding: u16,
}

/// All filtered cell voltage results (Filtered Cell Voltage Register Group A through F).
#[register_group_aggregate(
    read = commands::filtered_cell_voltage::rdfcall().frame(),
)]
#[derive(Clone, Copy, Debug)]
pub struct FilteredCellVoltagesAll {
    /// Cells 1–3 (Filtered Cell Voltage Register Group A).
    pub a: FilteredCellVoltagesA,
    /// Cells 4–6 (Filtered Cell Voltage Register Group B).
    pub b: FilteredCellVoltagesB,
    /// Cells 7–9 (Filtered Cell Voltage Register Group C).
    pub c: FilteredCellVoltagesC,
    /// Cells 10–12 (Filtered Cell Voltage Register Group D).
    pub d: FilteredCellVoltagesD,
    /// Cells 13–15 (Filtered Cell Voltage Register Group E).
    pub e: FilteredCellVoltagesE,
    /// Cell 16 (Filtered Cell Voltage Register Group F).
    /// 
    /// There's only 1 cell in Filtered Cell Voltage Register Group F, so this type has to be `types::FilteredCellVoltage` instead of `FilteredCellVoltagesF`.
    /// Otherwise the serialization would get messed up. Sad! This is bothering me but I may have OCD.
    pub f: types::FilteredCellVoltage,
}

/// S-Voltage Register Group A (SCA). Contains six 1-byte registers (so 6 bytes total).
/// 
/// Contains S pins 1-3.
/// 
/// See Table 75 on page 65 of the datasheet.
#[register_group(
    bytes = 6,
    write = None,
    read = commands::s_voltage::rdsva().frame(),
)]
#[bitfield(u64)]
pub struct SVoltagesA {
    /// Cell 1 S-Voltage Result. Corresponds to `S1V[15:0]`.
    #[bits(16, default = types::SVoltage::DEFAULT)]  pub s1v: types::SVoltage,
    /// Cell 2 S-Voltage Result. Corresponds to `S2V[15:0]`.
    #[bits(16, default = types::SVoltage::DEFAULT)]  pub s2v: types::SVoltage,
    /// Cell 3 S-Voltage Result. Corresponds to `S3V[15:0]`.
    #[bits(16, default = types::SVoltage::DEFAULT)]  pub s3v: types::SVoltage,
    #[bits(16, default = 0)]                                   _padding: u16,
}

/// S-Voltage Register Group B (SCB). Contains six 1-byte registers (so 6 bytes total).
/// 
/// Contains S pins 4-6.
/// 
/// See Table 76 on page 65 of the datasheet.
#[register_group(
    bytes = 6,
    write = None,
    read = commands::s_voltage::rdsvb().frame(),
)]
#[bitfield(u64)]
pub struct SVoltagesB {
    /// Cell 4 S-Voltage Result. Corresponds to `S4V[15:0]`.
    #[bits(16, default = types::SVoltage::DEFAULT)]  pub s4v: types::SVoltage,
    /// Cell 5 S-Voltage Result. Corresponds to `S5V[15:0]`.
    #[bits(16, default = types::SVoltage::DEFAULT)]  pub s5v: types::SVoltage,
    /// Cell 6 S-Voltage Result. Corresponds to `S6V[15:0]`.
    #[bits(16, default = types::SVoltage::DEFAULT)]  pub s6v: types::SVoltage,
    #[bits(16, default = 0)]                                   _padding: u16,
}

/// S-Voltage Register Group C (SCC). Contains six 1-byte registers (so 6 bytes total).
/// 
/// Contains S pins 7-9.
/// 
/// See Table 77 on page 65 of the datasheet.
#[register_group(
    bytes = 6,
    write = None,
    read = commands::s_voltage::rdsvc().frame(),
)]
#[bitfield(u64)]
pub struct SVoltagesC {
    /// Cell 7 S-Voltage Result. Corresponds to `S7V[15:0]`.
    #[bits(16, default = types::SVoltage::DEFAULT)]  pub s7v: types::SVoltage,
    /// Cell 8 S-Voltage Result. Corresponds to `S8V[15:0]`.
    #[bits(16, default = types::SVoltage::DEFAULT)]  pub s8v: types::SVoltage,
    /// Cell 9 S-Voltage Result. Corresponds to `S9V[15:0]`.
    #[bits(16, default = types::SVoltage::DEFAULT)]  pub s9v: types::SVoltage,
    #[bits(16, default = 0)]                                   _padding: u16,
}

/// S-Voltage Register Group D (SCD). Contains six 1-byte registers (so 6 bytes total).
/// 
/// Contains S pins 10-12.
/// 
/// See Table 78 on page 65 of the datasheet.
#[register_group(
    bytes = 6,
    write = None,
    read = commands::s_voltage::rdsvd().frame(),
)]
#[bitfield(u64)]
pub struct SVoltagesD {
    /// Cell 10 S-Voltage Result. Corresponds to `S10V[15:0]`.
    #[bits(16, default = types::SVoltage::DEFAULT)]  pub s10v: types::SVoltage,
    /// Cell 11 S-Voltage Result. Corresponds to `S11V[15:0]`.
    #[bits(16, default = types::SVoltage::DEFAULT)]  pub s11v: types::SVoltage,
    /// Cell 12 S-Voltage Result. Corresponds to `S12V[15:0]`.
    #[bits(16, default = types::SVoltage::DEFAULT)]  pub s12v: types::SVoltage,
    #[bits(16, default = 0)]                                   _padding: u16,
}

/// S-Voltage Register Group E (SCE). Contains six 1-byte registers (so 6 bytes total).
/// 
/// Contains S pins 13-15.
/// 
/// See Table 79 on page 65 of the datasheet.
#[register_group(
    bytes = 6,
    write = None,
    read = commands::s_voltage::rdsve().frame(),
)]
#[bitfield(u64)]
pub struct SVoltagesE {
    /// Cell 13 S-Voltage Result. Corresponds to `S13V[15:0]`.
    #[bits(16, default = types::SVoltage::DEFAULT)]  pub s13v: types::SVoltage,
    /// Cell 14 S-Voltage Result. Corresponds to `S14V[15:0]`.
    #[bits(16, default = types::SVoltage::DEFAULT)]  pub s14v: types::SVoltage,
    /// Cell 15 S-Voltage Result. Corresponds to `S15V[15:0]`.
    #[bits(16, default = types::SVoltage::DEFAULT)]  pub s15v: types::SVoltage,
    #[bits(16, default = 0)]                                   _padding: u16,
}

/// S-Voltage Register Group F (FCF). Contains six 1-byte registers (so 6 bytes total).
/// 
/// Just S pin 16.
/// 
/// See Table 80 on page 66 of the datasheet.
#[register_group(
    bytes = 6,
    write = None,
    read = commands::s_voltage::rdsvf().frame(),
)]
#[bitfield(u64)]
pub struct SVoltagesF {
    /// Cell 16 S-Voltage Result. Corresponds to `S16V[15:0]`.
    #[bits(16, default = types::SVoltage::DEFAULT)]  pub s16v: types::SVoltage,
    #[bits(32, default = u32::MAX)]                  _reserved: u32,
    #[bits(16, default = 0)]                         _padding: u16,
}

/// All S-voltage results (S-Voltage Register Group A through F).
#[register_group_aggregate(
    read = commands::s_voltage::rdsall().frame(),
)]
#[derive(Clone, Copy, Debug)]
pub struct SVoltagesAll {
    /// Cells/Pins 1–3 (S-Voltage Register Group A).
    pub a: SVoltagesA,
    /// Cells/Pins 4–6 (S-Voltage Register Group B).
    pub b: SVoltagesB,
    /// Cells/Pins 7–9 (S-Voltage Register Group C).
    pub c: SVoltagesC,
    /// Cells/Pins 10–12 (S-Voltage Register Group D).
    pub d: SVoltagesD,
    /// Cells 13–15 (S-Voltage Register Group E).
    pub e: SVoltagesE,
    /// Cell/Pin 16 (S-Voltage Register Group F).
    /// 
    /// There's only 1 cell in S-Voltage Register Group F, so this type has to be `types::SVoltage` instead of `SVoltagesF`.
    /// Otherwise the serialization would get messed up. Sad! This is bothering me but I may have OCD.
    pub f: types::SVoltage,
}

/// Auxillary Register Group A (AUXA). Contains six 1-byte registers (so 6 bytes total).
/// 
/// Contains GPIOs 1-3.
/// 
/// See Table 81 on page 66 of the datasheet.
#[register_group(
    bytes = 6,
    write = None,
    read = commands::aux::rdauxa().frame(),
)]
#[bitfield(u64)]
pub struct AuxillaryA {
    /// GPIO 1 Voltage Result. Corresponds to `G1V[15:0]`.
    #[bits(16, default = types::GpioVoltage::DEFAULT)]  pub g1v: types::GpioVoltage,
    /// GPIO 2 Voltage Result. Corresponds to `G2V[15:0]`.
    #[bits(16, default = types::GpioVoltage::DEFAULT)]  pub g2v: types::GpioVoltage,
    /// GPIO 3 Voltage Result. Corresponds to `G3V[15:0]`.
    #[bits(16, default = types::GpioVoltage::DEFAULT)]  pub g3v: types::GpioVoltage,
    #[bits(16, default = 0)]                            _padding: u16,
}

/// Auxillary Register Group B (AUXB). Contains six 1-byte registers (so 6 bytes total).
/// 
/// Contains GPIOs 4-6.
/// 
/// See Table 82 on page 66 of the datasheet.
#[register_group(
    bytes = 6,
    write = None,
    read = commands::aux::rdauxb().frame(),
)]
#[bitfield(u64)]
pub struct AuxillaryB {
    /// GPIO 4 Voltage Result. Corresponds to `G4V[15:0]`.
    #[bits(16, default = types::GpioVoltage::DEFAULT)]  pub g4v: types::GpioVoltage,
    /// GPIO 5 Voltage Result. Corresponds to `G5V[15:0]`.
    #[bits(16, default = types::GpioVoltage::DEFAULT)]  pub g5v: types::GpioVoltage,
    /// GPIO 6 Voltage Result. Corresponds to `G6V[15:0]`.
    #[bits(16, default = types::GpioVoltage::DEFAULT)]  pub g6v: types::GpioVoltage,
    #[bits(16, default = 0)]                            _padding: u16,
}

/// Auxillary Register Group C (AUXC). Contains six 1-byte registers (so 6 bytes total).
/// 
/// Contains GPIOs 7-9.
/// 
/// See Table 83 on page 66 of the datasheet.
#[register_group(
    bytes = 6,
    write = None,
    read = commands::aux::rdauxc().frame(),
)]
#[bitfield(u64)]
pub struct AuxillaryC {
    /// GPIO 7 Voltage Result. Corresponds to `G7V[15:0]`.
    #[bits(16, default = types::GpioVoltage::DEFAULT)]  pub g7v: types::GpioVoltage,
    /// GPIO 8 Voltage Result. Corresponds to `G8V[15:0]`.
    #[bits(16, default = types::GpioVoltage::DEFAULT)]  pub g8v: types::GpioVoltage,
    /// GPIO 9 Voltage Result. Corresponds to `G9V[15:0]`.
    #[bits(16, default = types::GpioVoltage::DEFAULT)]  pub g9v: types::GpioVoltage,
    #[bits(16, default = 0)]                            _padding: u16,
}

/// Auxillary Register Group D (AUXD). Contains six 1-byte registers (so 6 bytes total).
/// 
/// Contains GPIO 10, and VMV/VPV.
/// 
/// See Table 84 on page 66 of the datasheet.
#[register_group(
    bytes = 6,
    write = None,
    read = commands::aux::rdauxd().frame(),
)]
#[bitfield(u64)]
pub struct AuxillaryD {
    /// GPIO 10 Voltage Result. Corresponds to `G10V[15:0]`.
    #[bits(16, default = types::GpioVoltage::DEFAULT)]    pub g10v: types::GpioVoltage,
    /// VMV Voltage Result. Corresponds to `VMV[15:0]`.
    #[bits(16, default = types::VMinusVoltage::DEFAULT)]  pub vmv: types::VMinusVoltage,
    /// VPV Voltage Result. Corresponds to `VPV[15:0]`.
    #[bits(16, default = types::VPlusVoltage::DEFAULT)]   pub vpv: types::VPlusVoltage,
    #[bits(16, default = 0)]                              _padding: u16,
}