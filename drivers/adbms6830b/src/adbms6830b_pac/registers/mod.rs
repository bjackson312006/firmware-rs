//! Register structure declarations based on the ADBMS6830B datasheet.

#![allow(dead_code)]
#![allow(rustdoc::broken_intra_doc_links)]

use adbms6830b_macros::{register_group, register_group_aggregate};

/// Defines whether a register group can be written to, or is read-only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegisterKind {
    /// The register group can only be read from.
    ReadOnly,
    /// The register group can only be written to (not a joke).
    WriteOnly,
    /// The register group can be read from and written to.
    ReadWrite,
}

/// Common interface implemented by every register group.
/// 
/// This interface is implemented by the "internal" `register_group` proc macro.
pub trait RegisterGroup {
    /// Whether this register group is read-only or read/write.
    const KIND: RegisterKind;
}

/// Module for internal shared helpers for the register types laid out in Table 107 on page 72 of the datasheet.
pub(in crate::adbms6830b_pac) mod table107 {
    use bitfield_struct::bitfield;

    /// Internal type for the registers from the first row of Table 107.
    /// Aka all of these registers:
    /// `CxV`, `SxV`, `ACxV`, `FCxV`, `GxV`, `R_GxV`, `VREF2`, `VD`, `VA`, `VRES`, `VMV`
    #[bitfield(u16)]
    #[derive(PartialEq, Eq)]
    pub(in crate::adbms6830b_pac) struct FirstRowRegisters { #[bits(16, default = 0x8000)] inner: u16 }
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

    /// This macro implements a newtype around a `FirstRowRegisters`. You can put docs in the macro
    /// to inject custom docs for the generated struct.
    macro_rules! impl_firstrowregister {
        (
            $(#[$outer:meta])*
            $structname:ident
        ) => {
            $(#[$outer])*
            #[derive(Debug, Copy, Clone, Eq, PartialEq)]
            pub struct $structname { inner: $crate::adbms6830b_pac::registers::table107::FirstRowRegisters }
            impl $structname {
                pub const DEFAULT: $structname = Self { inner: $crate::adbms6830b_pac::registers::table107::FirstRowRegisters::new() };

                // this stuff is needed for the "read all" command and the associated serialization via `#[register_group_aggregate]`:
                /// The number of protocol bytes this result value occupies.
                pub const BYTES: usize = $crate::adbms6830b_pac::registers::table107::FirstRowRegisters::BYTES;
                /// Serializes into protocol bytes (little-endian) since this is the datasheet register order.
                pub const fn to_bytes(self) -> [u8; 2] { self.inner.to_bytes() }
                /// Reconstructs from protocl bytes (little-endian).
                pub const fn from_bytes(bytes: [u8; 2]) -> Self { Self { inner: $crate::adbms6830b_pac::registers::table107::FirstRowRegisters::from_bytes(bytes) } }
                /// Convert from bits.
                pub const fn from_bits(bits: u16) -> Self { Self { inner: $crate::adbms6830b_pac::registers::table107::FirstRowRegisters::from_bits(bits) } }
                /// Convert into bits.
                pub const fn into_bits(self) -> u16 { self.inner.into_bits() }

                /// Min voltage the register can represent. This is around -3,415,200 uV, or around -3.4152 V.
                pub const MIN_MICROVOLTS: i32 = $crate::adbms6830b_pac::registers::table107::FirstRowRegisters::MIN_MICROVOLTS;

                /// Max voltage the register can represent in microvolts.
                /// This is around +6,415,050 uV / +6.41505 V.
                pub const MAX_MICROVOLTS: i32 = $crate::adbms6830b_pac::registers::table107::FirstRowRegisters::MAX_MICROVOLTS;

                #[doc = concat!("Converts a `", stringify!($structname), "` to microvolts.")]
                pub const fn as_microvolts(&self) -> i32 { self.inner.as_microvolts() }

                #[doc = concat!("Creates a new `", stringify!($structname), "` from an input value in uV.\n")]
                /// ### Parameters
                /// - `microvolts`: Voltage, in uV. May be negative.
                ///
                /// This function will return `None` if your input is outside the `MIN_MICROVOLTS`
                /// to `MAX_MICROVOLTS` range.
                pub const fn from_microvolts(microvolts: i32) -> Option<Self> {
                    match $crate::adbms6830b_pac::registers::table107::FirstRowRegisters::from_microvolts(microvolts) {
                        Some(inner) => Some(Self { inner }),
                        None => None,
                    }
                }
            }
        }
    }
    pub(in crate::adbms6830b_pac) use impl_firstrowregister;

    /// Private type for the `VPV` register type from Table 107 (row 4).
    /// 
    /// This type doesn't need to exist probably since this logic is only used by one public type but I like the
    /// consistency of this pattern so why not
    #[bitfield(u16)]
    #[derive(Eq, PartialEq)]
    pub(in crate::adbms6830b_pac) struct VpvInner { #[bits(16, default = 0x8000)] inner: u16 }
    impl VpvInner {
        /// Microvolts per VPV code increment. This is `25 × 150 µV` because of the VPV gain factor.
        pub const VPV_LSB_MICROVOLTS: i32 = 3_750;
        /// The offset of the VPV measurement formula (`25 × +1.5 V` = +37.5 V).
        pub const VPV_OFFSET_MICROVOLTS: i32 = 37_500_000;

        /// Max voltage VPV can represent in microvolts.
        /// This is around +160,376,250 uV / +160.37625 V (raw code 0x7FFF).
        pub const VPV_MAX_MICROVOLTS: i32 = 160_376_250;

        /// Min voltage VPV can represent. This is around -85,380,000 uV, or around -85.38 V (raw code 0x8000).
        pub const VPV_MIN_MICROVOLTS: i32 = -85_380_000;

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
            pub struct $structname { inner: $crate::adbms6830b_pac::registers::table107::VpvInner }
            impl $structname {
                pub const DEFAULT: $structname = Self { inner: $crate::adbms6830b_pac::registers::table107::VpvInner::new() };

                // this stuff is usually needed for the "read all" command and the associated serialization via `#[register_group_aggregate]`, but
                // this type doesn't even need this since the register groups that use it don't have a "read all" command. Still gonna keep this stuff here though just in case.
                /// The number of protocol bytes this result value occupies.
                pub const BYTES: usize = 2;
                /// Serializes into protocol bytes (little-endian) since this is the datasheet register order.
                pub const fn to_bytes(self) -> [u8; 2] { self.inner.to_bytes() }
                /// Reconstructs from protocl bytes (little-endian).
                pub const fn from_bytes(bytes: [u8; 2]) -> Self { Self { inner: $crate::adbms6830b_pac::registers::table107::VpvInner::from_bytes(bytes) } }
                /// Convert from bits.
                pub const fn from_bits(bits: u16) -> Self { Self { inner: $crate::adbms6830b_pac::registers::table107::VpvInner::from_bits(bits) } }
                /// Convert into bits.
                pub const fn into_bits(self) -> u16 { self.inner.into_bits() }

                /// Min voltage the register can represent. This is around -85,380,000 uV, or around -85.38 V.
                pub const VPV_MIN_MICROVOLTS: i32 = $crate::adbms6830b_pac::registers::table107::VpvInner::VPV_MIN_MICROVOLTS;

                /// Max voltage the register can represent in microvolts.
                /// This is around +160,376,250 uV / +160.37625 V.
                pub const VPV_MAX_MICROVOLTS: i32 = $crate::adbms6830b_pac::registers::table107::VpvInner::VPV_MAX_MICROVOLTS;

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
                    match $crate::adbms6830b_pac::registers::table107::VpvInner::from_microvolts(microvolts) {
                        Some(inner) => Some(Self { inner: inner }),
                        None => None,
                    }
                }
            }
        }
    }
    pub(in crate::adbms6830b_pac) use impl_vpvinner;
}

pub mod config_a;
pub mod config_b;
pub mod results;