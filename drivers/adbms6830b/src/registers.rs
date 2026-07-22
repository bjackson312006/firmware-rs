//! Low-level, generated register/command layer for the ADBMS6830B.
//!
//! This module contains a single [`device_driver::create_device!`] invocation
//! that generates a PAC-like typed layer for the chip: one accessor per
//! register group, typed getters/setters for every field, and generated enums
//! for multi-bit selectors. It is intentionally transport-agnostic — the
//! isoSPI daisy-chain framing, command PEC (CRC15), data PEC (CRC10), command
//! counter, wake pulses and ADC polling all live in the hand-written interface
//! that implements [`device_driver::RegisterInterface`] /
//! [`device_driver::CommandInterface`] (or their async variants).
//!
//! # Addressing model
//!
//! `device_driver` gives each register a *single* address, but the ADBMS uses
//! **different command codes for reading and writing the same register group**
//! (e.g. `RDCFGA = 0x002`, `WRCFGA = 0x001`). The convention used here is:
//!
//! * `ADDRESS` = the **read** command code (`RDxxx`).
//! * The interface's `write_register` maps the read code to the matching write
//!   code. For the writable groups that mapping is simply:
//!
//! | Register | `ADDRESS` (RD) | Write code (WR) |
//! |----------|----------------|-----------------|
//! | `CfgA`   | `0x002`        | `0x001`         |
//! | `CfgB`   | `0x026`        | `0x024`         |
//! | `PwmA`   | `0x022`        | `0x020`         |
//! | `PwmB`   | `0x023`        | `0x021`         |
//! | `Comm`   | `0x722`        | `0x721`         |
//! | `Retention` | `0x072`     | `0x071` (`WRRR`)|
//!
//! # Layout / byte + bit order
//!
//! Each group is 6 payload bytes (`R0..R5`), transmitted `R0` first, and `R0`
//! holds the least-significant bits — so the fieldsets use little-endian byte
//! order with LSB0 bit order. Byte `n` occupies bits `[8n .. 8n+8)`. The
//! retention register is the exception (`RRR0` holds the *high* bits) and
//! overrides the byte order to big-endian.
//!
//! # Omitted groups
//!
//! The Averaged Cell (`RDACx`), Filtered Cell (`RDFCx`), Redundant Aux
//! (`RDRAXx`) and LPCM (`RDCMxxx`) groups are omitted here for brevity: they
//! are byte-for-byte identical in shape to the groups that are defined
//! (three `int` results, or a config-style fieldset) and can be added the same
//! way — or generated with a `ref`/`repeat` if you don't need distinct field
//! names. 
