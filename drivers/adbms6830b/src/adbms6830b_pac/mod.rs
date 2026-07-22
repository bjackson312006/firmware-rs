//! This is a "PAC" for the ADBMS6830B (it isn't really a PAC since ADBMS6830B is a I2C device, but it's close enough). Basically, this crate provides the types that map to the
//! chip's registers and commands, basically taken straight from the datasheet and encoded into Rust. The full ADBMS6830B driver can use this interface to interact with registers
//! and such without doing any manual bit fiddling.

pub mod commands;
pub mod pec;
pub mod registers;
pub mod types;