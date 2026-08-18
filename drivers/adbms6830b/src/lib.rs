//! Driver for the ADBMS6830B battery monitor.
//!
//! - `chip`: the register and command types the device speaks in.
//! - `spi`: `Line`, the transport for one isoSPI line of daisy-chained devices.
//! - `Api`: `Api`, two lines plus the routing and command counter state tracked across them.
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
pub mod manager;
pub mod line;
pub mod service;
mod docs;
