//! Status Register A!!!!!!!

use bitfield_struct::{bitfield, bitenum};
use adbms6830b_macros::BitfieldEnumDefault;

use super::{register_group, register_group_aggregate};
use super::super::commands;

/// Field types relavent to Status Register A. See Table 105 on page 71 of the datasheet.
pub mod types {
    use crate::chip::registers::table107::{impl_firstrowregister, impl_itmpinner};

    impl_firstrowregister!(
        /// Second reference voltage (VREF2). See Table 105 on page 71 of the datasheet.
        /// 
        /// 16-bit ADC measurement value for second reference voltage for second reference = VREF2 × 150 μV +1.5 V.
        /// Normal range is within 2.988 V to 3.012 V considering data sheet limits, thermal hysteresis, and long-term drift.
        /// Reset to 0x8000 after power-up, sleep, or clear command (CLRAUX)
        Vref2,
        0x8000
    );

    impl_itmpinner!(
        /// Internal die temperature (ITMP). See Table 105 on page 72 of the datasheet.
        /// 
        /// 16-bit ADC measurement value of Internal Die temperature. Temperature measurement voltage = (ITMP × 150
        /// μV + 1.5 V)/7.5 mV/°C – 273°C. Reset to 0x7FFF after power-up, sleep, and to 0x8000 after clear command
        /// (CLRAUX).
        Itmp,
        0x7FFF
    );
}