//! Mapping for the Command Codes seen in Table 50 of the datasheet (page 57).

use bitfield_struct::bitfield;

/// CC[10:0] - Command Code. 11-bit field.
/// 
/// See Table 50 on page 57 of the datasheet.
#[bitfield(u16)]
struct CommandCode {
    /// CC[10:0]
    #[bits(11)]
    pub code: u16,
    /// Reserved bits, since this is only an 11-bit field.
    #[bits(5)]
    _reserved: u8,
}

/// ADBMS6830B Command. See Table 50 on page 57 of the datasheet.
pub(crate) struct Command {
    /// Whether or not the command counter increments for the command. Corresponds to the INC value from Table 50 in the datasheet.
    inc: bool,
    /// The 11-bit CC[10:0] field for the command.
    code: CommandCode,
}

impl Command {
    /// Allows you to define a command. Ideally should be called in a const context.
    /// ## Prammies
    /// - `inc`: Whether or not the command counter increments for the command. Corresponds to the INC value from Table 50 in the datasheet.
    /// - `code`: The 11-bit CC[10:0] field for the command.
    const fn define(inc: bool, code: u16) -> Self {
        Self {
            inc,
            code: CommandCode(code),
        }
    }
}

/// Write Configuration Register Group A
pub(crate) const WRCFGA: Command = Command::define(true, 0b00000000001);