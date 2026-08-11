//! Driver for the ADBMS6830B battery monitor.
//!
//! See the `spi` module (specifically the `Line` struct) for the driver itself, and the `chip`
//! module for the register/command types it speaks in.
//!
//! ### Defmt Support
//! - The `defmt` feature flag will implement `defmt::Format` for every public type this driver exposes. This makes
//! it so you can log register groups, field values, errors, etc. directly. To enable, add this
//! to your Cargo.toml:
//! ```toml
//! adbms6830b = { path = "...", features = ["defmt"] }
//! ```

#![no_std]

pub mod chip;
pub mod spi;
mod docs;