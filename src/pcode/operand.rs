//! P-Code operand types and decoding.
//!
//! Operands are the arguments to P-Code instructions. They are decoded
//! according to format specifiers embedded in the opcode table:
//!
//! | Specifier | Meaning | Bytes Consumed |
//! |-----------|---------|----------------|
//! | `%1` | 1-byte unsigned literal | 1 |
//! | `%2` | 2-byte (Int16) literal | 2 |
//! | `%4` | 4-byte (Int32) literal | 4 |
//! | `%a` | Stack variable reference (signed Int16 EBP offset) | 2 |
//! | `%s` | Constant pool index (unsigned Int16) | 2 |
//! | `%l` | Jump target (unsigned Int16 from function start) | 2 |
//! | `%c` | Control/import index (unsigned Int16) | 2 |
//! | `%v` | VTable reference (two Int16 values) | 4 |
//! | `%x` | External call (two Int16 values) | 4 |
//! | `%N` / `%D` | Named arguments to the payload's end (names / DISPIDs) | rest |
//! | `%L` | Jump table to the payload's end (u16 targets) | rest |
//! | `%F` | Frame slots to the payload's end (i16 offsets from ebp) | rest |

use crate::{
    error::Error,
    util::{read_i16_le, read_i32_le, read_u16_le},
};

/// A decoded operand from a P-Code instruction.
///
/// Each variant corresponds to one of the format specifiers in the opcode table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operand {
    /// `%1`: 1-byte unsigned literal value.
    Byte(u8),
    /// `%2`: 2-byte signed integer literal.
    Int16(i16),
    /// `%4`: 4-byte signed integer literal.
    Int32(i32),
    /// `%a`: Stack variable reference (signed 16-bit offset from EBP).
    ///
    /// Negative values are local variables (e.g., `-0x90` = `var_90`),
    /// positive values are function arguments.
    StackVar(i16),
    /// `%s` / `%c`: Constant pool index (unsigned 16-bit).
    ///
    /// The procedure's constant pool is an array of 4-byte entries the runtime
    /// reads at `pool + 4 * index` (`[ebp-0x54]` in the interpreter frame): a
    /// string, a GUID, an object or class descriptor, or the address of a
    /// procedure or a global another module holds. `%s` and `%c` encode the
    /// same index; the table spells it `%c` where the entry is an address.
    ConstPoolIndex(u16),
    /// `%l`: Jump target (unsigned 16-bit offset from function start).
    JumpTarget(u16),
    /// `%v`: A vtable call (`VCallHresult`): the byte offset of the method in
    /// the receiver's vtable, and the constant pool index of the interface's
    /// IID, which the runtime reports a failed call against.
    VTableRef {
        /// Byte offset of the method in the receiver's vtable.
        offset: u16,
        /// Constant pool index of the interface's IID.
        interface: u16,
    },
    /// `%x`: A call through the constant pool (`ImpAdCall*`): the index of the
    /// entry holding the procedure's address, and the bytes of arguments it
    /// takes, which the runtime checks the callee released.
    ExternalCall {
        /// Constant pool index of the procedure's address.
        import: u16,
        /// Bytes of arguments the call passes.
        arg_bytes: u16,
    },
    /// Variable-length byte list (for `FFreeVar`, `FFreeStr`, `FFreeAd`, etc.).
    ///
    /// The `byte_count` gives the number of payload bytes. The payload
    /// typically consists of `byte_count / 2` stack variable references.
    VariableLength {
        /// Number of payload bytes following the size field.
        byte_count: u16,
    },
    /// `%N` / `%D`: the named arguments a named late-bound call ends with,
    /// running to the end of its payload - `count` entries from stream offset
    /// `at`, each the constant pool index of an argument's name (`%N`, two
    /// bytes) or its DISPID (`%D`, four bytes).
    NamedArgs {
        /// Stream offset of the first entry.
        at: u16,
        /// Number of named arguments.
        count: u16,
        /// What each entry is.
        kind: NamedArgKind,
    },
    /// `%F`: the frame slots `FFreeVar` / `FFreeStr` / `FFreeAd` release,
    /// running to the end of their payload - `count` i16 offsets from ebp,
    /// from stream offset `at` ([`Instruction::frame_slots`](super::decoder::Instruction::frame_slots)
    /// reads them).
    FrameList {
        /// Stream offset of the first offset.
        at: u16,
        /// Number of frame slots.
        count: u16,
    },
    /// `%L`: the jump table of `On ... GoTo` / `On ... GoSub`, running to the
    /// end of its payload - `count` u16 jump targets from stream offset `at`.
    JumpTable {
        /// Stream offset of the first target.
        at: u16,
        /// Number of targets.
        count: u16,
    },
}

/// What a named-argument list ([`Operand::NamedArgs`]) names each argument by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamedArgKind {
    /// The constant pool index of the argument's name (`LateMem*`).
    Name,
    /// The argument's DISPID (`LateId*`).
    DispId,
}

impl NamedArgKind {
    /// Returns the size of one entry in bytes.
    #[must_use]
    pub const fn entry_len(self) -> usize {
        match self {
            Self::Name => 2,
            Self::DispId => 4,
        }
    }
}

/// Decodes operands from the instruction stream according to the format string.
///
/// Reads operand bytes starting at `stream[pos]` and advances `pos`
/// past the consumed bytes.
///
/// # Arguments
///
/// * `format` - The operand format string from the opcode table (e.g., `"%a"`, `"%s %2"`).
/// * `stream` - The raw byte stream (the entire P-Code procedure).
/// * `pos` - Current position in the stream. Advanced past consumed bytes on return.
/// * `limit` - Maximum valid position in the stream.
///
/// # Returns
///
/// An array of up to 4 decoded operands. Unused slots are `None`.
///
/// # Errors
///
/// - [`Error::UnexpectedEndOfPCode`] if the stream is too short for the operands.
/// - [`Error::ArithmeticOverflow`] if `pos` advancement would overflow `usize`.
pub fn decode_operands(
    format: &str,
    stream: &[u8],
    pos: &mut usize,
    limit: usize,
) -> Result<[Option<Operand>; 4], Error> {
    let mut operands = [None; 4];
    let mut op_idx = 0usize;
    let mut iter = format.bytes();

    while op_idx < operands.len() {
        let Some(b) = iter.next() else { break };
        if b != b'%' {
            continue;
        }
        let Some(spec) = iter.next() else { break };
        let operand =
            match spec {
                b'1' => {
                    ensure_bytes(stream, *pos, 1, limit)?;
                    let val = stream
                        .get(*pos)
                        .copied()
                        .ok_or(Error::UnexpectedEndOfPCode {
                            offset: *pos,
                            needed: 1,
                        })?;
                    advance(pos, 1)?;
                    Operand::Byte(val)
                }
                b'2' => {
                    ensure_bytes(stream, *pos, 2, limit)?;
                    let val = read_i16_le(stream, *pos)?;
                    advance(pos, 2)?;
                    Operand::Int16(val)
                }
                b'4' => {
                    ensure_bytes(stream, *pos, 4, limit)?;
                    let val = read_i32_le(stream, *pos)?;
                    advance(pos, 4)?;
                    Operand::Int32(val)
                }
                b'a' => {
                    ensure_bytes(stream, *pos, 2, limit)?;
                    let val = read_i16_le(stream, *pos)?;
                    advance(pos, 2)?;
                    Operand::StackVar(val)
                }
                b's' => {
                    ensure_bytes(stream, *pos, 2, limit)?;
                    let val = read_u16_le(stream, *pos)?;
                    advance(pos, 2)?;
                    Operand::ConstPoolIndex(val)
                }
                b'l' => {
                    ensure_bytes(stream, *pos, 2, limit)?;
                    let val = read_u16_le(stream, *pos)?;
                    advance(pos, 2)?;
                    Operand::JumpTarget(val)
                }
                b'c' => {
                    ensure_bytes(stream, *pos, 2, limit)?;
                    let val = read_u16_le(stream, *pos)?;
                    advance(pos, 2)?;
                    Operand::ConstPoolIndex(val)
                }
                b'v' => {
                    ensure_bytes(stream, *pos, 4, limit)?;
                    let offset = read_u16_le(stream, *pos)?;
                    let interface_pos = pos.checked_add(2).ok_or(Error::ArithmeticOverflow {
                        context: "operand %v interface offset",
                    })?;
                    let interface = read_u16_le(stream, interface_pos)?;
                    advance(pos, 4)?;
                    Operand::VTableRef { offset, interface }
                }
                b'x' => {
                    ensure_bytes(stream, *pos, 4, limit)?;
                    let import = read_u16_le(stream, *pos)?;
                    let arg_pos = pos.checked_add(2).ok_or(Error::ArithmeticOverflow {
                        context: "operand %x arg_bytes offset",
                    })?;
                    let arg_bytes = read_u16_le(stream, arg_pos)?;
                    advance(pos, 4)?;
                    Operand::ExternalCall { import, arg_bytes }
                }
                // A named-argument list runs to the end of the payload it is in.
                b'N' | b'D' => {
                    let kind = if spec == b'N' {
                        NamedArgKind::Name
                    } else {
                        NamedArgKind::DispId
                    };
                    let remaining = limit.saturating_sub(*pos);
                    let entry_len = kind.entry_len();
                    let partial = remaining.checked_rem(entry_len).unwrap_or(0);
                    if partial != 0 {
                        return Err(Error::UnexpectedEndOfPCode {
                            offset: *pos,
                            needed: entry_len.saturating_sub(partial),
                        });
                    }
                    let at = u16::try_from(*pos).map_err(|_| Error::ArithmeticOverflow {
                        context: "operand named-argument list offset",
                    })?;
                    let count = u16::try_from(remaining.checked_div(entry_len).unwrap_or(0))
                        .map_err(|_| Error::ArithmeticOverflow {
                            context: "operand named-argument count",
                        })?;
                    advance(pos, remaining)?;
                    Operand::NamedArgs { at, count, kind }
                }
                // A jump table, or a list of frame slots, runs to the end of
                // the payload it is in.
                b'L' | b'F' => {
                    let remaining = limit.saturating_sub(*pos);
                    if !remaining.is_multiple_of(2) {
                        return Err(Error::UnexpectedEndOfPCode {
                            offset: *pos,
                            needed: 1,
                        });
                    }
                    let at = u16::try_from(*pos).map_err(|_| Error::ArithmeticOverflow {
                        context: "operand jump table offset",
                    })?;
                    let count = u16::try_from(remaining.div_euclid(2)).map_err(|_| {
                        Error::ArithmeticOverflow {
                            context: "operand jump table count",
                        }
                    })?;
                    advance(pos, remaining)?;
                    if spec == b'L' {
                        Operand::JumpTable { at, count }
                    } else {
                        Operand::FrameList { at, count }
                    }
                }
                // Unknown specifiers consume 0 bytes.
                _ => continue,
            };
        if let Some(slot) = operands.get_mut(op_idx) {
            *slot = Some(operand);
        }
        op_idx = op_idx.checked_add(1).ok_or(Error::ArithmeticOverflow {
            context: "operand index",
        })?;
    }

    Ok(operands)
}

/// Advances `*pos` by `delta`, returning [`Error::ArithmeticOverflow`] on wrap.
#[inline]
fn advance(pos: &mut usize, delta: usize) -> Result<(), Error> {
    *pos = pos.checked_add(delta).ok_or(Error::ArithmeticOverflow {
        context: "operand pos advance",
    })?;
    Ok(())
}

/// Ensures that at least `needed` bytes are available at `pos` within `limit`.
fn ensure_bytes(stream: &[u8], pos: usize, needed: usize, limit: usize) -> Result<(), Error> {
    let available = limit
        .saturating_sub(pos)
        .min(stream.len().saturating_sub(pos));
    if available < needed {
        return Err(Error::UnexpectedEndOfPCode {
            offset: pos,
            needed,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_byte_operand() {
        let stream = [0x42];
        let mut pos = 0;
        let ops = decode_operands("%1", &stream, &mut pos, stream.len()).unwrap();
        assert_eq!(ops[0], Some(Operand::Byte(0x42)));
        assert_eq!(ops[1], None);
        assert_eq!(pos, 1);
    }

    #[test]
    fn test_decode_int16_operand() {
        let stream = [0x34, 0x12];
        let mut pos = 0;
        let ops = decode_operands("%2", &stream, &mut pos, stream.len()).unwrap();
        assert_eq!(ops[0], Some(Operand::Int16(0x1234)));
        assert_eq!(pos, 2);
    }

    #[test]
    fn test_decode_int32_operand() {
        let stream = [0x78, 0x56, 0x34, 0x12];
        let mut pos = 0;
        let ops = decode_operands("%4", &stream, &mut pos, stream.len()).unwrap();
        assert_eq!(ops[0], Some(Operand::Int32(0x12345678)));
        assert_eq!(pos, 4);
    }

    #[test]
    fn test_decode_stack_var() {
        // -0x90 = 0xFF70 as i16
        let stream = [0x70, 0xFF];
        let mut pos = 0;
        let ops = decode_operands("%a", &stream, &mut pos, stream.len()).unwrap();
        assert_eq!(ops[0], Some(Operand::StackVar(-144)));
        assert_eq!(pos, 2);
    }

    #[test]
    fn test_decode_const_pool_index() {
        let stream = [0x10, 0x00];
        let mut pos = 0;
        let ops = decode_operands("%s", &stream, &mut pos, stream.len()).unwrap();
        assert_eq!(ops[0], Some(Operand::ConstPoolIndex(0x0010)));
        assert_eq!(pos, 2);
    }

    #[test]
    fn test_decode_jump_target() {
        let stream = [0x20, 0x00];
        let mut pos = 0;
        let ops = decode_operands("%l", &stream, &mut pos, stream.len()).unwrap();
        assert_eq!(ops[0], Some(Operand::JumpTarget(0x0020)));
        assert_eq!(pos, 2);
    }

    #[test]
    fn test_decode_import_pool_index() {
        let stream = [0x05, 0x00];
        let mut pos = 0;
        let ops = decode_operands("%c", &stream, &mut pos, stream.len()).unwrap();
        assert_eq!(ops[0], Some(Operand::ConstPoolIndex(5)));
        assert_eq!(pos, 2);
    }

    #[test]
    fn test_decode_vtable_ref() {
        let stream = [0x10, 0x00, 0x03, 0x00];
        let mut pos = 0;
        let ops = decode_operands("%v", &stream, &mut pos, stream.len()).unwrap();
        assert_eq!(
            ops[0],
            Some(Operand::VTableRef {
                offset: 0x10,
                interface: 0x03,
            })
        );
        assert_eq!(pos, 4);
    }

    #[test]
    fn test_decode_external_call() {
        let stream = [0x02, 0x00, 0x04, 0x00];
        let mut pos = 0;
        let ops = decode_operands("%x", &stream, &mut pos, stream.len()).unwrap();
        assert_eq!(
            ops[0],
            Some(Operand::ExternalCall {
                import: 2,
                arg_bytes: 4,
            })
        );
        assert_eq!(pos, 4);
    }

    #[test]
    fn test_decode_multiple_operands() {
        // LitVarI2: %a %2
        let stream = [0x70, 0xFF, 0x05, 0x00];
        let mut pos = 0;
        let ops = decode_operands("%a %2", &stream, &mut pos, stream.len()).unwrap();
        assert_eq!(ops[0], Some(Operand::StackVar(-144)));
        assert_eq!(ops[1], Some(Operand::Int16(5)));
        assert_eq!(ops[2], None);
        assert_eq!(pos, 4);
    }

    #[test]
    fn test_decode_empty_format() {
        let stream = [0x00];
        let mut pos = 0;
        let ops = decode_operands("", &stream, &mut pos, stream.len()).unwrap();
        assert_eq!(ops[0], None);
        assert_eq!(pos, 0);
    }

    #[test]
    fn test_decode_end_of_procedure_marker() {
        let stream = [];
        let mut pos = 0;
        let ops = decode_operands("%}", &stream, &mut pos, stream.len()).unwrap();
        assert_eq!(ops[0], None); // an unknown specifier consumes no bytes
        assert_eq!(pos, 0);
    }

    #[test]
    fn test_decode_truncated_stream() {
        let stream = [0x01]; // Only 1 byte, but %2 needs 2
        let mut pos = 0;
        assert!(matches!(
            decode_operands("%2", &stream, &mut pos, stream.len()),
            Err(Error::UnexpectedEndOfPCode { .. })
        ));
    }

    #[test]
    fn test_decode_truncated_at_limit() {
        let stream = [0x01, 0x02, 0x03, 0x04];
        let mut pos = 0;
        // Limit is 1, so only 1 byte available even though stream has 4
        assert!(matches!(
            decode_operands("%2", &stream, &mut pos, 1),
            Err(Error::UnexpectedEndOfPCode { .. })
        ));
    }

    #[test]
    fn test_decode_max_4_operands() {
        let stream = [0x01, 0x02, 0x03, 0x04, 0x05];
        let mut pos = 0;
        let ops = decode_operands("%1 %1 %1 %1 %1", &stream, &mut pos, stream.len()).unwrap();
        // Only 4 operands can be stored
        assert!(ops[0].is_some());
        assert!(ops[1].is_some());
        assert!(ops[2].is_some());
        assert!(ops[3].is_some());
        assert_eq!(pos, 4); // Only consumed 4 bytes (stopped at 4th operand)
    }
}
