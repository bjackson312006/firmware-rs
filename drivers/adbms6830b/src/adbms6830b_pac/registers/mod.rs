//! Register structure declarations based on the ADBMS6830B datasheet.

#![allow(dead_code)]

pub mod config_a;

/// Defines the read/write type of a register.
pub enum RegisterKind {
    /// Register can only be read from.
    ReadOnly,
    /// Register can be read from and written to.
    ReadWrite,
    /// get it
    WriteOnly,
}

/// A register inside the ADBMS6830.
pub trait Register {
    /// Gets the type of register (i.e., whether it is read-only, read/write, etc.)
    fn kind(&self) -> RegisterKind;
}