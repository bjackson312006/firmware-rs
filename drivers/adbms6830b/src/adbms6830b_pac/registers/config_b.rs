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
    /// Cell undervoltage threshold = VUV x 16 x 150uV x 1.5V
    #[bitfield(u16)]
    pub struct UvComparisonVoltage {
        /// Cell undervoltage threshold = VUV x 16 x 150uV x 1.5V. Corresponds to `VUV[11:0]`.
        #[bits(11, default = 0x800)]     pub vuv: u16,
        #[bits(5, default = 0)]         _reserved: u8,
    }
}