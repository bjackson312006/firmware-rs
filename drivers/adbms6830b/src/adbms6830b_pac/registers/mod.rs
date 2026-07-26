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

pub mod config_a;
pub mod config_b;
pub mod results;