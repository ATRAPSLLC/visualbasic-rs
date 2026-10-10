//! P-Code instruction decoder and streaming iterator.
//!
//! The [`InstructionIterator`] yields decoded [`Instruction`]s from a
//! P-Code byte stream. It handles all three instruction categories:
//!
//! 1. **Primary opcodes** (1 byte): Direct index into the primary dispatch table.
//! 2. **Extended opcodes** (2 bytes): Lead byte (`0xFB`-`0xFF`) followed by
//!    the actual opcode byte, indexed into the corresponding extended table.
//! 3. **Variable-length opcodes** (size == -1): A `u16` byte count follows the
//!    opcode, then that many bytes of payload data.

use std::fmt;

use crate::{
    error::Error,
    pcode::{
        calltarget::CallSignature,
        movement::Movement,
        opcode::{self, OpcodeInfo, PrLoad, StackRule},
        operand::{self, Operand},
        semantics::{OpcodeSemantics, PCodeDataType},
        stackeffect::{Pop, PrSource, Push, StackEffect},
    },
    util::{read_i16_le, read_u16_le},
};

/// Maximum sentinel raw length when an instruction's byte span exceeds `u8::MAX`.
///
/// VB6 instructions are at most a few dozen bytes, but variable-length
/// payloads could in principle exceed 255. We saturate the [`Instruction::raw_len`]
/// field rather than panic; the iterator's `pos` is the source of truth for stream
/// progress, so this only affects the public-facing length field.
const RAW_LEN_SATURATION: u8 = u8::MAX;

/// A single decoded P-Code instruction.
///
/// Contains the opcode metadata, decoded operands, and positional information
/// within the P-Code stream.
#[derive(Debug, Clone)]
pub struct Instruction {
    /// Byte offset of this instruction within the P-Code stream
    /// (relative to the start of the procedure's P-Code).
    pub offset: u16,
    /// Total raw byte length of this instruction in the stream.
    ///
    /// For primary opcodes: 1 (opcode) + operand bytes.
    /// For extended opcodes: 2 (lead byte + opcode) + operand bytes.
    /// For variable-length: opcode bytes + 2 (size field) + payload bytes.
    pub raw_len: u8,
    /// Static reference to the opcode's metadata (mnemonic, size, format).
    pub info: &'static OpcodeInfo,
    /// Decoded operands (up to 4). Unused slots are `None`.
    pub operands: [Option<Operand>; 4],
}

impl Instruction {
    /// Returns the type the opcode imprints on its evaluation-stack result, if any.
    ///
    /// Mirrors the `data_type` field on the parent [`OpcodeInfo`] - for
    /// example `LitI4` returns `Some(PCodeDataType::I4)`, `FStR8` returns
    /// `Some(PCodeDataType::R8)`, control-flow / Nop / Stack opcodes return
    /// `None`. Build-time-resolved from the opcode's mnemonic suffix; no
    /// runtime string parsing.
    ///
    /// This is the type-level signal consumers should prefer over
    /// pattern-matching on mnemonic strings (e.g., `mnemonic.ends_with("I4")`),
    /// which is fragile across renamings and does not generalize across
    /// the six dispatch tables.
    #[inline]
    pub fn data_type(&self) -> Option<PCodeDataType> {
        self.info.data_type
    }

    /// Returns the VB type the instruction fixes
    /// ([`OpcodeInfo::value_type`]): an operation's operand type, a
    /// conversion's target, an x87 move's float type; `None` for a move that
    /// names only a width, whose type comes from where its value goes.
    #[inline]
    pub fn value_type(&self) -> Option<PCodeDataType> {
        self.info.value_type
    }

    /// Returns the opcode's [`data_type`](Self::data_type) for an operand
    /// slot that holds an operand.
    ///
    /// The opcode tables type the instruction, not each operand: this is
    /// the instruction's type for every operand present. Returns `None` for
    /// an out-of-range `index`, an empty slot, and an opcode with no
    /// [`OpcodeInfo::data_type`].
    #[inline]
    pub fn operand_type(&self, index: usize) -> Option<PCodeDataType> {
        // Validate the slot exists and carries an operand.
        let _ = self.operands.get(index)?.as_ref()?;
        self.info.data_type
    }

    /// Returns `true` if this instruction is a beginning-of-statement marker.
    ///
    /// Convenience for [`OpcodeInfo::is_bos`]. BOS markers (`LargeBos`) delimit
    /// source statements; see [`bos_distance`](Self::bos_distance).
    #[inline]
    pub fn is_bos(&self) -> bool {
        self.info.is_bos()
    }

    /// Returns the byte distance from this BOS marker to the next one.
    ///
    /// `LargeBos`'s 1-byte operand is the number of bytes to the next statement
    /// boundary (the next BOS marker, or the statement's terminating branch);
    /// `0` marks the last statement in the procedure. Returns `None` for
    /// non-BOS instructions.
    #[inline]
    pub fn bos_distance(&self) -> Option<u8> {
        if !self.is_bos() {
            return None;
        }
        match self.operands.first() {
            Some(Some(Operand::Byte(d))) => Some(*d),
            _ => None,
        }
    }

    /// Returns the code offsets this instruction can transfer control to.
    ///
    /// Covers `%l` operands - except the sentinels of `OnErrorGoto` (`0`,
    /// `0xFFFF`, `0xFFFE`) and `Resume` (`0xFFFF`, `0xFFFE`), which are no
    /// offsets (see
    /// [`error_flow`](Self::error_flow)) - and the entries of an `On ... GoTo`
    /// jump table, read from `code`, the procedure's P-Code the instruction
    /// was decoded from.
    pub fn jump_targets<'a>(&'a self, code: &'a [u8]) -> impl Iterator<Item = u16> + 'a {
        let semantics = self.info.semantics;
        let sentinel = move |target: u16| match semantics {
            OpcodeSemantics::OnError => target == 0 || target >= 0xFFFE,
            OpcodeSemantics::Resume => target >= 0xFFFE,
            _ => false,
        };
        self.operands.iter().flatten().flat_map(
            move |operand| -> Box<dyn Iterator<Item = u16> + 'a> {
                match *operand {
                    Operand::JumpTarget(t) if !sentinel(t) => Box::new(std::iter::once(t)),
                    Operand::JumpTable { at, count } => Box::new(
                        (0..usize::from(count))
                            .map(move |i| usize::from(at).saturating_add(i.saturating_mul(2)))
                            .filter_map(move |pos| read_u16_le(code, pos).ok()),
                    ),
                    _ => Box::new(std::iter::empty()),
                }
            },
        )
    }

    /// Returns what the instruction does to the evaluation stack, the x87
    /// stack and Pr.
    ///
    /// The fixed part comes from the opcode table
    /// ([`OpcodeInfo::popped`]); the operand-dependent part
    /// ([`OpcodeInfo::stack`]) from the operands: 4-slot Variants for a
    /// late-bound call's arguments, one slot per array index, two per
    /// `ReDim` dimension. A call's arguments come from `call`, the
    /// [`CallSignature`] of its callee:
    ///
    /// - `VCall*` / `ThisVCall*` ([`StackRule::Callee`]): the callee's
    ///   argument widths, else its slot count ([`Pop::Slots`]), else
    ///   [`Pop::Unknown`];
    /// - `ImpAdCall*` ([`StackRule::ArgBytes`]): the operand's byte count,
    ///   exact whatever the callee, split into the callee's argument widths
    ///   when they add up to it.
    ///
    /// A call that leaves the x87 stack to its callee
    /// ([`OpcodeInfo::fpu_callee`]) pushes an x87 value when
    /// [`CallSignature::float_result`] is `Some(true)`, nothing when it is
    /// `Some(false)`, and [`Push::MaybeX87`] without a signature.
    ///
    /// A `GoSub` pushes the return position (one slot) that its `Return`
    /// pops, so inside a `GoSub` body the stack is one slot deeper than at
    /// the `GoSub`.
    pub fn stack_effect(&self, call: Option<&CallSignature<'_>>) -> StackEffect {
        let info = self.info;
        let rule: Vec<Pop> = match info.stack {
            StackRule::Fixed => Vec::new(),
            StackRule::Callee => match call {
                Some(CallSignature {
                    arg_widths: Some(widths),
                    ..
                }) => widths.iter().map(|&w| Pop::Eval(w)).collect(),
                Some(CallSignature {
                    arg_slots: Some(slots),
                    ..
                }) => vec![Pop::Slots(*slots)],
                _ => vec![Pop::Unknown],
            },
            StackRule::ArgBytes { operand } => match self.operands.get(usize::from(operand)) {
                Some(Some(Operand::ExternalCall { arg_bytes, .. })) => {
                    let slots = arg_bytes / 4;
                    match call.and_then(|c| c.arg_widths.as_ref()) {
                        Some(widths)
                            if widths.iter().map(|&w| u16::from(w)).sum::<u16>() == slots =>
                        {
                            widths.iter().map(|&w| Pop::Eval(w)).collect()
                        }
                        _ => vec![Pop::Slots(slots)],
                    }
                }
                _ => vec![Pop::Unknown],
            },
            StackRule::Variants { operand } => self.repeat(operand, 1, Pop::Eval(4)),
            StackRule::Count { operand } => self.repeat(operand, 1, Pop::Eval(1)),
            StackRule::Pairs { operand } => self.repeat(operand, 2, Pop::Eval(1)),
            StackRule::PairsAtLeastOne { operand } => match self.count_operand(operand) {
                Some(n) => vec![Pop::Eval(1); usize::from(n.max(1)).saturating_mul(2)],
                None => vec![Pop::Unknown],
            },
            StackRule::Bytes { operand } => match self.count_operand(operand) {
                Some(bytes) => vec![Pop::Slots(bytes.saturating_sub(4) / 4)],
                None => vec![Pop::Unknown],
            },
        };
        let at = usize::from(info.rule_at).min(info.popped.len());
        let (before, after) = info.popped.split_at(at);
        let popped = before
            .iter()
            .copied()
            .chain(rule)
            .chain(after.iter().copied())
            .collect();
        let pushed = if info.pushes > 0 {
            Some(Push::Eval(u8::try_from(info.pushes).unwrap_or(0)))
        } else if info.fpu_push > 0 {
            Some(Push::X87)
        } else if info.fpu_callee {
            match call.and_then(|c| c.float_result) {
                Some(true) => Some(Push::X87),
                Some(false) => None,
                None => Some(Push::MaybeX87),
            }
        } else {
            None
        };
        StackEffect {
            popped,
            pushed,
            receiver: info.receiver,
            pr: self.pr_source(),
        }
    }

    /// Returns where the instruction loads Pr, the object register, from,
    /// with the operands that locate the object; `None` if it does not load
    /// Pr ([`OpcodeInfo::pr_load`]).
    pub fn pr_source(&self) -> Option<PrSource> {
        let operand = |index: usize| self.operands.get(index).copied().flatten();
        let frame = |index: usize| match operand(index) {
            Some(Operand::StackVar(offset)) => Some(offset),
            _ => None,
        };
        let int16 = |index: usize| match operand(index) {
            Some(Operand::Int16(value)) => Some(value),
            _ => None,
        };
        let pool = |index: usize| match operand(index) {
            Some(Operand::ConstPoolIndex(value)) => Some(value),
            _ => None,
        };
        Some(match self.info.pr_load? {
            PrLoad::Frame => PrSource::Frame { offset: frame(0)? },
            PrLoad::Me => PrSource::Me,
            PrLoad::Pool => PrSource::Pool { index: pool(0)? },
            PrLoad::FrameIndirect => PrSource::FrameIndirect { offset: frame(0)? },
            PrLoad::PrMember => PrSource::PrMember {
                offset: int16(0)?.cast_unsigned(),
            },
            PrLoad::FrameMember => PrSource::FrameMember {
                frame: frame(0)?,
                offset: int16(1)?.cast_unsigned(),
            },
            PrLoad::ArrayElement => PrSource::ArrayElement,
            PrLoad::Variant => PrSource::Variant,
            PrLoad::NewIfNull => PrSource::NewIfNull { class: pool(0)? },
            PrLoad::WithMember => PrSource::WithMember { offset: int16(0)? },
            PrLoad::LateGet => PrSource::LateGet {
                dispid: match operand(1) {
                    Some(Operand::Int32(value)) => value,
                    _ => return None,
                },
                object: frame(2)?,
                temp: frame(0)?,
            },
        })
    }

    /// Returns operand `index` as a count: an unsigned `%2` or `%1`.
    fn count_operand(&self, index: u8) -> Option<u16> {
        match self.operands.get(usize::from(index))? {
            Some(Operand::Int16(n)) => u16::try_from(*n).ok(),
            Some(Operand::Byte(n)) => Some(u16::from(*n)),
            _ => None,
        }
    }

    /// `per` copies of `pop` for each unit of count operand `index`.
    fn repeat(&self, index: u8, per: usize, pop: Pop) -> Vec<Pop> {
        match self.count_operand(index) {
            Some(n) => vec![pop; usize::from(n).saturating_mul(per)],
            None => vec![Pop::Unknown],
        }
    }

    /// Returns what a load, store or literal instruction moves, with its
    /// operands resolved ([`Movement::of`]); `None` for other instructions.
    pub fn movement(&self) -> Option<Movement> {
        Movement::of(self)
    }

    /// Returns every frame slot (offset from ebp) the instruction names, in
    /// order: its `%a` operands and the slots of an `FFree*` payload
    /// ([`Operand::FrameList`], read from `code`, the procedure's P-Code).
    ///
    /// A `GetRecOwner*` / `PutRecOwner*` payload is an inline record
    /// descriptor the runtime reads (its address is passed to the helper),
    /// not frame slots; no fixture emits those opcodes.
    pub fn frame_slots(&self, code: &[u8]) -> Vec<i16> {
        let mut slots = Vec::new();
        for operand in self.operands.iter().flatten() {
            match *operand {
                Operand::StackVar(offset) => slots.push(offset),
                Operand::FrameList { at, count } => slots.extend(
                    (0..usize::from(count))
                        .map_while(|i| usize::from(at).checked_add(i.checked_mul(2)?))
                        .map_while(|pos| read_i16_le(code, pos).ok()),
                ),
                _ => {}
            }
        }
        slots
    }

    /// Returns every constant pool entry the instruction names, with what
    /// the opcode says the entry holds ([`PoolEntryRole`]): its `%s`, `%c`,
    /// `%v` and `%x` operands and the names of a named-argument list
    /// ([`Operand::NamedArgs`], read from `code`, the procedure's P-Code).
    pub fn pool_references(&self, code: &[u8]) -> Vec<PoolReference> {
        let mnemonic = self.info.mnemonic;
        let mut specs = self
            .info
            .operand_format
            .split('%')
            .skip(1)
            .filter_map(|spec| spec.bytes().next());
        let mut references = Vec::new();
        for operand in self.operands.iter().flatten() {
            let spec = specs.next();
            match *operand {
                Operand::ConstPoolIndex(index) => references.push(PoolReference {
                    index,
                    role: PoolEntryRole::of_operand(mnemonic, spec.unwrap_or(b'c')),
                }),
                Operand::VTableRef { interface, .. } => references.push(PoolReference {
                    index: interface,
                    role: PoolEntryRole::Guid,
                }),
                Operand::ExternalCall { import, .. } => references.push(PoolReference {
                    index: import,
                    role: PoolEntryRole::Address,
                }),
                Operand::NamedArgs {
                    at,
                    count,
                    kind: operand::NamedArgKind::Name,
                } => references.extend(
                    (0..usize::from(count))
                        .map_while(|i| usize::from(at).checked_add(i.checked_mul(2)?))
                        .map_while(|pos| read_u16_le(code, pos).ok())
                        .map(|index| PoolReference {
                            index,
                            role: PoolEntryRole::MemberName,
                        }),
                ),
                _ => {}
            }
        }
        references
    }

    /// Returns how an `ExitProc*` instruction returns its procedure's result
    /// ([`ProcedureReturn`]); `None` for any other instruction.
    pub fn procedure_return(&self) -> Option<ProcedureReturn> {
        /// The interpreter's slots end here; the locals start below.
        const TOP: i16 = -0x84;
        // The copy (0x6610640a) aligns its source down to an even address.
        let even = |offset: i16| offset & !1;
        let operand = |index: usize| match self.operands.get(index).copied().flatten() {
            Some(Operand::Int16(value) | Operand::StackVar(value)) => Some(value),
            _ => None,
        };
        // A width code: 1, 2, 4, else 8 bytes.
        let width = |code: i16| match code {
            1 => 1u8,
            2 => 2,
            4 => 4,
            _ => 8,
        };
        let register = |from: i16, bytes: u8| ProcedureReturn::Register {
            from,
            bytes,
            signed: false,
        };
        Some(match self.info.mnemonic {
            "ExitProc" | "ExitProcStr" => register(-0x88, 4),
            "ExitProcI2" => ProcedureReturn::Register {
                from: -0x86,
                bytes: 2,
                signed: true,
            },
            "ExitProcUI1" => register(-0x86, 1),
            "ExitProcCy" => register(-0x8C, 8),
            "ExitProcR4" => ProcedureReturn::X87 {
                from: -0x88,
                bytes: 4,
            },
            "ExitProcR8" => ProcedureReturn::X87 {
                from: -0x8C,
                bytes: 8,
            },
            "ExitProcHresult" => ProcedureReturn::Hresult,
            "ExitProcCbStack" => {
                let bytes = width(operand(0)?);
                register(TOP.saturating_sub(i16::from(bytes)), bytes)
            }
            "ExitProcFrameCbStack" => register(operand(0)?, width(operand(1)?)),
            "ExitProcCbHresult" => {
                let bytes = operand(1)?;
                ProcedureReturn::CopyToRetval {
                    from: even(TOP.saturating_sub(bytes)),
                    bytes: bytes.cast_unsigned(),
                    retval_arg: operand(0)?,
                }
            }
            "ExitProcFrameCbHresult" => ProcedureReturn::CopyToRetval {
                from: even(operand(0)?),
                bytes: operand(2)?.cast_unsigned(),
                retval_arg: operand(1)?,
            },
            "ExitProcCb" => {
                let bytes = operand(0)?;
                ProcedureReturn::CopyToHidden {
                    from: even(TOP.saturating_sub(bytes)),
                    bytes: bytes.cast_unsigned(),
                }
            }
            "ExitProcFrameCb" => ProcedureReturn::CopyToHidden {
                from: even(operand(0)?),
                bytes: operand(1)?.cast_unsigned(),
            },
            _ => return None,
        })
    }

    /// Classifies a `Resume` / `OnErrorGoto` instruction's operand into the
    /// error-flow construct it encodes.
    ///
    /// The operand is a P-Code offset (a label) or a sentinel. Read from the
    /// handlers (6.00.8176): `OnErrorGoto` (0x66105e46) first zeroes `Erl`
    /// (runtime context + 0x98) and resets the error object at context +
    /// 0x78 (0x66003b2f), then sets the handler `[ebp-0x40]` to
    /// the label, to -1 for `0xFFFF` (`On Error Resume Next`) or to 0 for
    /// `0xFFFE` (`On Error GoTo 0`); operand `0` instead clears the
    /// statement an active handler is handling (`[ebp-0x3C]`) and keeps the
    /// handler. `Resume` takes a label, `0xFFFF` (`Resume Next`) or `0xFFFE`
    /// (`Resume`).
    ///
    /// Returns `None` for any other opcode. Prefer this over reading the raw
    /// [`Operand::JumpTarget`], which renders a sentinel as `loc_FFFF`.
    pub fn error_flow(&self) -> Option<ErrorFlow> {
        let target = match self.operands.first() {
            Some(Some(Operand::JumpTarget(v))) => *v,
            _ => return None,
        };
        match self.info.semantics {
            OpcodeSemantics::OnError => Some(match target {
                0xFFFF => ErrorFlow::OnErrorResumeNext,
                0xFFFE => ErrorFlow::OnErrorGotoZero,
                0 => ErrorFlow::OnErrorClearActive,
                label => ErrorFlow::OnErrorGoto(label),
            }),
            OpcodeSemantics::Resume => Some(match target {
                0xFFFF => ErrorFlow::ResumeNext,
                0xFFFE => ErrorFlow::Resume,
                label => ErrorFlow::ResumeLabel(label),
            }),
            _ => None,
        }
    }
}

/// How an `ExitProc*` instruction returns its procedure's result, read from
/// the handlers (6.00.8176). Returned by
/// [`Instruction::procedure_return`].
///
/// The result never comes from the evaluation stack: a function stores it in
/// its frame (a 2-byte one at `ebp-0x86`, a 4-byte one at `ebp-0x88`, an
/// 8-byte one at `ebp-0x8C`, a Variant at `ebp-0x94`: the top of the locals,
/// just below the interpreter's slots that end at `ebp-0x84`), and the
/// `ExitProc*` form says where to take it from and how to hand it back. A
/// `Sub` ends with `ExitProc`, whose `eax` is then unused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcedureReturn {
    /// In `eax` (1, 2 or 4 bytes, extended) or `edx:eax` (8 bytes), loaded
    /// from frame offset `from`.
    Register {
        /// Frame offset (from ebp) of the value.
        from: i16,
        /// Its size in bytes.
        bytes: u8,
        /// `true` if a 2-byte value is sign-extended (`ExitProcI2`).
        signed: bool,
    },
    /// On the x87 stack (`fld`), from frame offset `from`: a Single (4
    /// bytes), a Double or Date (8).
    X87 {
        /// Frame offset (from ebp) of the value.
        from: i16,
        /// Its size in bytes.
        bytes: u8,
    },
    /// `eax` = 0 (`S_OK`) and nothing else: a method with no result, or one
    /// whose result went through its `[out, retval]` pointer before.
    Hresult,
    /// `bytes` copied from frame offset `from` to the `[out, retval]`
    /// pointer the caller passed in argument `retval_arg` (an offset from
    /// ebp); `eax` = 0 (`S_OK`) (`ExitProcCbHresult`,
    /// `ExitProcFrameCbHresult`).
    CopyToRetval {
        /// Frame offset (from ebp) of the value.
        from: i16,
        /// Bytes copied.
        bytes: u16,
        /// Offset from ebp of the argument holding the pointer.
        retval_arg: i16,
    },
    /// `bytes` copied from frame offset `from` to the hidden result pointer
    /// the caller passed as the first argument (`ebp+0xC`), which `eax`
    /// returns (`ExitProcCb`, `ExitProcFrameCb`: a module function returning
    /// a Variant or a record).
    CopyToHidden {
        /// Frame offset (from ebp) of the value.
        from: i16,
        /// Bytes copied.
        bytes: u16,
    },
}

/// Source-level error-handling construct recovered from a `Resume` or
/// `OnErrorGoto` instruction by [`Instruction::error_flow`].
///
/// VB6 encodes the `On Error` and `Resume` forms in one
/// signed `i16` operand; this enum makes the encoding legible (and keeps the
/// disassembler from printing a sentinel as `loc_FFFF`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorFlow {
    /// `On Error GoTo <label>` - installs the handler at the given P-Code offset.
    OnErrorGoto(u16),
    /// `On Error Resume Next` - operand `-1` (`0xFFFF`).
    OnErrorResumeNext,
    /// `On Error GoTo 0` - disables error handling; operand `-2` (`0xFFFE`).
    OnErrorGotoZero,
    /// Operand `0`: resets the error state and the statement an active
    /// handler is handling, keeping the installed handler. No fixture's source compiles
    /// to it.
    OnErrorClearActive,
    /// `Resume <label>` - resumes at the given P-Code offset.
    ResumeLabel(u16),
    /// `Resume Next` - resumes after the faulting statement; operand `-1` (`0xFFFF`).
    ResumeNext,
    /// bare `Resume` - re-executes the faulting statement; operand `-2` (`0xFFFE`).
    Resume,
}

impl fmt::Display for ErrorFlow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OnErrorGoto(label) => write!(f, "On Error GoTo loc_{label:04X}"),
            Self::OnErrorResumeNext => f.write_str("On Error Resume Next"),
            Self::OnErrorGotoZero => f.write_str("On Error GoTo 0"),
            Self::OnErrorClearActive => f.write_str("On Error (clear the active error)"),
            Self::ResumeLabel(label) => write!(f, "Resume loc_{label:04X}"),
            Self::ResumeNext => f.write_str("Resume Next"),
            Self::Resume => f.write_str("Resume"),
        }
    }
}

/// What a constant pool entry holds, as the opcode that names it says
/// (see [`Instruction::pool_references`]).
///
/// Read from the handlers of MSVBVM60 6.00.8176 and the runtime functions
/// they call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PoolEntryRole {
    /// A BSTR string literal (`LitStr`, `LitVarStr`).
    String,
    /// A member name a late-bound call passes to `GetIDsOfNames`: UTF-16,
    /// NUL-terminated, with no length prefix (`LateMem*`, `VarLateMem*`,
    /// and each name of a named-argument list).
    MemberName,
    /// A 16-byte GUID: the IID of a vtable call's interface (the second
    /// half of `%v`), of a cast's or `TypeOf`'s interface (`CastAd`,
    /// `CastAdVar`, `CheckType`, `CheckTypeVar`, which compares a record's
    /// GUID), or of a `For Each` loop variable's (`ForEachCollObj`,
    /// `NextEachCollObj`).
    Guid,
    /// A record's layout
    /// ([`RecordLayout`](crate::vb::controlprop::RecordLayout)):
    /// `AssignRecord`, `CRec*`, `CVar*Udt`, `CDargRefUdt`, `Destruct*`,
    /// `StUdtVar`, `Redim*VarUdt`, and the element layout of `EraseDestruct`,
    /// `EraseDestrKeepData` and `StAryRec*`.
    RecordLayout,
    /// The descriptor `Get #` and `Put #` of a record read
    /// ([`RecordIoDescriptor`](crate::vb::pooldesc::RecordIoDescriptor):
    /// `GetRecOwn*`, `PutRecOwn*`).
    RecordIo,
    /// The item list of a `Print`, `Write` or `Input` statement
    /// ([`IoItems`](crate::vb::pooldesc::IoItems): `PrintObject`,
    /// `PrintFile`, `WriteFile`, `InputFile`).
    IoItems,
    /// The `SAFEARRAY` header of a fixed array inside a record
    /// ([`ArrayDescriptor`](crate::vb::pooldesc::ArrayDescriptor):
    /// `AryInRecLdPr`, `AryInRecLdRf`).
    ArrayDescriptor,
    /// What `New` creates (`New`, `NewIfNull*`): a project class's
    /// `ObjectInfo`, or a
    /// [`CreationDescriptor`](crate::vb::pooldesc::CreationDescriptor).
    Creation,
    /// An address (`%c`), or the procedure or stub an `ImpAdCall*` calls
    /// (`%x`).
    Address,
}

impl PoolEntryRole {
    /// The role of the entry a `%s` or `%c` operand of `mnemonic` names.
    fn of_operand(mnemonic: &str, spec: u8) -> Self {
        match (spec, mnemonic) {
            (b's', "LitStr" | "LitVarStr") => Self::String,
            (
                b's',
                "AssignRecord"
                | "CDargRefUdt"
                | "CRec2Ansi"
                | "CRec2Uni"
                | "CRecAnsi2Uni"
                | "CRecUni2Ansi"
                | "CVarAryUdt"
                | "CVarRefUdt"
                | "CVarUdt"
                | "DestructAnsiOFrame"
                | "DestructOFrame"
                | "DestructRecord"
                | "StUdtVar"
                | "RedimVarUdt"
                | "RedimPreserveVarUdt"
                | "EraseDestruct"
                | "EraseDestrKeepData"
                | "StAryRecCopy"
                | "StAryRecMove",
            ) => Self::RecordLayout,
            (b's', "GetRecOwn3" | "GetRecOwn4" | "PutRecOwn3" | "PutRecOwn4") => Self::RecordIo,
            (b's', "PrintObject" | "PrintFile" | "WriteFile" | "InputFile") => Self::IoItems,
            (b's', "AryInRecLdPr" | "AryInRecLdRf") => Self::ArrayDescriptor,
            (b's', "ForEachCollObj" | "NextEachCollObj")
            | (b'c', "CastAd" | "CastAdVar" | "CheckType" | "CheckTypeVar") => Self::Guid,
            (b'c', "New" | "NewIfNullPr" | "NewIfNullAd" | "NewIfNullRf") => Self::Creation,
            (b's', _) if mnemonic.contains("LateMem") => Self::MemberName,
            _ => Self::Address,
        }
    }
}

/// A constant pool entry an instruction names: see
/// [`Instruction::pool_references`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PoolReference {
    /// The entry's index in the procedure's constant pool.
    pub index: u16,
    /// What the opcode says the entry holds.
    pub role: PoolEntryRole,
}

/// Streaming iterator over P-Code instructions.
///
/// Yields one [`Instruction`] per call to [`next()`](Iterator::next),
/// consuming bytes from the P-Code stream. Returns `None` when the
/// stream is exhausted (position reaches `limit`).
///
/// # Example
///
/// ```ignore
/// let iter = InstructionIterator::new(pcode_bytes, proc_size);
/// for result in iter {
///     let insn = result?;
///     println!("{:04X}  {}", insn.offset, insn.info.mnemonic);
/// }
/// ```
#[must_use = "iterators are lazy and do nothing unless consumed"]
pub struct InstructionIterator<'a> {
    /// The P-Code byte stream for one procedure.
    bytes: &'a [u8],
    /// Current position within `bytes`.
    pos: usize,
    /// Total expected length (from `ProcDscInfo.wProcSize`).
    limit: usize,
    /// The furthest offset a jump decoded so far targets: the code reaches at
    /// least that far, whatever terminators come before it.
    furthest_target: usize,
}

impl<'a> InstructionIterator<'a> {
    /// Creates a new iterator over `pcode_bytes[..proc_size]`.
    ///
    /// # Arguments
    ///
    /// * `pcode_bytes` - The raw P-Code byte stream for one procedure.
    ///   Must be at least `proc_size` bytes long.
    /// * `proc_size` - The procedure size from `ProcDscInfo.wProcSize`.
    ///   The iterator stops at this boundary.
    pub fn new(pcode_bytes: &'a [u8], proc_size: u16) -> Self {
        let limit = (proc_size as usize).min(pcode_bytes.len());
        Self {
            bytes: pcode_bytes,
            pos: 0,
            limit,
            furthest_target: 0,
        }
    }

    /// Returns the current byte position within the P-Code stream.
    #[inline]
    pub fn position(&self) -> usize {
        self.pos
    }

    /// Returns `true` if every byte from `start` to the stream limit is `0x00`.
    ///
    /// VB6 pads each procedure's P-Code stream to a 4-byte boundary with zero
    /// bytes, so `proc_size` can include 1 to 3 trailing pad bytes after the final
    /// terminator. A partial "instruction" made entirely of those pad bytes
    /// (e.g. a lone `0x00`, which would otherwise look like a truncated
    /// `LargeBos`) is not a decode error - it is the end of the real stream.
    fn tail_is_zero_padding(&self, start: usize) -> bool {
        self.bytes
            .get(start..self.limit)
            .is_some_and(|tail| !tail.is_empty() && tail.iter().all(|&b| b == 0))
    }
}

impl Iterator for InstructionIterator<'_> {
    type Item = Result<Instruction, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos >= self.limit {
            return None;
        }

        // VB6 pads each procedure's P-Code to a 4-byte boundary with zero
        // bytes. Once everything remaining is `0x00`, the real instruction
        // stream is over - stop cleanly rather than decoding the padding (a
        // lone trailing `0x00` otherwise looks like a truncated `LargeBos`,
        // and an even pad like `00 00` like a spurious BOS marker). Real code
        // never reaches an all-zero tail: procedures end on a terminator, not
        // on padding.
        if self.tail_is_zero_padding(self.pos) {
            self.pos = self.limit;
            return None;
        }

        let start = self.pos;

        // Read first byte
        let Some(&first_byte) = self.bytes.get(self.pos) else {
            return Some(Err(Error::UnexpectedEndOfPCode {
                offset: self.pos,
                needed: 1,
            }));
        };
        self.pos = match self.pos.checked_add(1) {
            Some(v) => v,
            None => {
                return Some(Err(Error::ArithmeticOverflow {
                    context: "decoder pos advance after first byte",
                }));
            }
        };

        // Determine if this is a lead byte
        let next_byte = if self.pos < self.limit {
            self.bytes.get(self.pos).copied()
        } else {
            None
        };

        let (info, opcode_bytes_consumed) = opcode::lookup(first_byte, next_byte);

        // If it's an extended opcode, consume the second byte
        if opcode_bytes_consumed == 2 {
            if self.pos >= self.limit {
                return Some(Err(Error::UnexpectedEndOfPCode {
                    offset: start,
                    needed: 2,
                }));
            }
            self.pos = match self.pos.checked_add(1) {
                Some(v) => v,
                None => {
                    return Some(Err(Error::ArithmeticOverflow {
                        context: "decoder pos advance after lead byte",
                    }));
                }
            };
        }

        // Now decode the operands
        let operands;

        if info.is_variable_length() {
            // Variable-length instruction: read u16 byte count, then payload
            let after_size = match self.pos.checked_add(2) {
                Some(v) => v,
                None => {
                    return Some(Err(Error::ArithmeticOverflow {
                        context: "decoder variable-length size offset",
                    }));
                }
            };
            if after_size > self.limit {
                let needed = after_size.saturating_sub(start);
                return Some(Err(Error::UnexpectedEndOfPCode {
                    offset: start,
                    needed,
                }));
            }
            let byte_count = match read_u16_le(self.bytes, self.pos) {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            self.pos = after_size;

            // Validate and skip the payload
            let payload_end = match self.pos.checked_add(byte_count as usize) {
                Some(v) => v,
                None => {
                    return Some(Err(Error::ArithmeticOverflow {
                        context: "decoder variable-length payload end",
                    }));
                }
            };
            if payload_end > self.limit {
                return Some(Err(Error::InvalidVariableLengthSize {
                    opcode_name: info.mnemonic,
                    size: byte_count,
                }));
            }
            // A payload with a stated layout (the named late-bound calls) decodes
            // to its operands, and must fill the payload exactly; one without
            // stays an opaque byte list.
            if info.operand_format.is_empty() {
                operands = [
                    Some(Operand::VariableLength { byte_count }),
                    None,
                    None,
                    None,
                ];
            } else {
                let mut payload = self.pos;
                operands = match operand::decode_operands(
                    info.operand_format,
                    self.bytes,
                    &mut payload,
                    payload_end,
                ) {
                    Ok(operands) => operands,
                    Err(e) => return Some(Err(e)),
                };
                if payload != payload_end {
                    return Some(Err(Error::InvalidVariableLengthSize {
                        opcode_name: info.mnemonic,
                        size: byte_count,
                    }));
                }
            }
            self.pos = payload_end;
        } else if info.size > 0 {
            // Fixed-size instruction: decode operands according to format string.
            // The 'size' includes the opcode byte itself (but not the lead byte).
            match operand::decode_operands(
                info.operand_format,
                self.bytes,
                &mut self.pos,
                self.limit,
            ) {
                Ok(ops) => operands = ops,
                Err(e) => return Some(Err(e)),
            }

            // Ensure pos advances to the declared instruction size even when
            // the operand format is empty or incomplete. Many opcodes have
            // size > 1 but no documented operand format specifiers - we still
            // need to skip over their operand bytes to stay aligned.
            let lead_extra = opcode_bytes_consumed.saturating_sub(1);
            let expected_end = start
                .checked_add(lead_extra)
                .and_then(|v| v.checked_add(info.size as usize));
            if let Some(expected_end) = expected_end
                && self.pos < expected_end
                && expected_end <= self.limit
            {
                self.pos = expected_end;
            }
        } else {
            // Unimplemented/invalid opcode (size == 0)
            operands = [None; 4];
        }

        let raw_len = u8::try_from(self.pos.saturating_sub(start)).unwrap_or(RAW_LEN_SATURATION);
        let offset_u16 = u16::try_from(start).unwrap_or(u16::MAX);

        // The compiler pads a procedure's code to a multiple of four bytes,
        // usually with zeros but not always (a `ParamArray` method can end
        // `ExitProcCbHresult` then `00 ff ff`). Fewer than four bytes after a
        // terminator are that padding, unless a jump reaches them; four or
        // more are code, reachable or not (a `Function` with `GoSub`s ends
        // with its `Return`s, then the `ExitProc` of `End Function` that
        // nothing reaches).
        let instruction = Instruction {
            offset: offset_u16,
            raw_len,
            info,
            operands,
        };
        for target in instruction.jump_targets(self.bytes) {
            self.furthest_target = self.furthest_target.max(usize::from(target));
        }
        if info.is_terminator()
            && self.limit.saturating_sub(self.pos) < 4
            && self.pos > self.furthest_target
        {
            self.limit = self.pos;
        }

        Some(Ok(instruction))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pcode::opcode::{DispatchTable, PRIMARY_TABLE};

    /// Collect all instructions from a byte stream, asserting no errors.
    fn decode_all(bytes: &[u8]) -> Vec<Instruction> {
        let iter = InstructionIterator::new(bytes, bytes.len() as u16);
        iter.map(|r| r.expect("decode error")).collect()
    }

    #[test]
    fn test_exit_proc() {
        // 0x14 = ExitProc, size 1 (no operands)
        let insns = decode_all(&[0x14]);
        assert_eq!(insns.len(), 1);
        assert_eq!(insns[0].info.mnemonic, "ExitProc");
        assert_eq!(insns[0].raw_len, 1);
        assert_eq!(insns[0].offset, 0);
        assert!(insns[0].operands[0].is_none());
    }

    #[test]
    fn test_lit_i2() {
        // 0xF3 = LitI2, size 3, format "%2"
        let insns = decode_all(&[0xF3, 0x05, 0x00]);
        assert_eq!(insns.len(), 1);
        assert_eq!(insns[0].info.mnemonic, "LitI2");
        assert_eq!(insns[0].raw_len, 3);
        assert_eq!(insns[0].operands[0], Some(Operand::Int16(5)));
    }

    #[test]
    fn test_branch() {
        // 0x1E = Branch, size 3, format "%l"
        let insns = decode_all(&[0x1E, 0x20, 0x00]);
        assert_eq!(insns.len(), 1);
        assert_eq!(insns[0].info.mnemonic, "Branch");
        assert_eq!(insns[0].operands[0], Some(Operand::JumpTarget(0x20)));
    }

    #[test]
    fn test_lit_str() {
        // 0x1B = LitStr, size 3, format "%s"
        let insns = decode_all(&[0x1B, 0x10, 0x00]);
        assert_eq!(insns.len(), 1);
        assert_eq!(insns[0].info.mnemonic, "LitStr");
        assert_eq!(insns[0].operands[0], Some(Operand::ConstPoolIndex(0x10)));
    }

    #[test]
    fn test_fld_rf_var() {
        // 0x04 = FLdRfVar, size 3, format "%a"
        let insns = decode_all(&[0x04, 0x70, 0xFF]);
        assert_eq!(insns.len(), 1);
        assert_eq!(insns[0].info.mnemonic, "FLdRfVar");
        assert_eq!(insns[0].operands[0], Some(Operand::StackVar(-144))); // var_90
    }

    #[test]
    fn test_lit_i4() {
        // 0xF5 = LitI4, size 5, format "%4"
        let insns = decode_all(&[0xF5, 0x78, 0x56, 0x34, 0x12]);
        assert_eq!(insns.len(), 1);
        assert_eq!(insns[0].info.mnemonic, "LitI4");
        assert_eq!(insns[0].operands[0], Some(Operand::Int32(0x12345678)));
    }

    #[test]
    fn test_multiple_instructions() {
        // LitI2 5; LitI2 10; AddI2; ExitProc
        let bytes = [
            0xF3, 0x05, 0x00, // LitI2 5
            0xF3, 0x0A, 0x00, // LitI2 10
            0xA9, // AddI2
            0x14, // ExitProc
        ];
        let insns = decode_all(&bytes);
        assert_eq!(insns.len(), 4);
        assert_eq!(insns[0].info.mnemonic, "LitI2");
        assert_eq!(insns[0].offset, 0);
        assert_eq!(insns[1].info.mnemonic, "LitI2");
        assert_eq!(insns[1].offset, 3);
        assert_eq!(insns[2].info.mnemonic, "AddI2");
        assert_eq!(insns[2].offset, 6);
        assert_eq!(insns[3].info.mnemonic, "ExitProc");
        assert_eq!(insns[3].offset, 7);
    }

    #[test]
    fn test_extended_opcode_lead0() {
        // 0xFB 0x00 = Lead0 table, opcode 0x00
        let bytes = [0xFB, 0x00];
        let insns = decode_all(&bytes);
        assert_eq!(insns.len(), 1);
        assert_eq!(insns[0].info.table, DispatchTable::Lead0);
        assert_eq!(insns[0].raw_len, 2);
    }

    #[test]
    fn test_ffree_var_variable_length() {
        // 0x36 = FFreeVar (size = -1)
        // Format: [0x36] [u16 byte_count=6] [6 bytes of payload]
        let bytes = [
            0x36, // FFreeVar opcode
            0x06, 0x00, // byte_count = 6
            0x70, 0xFF, // var_90
            0x68, 0xFF, // var_98
            0x60, 0xFF, // var_A0
        ];
        let insns = decode_all(&bytes);
        assert_eq!(insns.len(), 1);
        assert_eq!(insns[0].info.mnemonic, "FFreeVar");
        assert_eq!(
            insns[0].operands[0],
            Some(Operand::FrameList { at: 3, count: 3 })
        );
        assert_eq!(insns[0].raw_len, 9); // 1 + 2 + 6
        assert_eq!(insns[0].frame_slots(&bytes), vec![-0x90, -0x98, -0xA0]);
    }

    #[test]
    fn test_ffree_str_variable_length() {
        // 0x32 = FFreeStr (size = -1)
        let bytes = [
            0x32, // FFreeStr
            0x02, 0x00, // byte_count = 2
            0x80, 0xFF, // one var ref
        ];
        let insns = decode_all(&bytes);
        assert_eq!(insns.len(), 1);
        assert_eq!(insns[0].info.mnemonic, "FFreeStr");
    }

    #[test]
    fn test_truncated_instruction() {
        // LitI2 needs 3 bytes total, but we only provide 2
        // The decoder reads the opcode (1 byte), then tries to read operands
        // and hits UnexpectedEndOfPCode
        let bytes = [0xF3, 0x05];
        let iter = InstructionIterator::new(&bytes, bytes.len() as u16);
        let results: Vec<_> = iter.collect();
        // At least one result should be an error
        assert!(results.iter().any(|r| r.is_err()));
    }

    #[test]
    fn test_trailing_zero_padding_is_clean_end() {
        // ExitProcHresult (0x13, size 1) then a single 0x00 alignment pad byte.
        // The lone pad would look like a truncated LargeBos; it must instead
        // terminate the stream cleanly with no error.
        let insns = decode_all(&[0x13, 0x00]);
        assert_eq!(insns.len(), 1);
        assert_eq!(insns[0].info.mnemonic, "ExitProcHresult");
    }

    #[test]
    fn test_trailing_double_zero_padding_no_spurious_bos() {
        // ExitProc (0x14) then two 0x00 pad bytes - a clean `00 00` would decode
        // as a LargeBos; as trailing padding it must be dropped, leaving one insn.
        let insns = decode_all(&[0x14, 0x00, 0x00]);
        assert_eq!(insns.len(), 1);
        assert_eq!(insns[0].info.mnemonic, "ExitProc");
    }

    #[test]
    fn test_midstream_zeros_followed_by_code_still_decode() {
        // A LargeBos `00 00` followed by real code is NOT a trailing tail, so it
        // must still decode (the padding guard only fires on an all-zero tail).
        let insns = decode_all(&[0x00, 0x00, 0x14]);
        assert_eq!(insns.len(), 2);
        assert_eq!(insns[0].info.mnemonic, "LargeBos");
        assert_eq!(insns[1].info.mnemonic, "ExitProc");
    }

    #[test]
    fn test_bos_marker_and_distance() {
        // LargeBos (0x00), operand 0x08 = distance to next statement.
        let b = decode_all(&[0x00, 0x08]);
        assert_eq!(b[0].info.mnemonic, "LargeBos");
        assert!(b[0].is_bos());
        assert_eq!(b[0].bos_distance(), Some(0x08));
        assert!(b[0].error_flow().is_none());
        // A non-BOS instruction reports neither.
        let e = decode_all(&[0x14]); // ExitProc
        assert!(!e[0].is_bos());
        assert_eq!(e[0].bos_distance(), None);
    }

    #[test]
    fn test_error_flow_onerrorgoto() {
        // OnErrorGoto (0x4B), %l operand.
        let lbl = decode_all(&[0x4B, 0x00, 0x01]); // operand 0x0100
        assert_eq!(lbl[0].error_flow(), Some(ErrorFlow::OnErrorGoto(0x0100)));
        assert_eq!(format!("{}", lbl[0]), "0000  On Error GoTo loc_0100");

        let next = decode_all(&[0x4B, 0xFF, 0xFF]); // -1
        assert_eq!(next[0].error_flow(), Some(ErrorFlow::OnErrorResumeNext));
        assert_eq!(format!("{}", next[0]), "0000  On Error Resume Next");

        let zero = decode_all(&[0x4B, 0xFE, 0xFF]); // -2
        assert_eq!(zero[0].error_flow(), Some(ErrorFlow::OnErrorGotoZero));
        assert_eq!(format!("{}", zero[0]), "0000  On Error GoTo 0");
    }

    #[test]
    fn test_error_flow_resume() {
        // Resume lives in the Lead2 table (prefix 0xFD, opcode 0x0C).
        let next = decode_all(&[0xFD, 0x0C, 0xFF, 0xFF]); // -1
        assert_eq!(next[0].error_flow(), Some(ErrorFlow::ResumeNext));
        assert_eq!(format!("{}", next[0]), "0000  Resume Next");

        let bare = decode_all(&[0xFD, 0x0C, 0xFE, 0xFF]); // -2
        assert_eq!(bare[0].error_flow(), Some(ErrorFlow::Resume));
        assert_eq!(format!("{}", bare[0]), "0000  Resume");

        let lbl = decode_all(&[0xFD, 0x0C, 0x0B, 0x00]); // 0x000B
        assert_eq!(lbl[0].error_flow(), Some(ErrorFlow::ResumeLabel(0x000B)));
        assert_eq!(format!("{}", lbl[0]), "0000  Resume loc_000B");
    }

    #[test]
    fn test_truncated_lead_byte() {
        // Lead byte 0xFB at the very end, no second byte within limit
        let bytes = [0xFB];
        let iter = InstructionIterator::new(&bytes, bytes.len() as u16);
        let results: Vec<_> = iter.collect();
        // Should yield something (possibly an error or the lead byte's own entry)
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_empty_stream() {
        let bytes: &[u8] = &[];
        let insns = decode_all(bytes);
        assert!(insns.is_empty());
    }

    #[test]
    fn test_data_type_and_operand_type() {
        // LitI4 - should report I4 as both instruction- and operand-type.
        let insns = decode_all(&[0xF5, 0x78, 0x56, 0x34, 0x12]);
        let insn = &insns[0];
        assert_eq!(insn.data_type(), Some(PCodeDataType::I4));
        assert_eq!(insn.operand_type(0), Some(PCodeDataType::I4));
        // LitI2
        let insns = decode_all(&[0xF3, 0x05, 0x00]);
        let insn = &insns[0];
        assert_eq!(insn.data_type(), Some(PCodeDataType::I2));
        assert_eq!(insn.operand_type(0), Some(PCodeDataType::I2));
        // ExitProc - Return semantics, no data type.
        let insns = decode_all(&[0x14]);
        let insn = &insns[0];
        assert_eq!(insn.data_type(), None);
        // Out-of-range and empty-slot handling.
        assert_eq!(insn.operand_type(0), None);
        assert_eq!(insn.operand_type(7), None);
    }

    #[test]
    fn test_position_tracking() {
        let bytes = [0xAA, 0x14]; // AddI4, ExitProc
        let mut iter = InstructionIterator::new(&bytes, bytes.len() as u16);
        assert_eq!(iter.position(), 0);
        let _ = iter.next();
        assert_eq!(iter.position(), 1);
        let _ = iter.next();
        assert_eq!(iter.position(), 2);
        assert!(iter.next().is_none());
    }

    /// The code ends after a terminator no jump reaches past, whatever bytes
    /// pad the procedure after it: `ExitProc` then `00 ff ff` is one
    /// instruction, where decoding on read the padding as a truncated
    /// `LargeBos`.
    #[test]
    fn decoding_stops_after_the_last_reachable_terminator() {
        let bytes = [0x14, 0x00, 0xFF, 0xFF];
        let decoded: Vec<_> = InstructionIterator::new(&bytes, bytes.len() as u16).collect();
        assert_eq!(decoded.len(), 1);
        assert!(decoded.iter().all(Result::is_ok));
    }

    /// Four or more bytes after a terminator are code: a `Function` with
    /// `GoSub`s ends `Return` (`FC C9`), then the `ExitProc` of `End
    /// Function` nothing reaches, then three bytes of padding
    /// (`tests/fixtures/flow`).
    #[test]
    fn decoding_continues_past_a_terminator_before_four_bytes() {
        let bytes = [0xFC, 0xC9, 0x14, 0xFF, 0xFF, 0xFF];
        let decoded: Vec<Instruction> = InstructionIterator::new(&bytes, bytes.len() as u16)
            .collect::<Result<_, _>>()
            .expect("decodes");
        let mnemonics: Vec<&str> = decoded.iter().map(|i| i.info.mnemonic).collect();
        assert_eq!(mnemonics, vec!["Return", "ExitProc"]);
    }

    /// Code a jump reaches is decoded past a terminator: `Branch +5; ExitProc;
    /// ... ExitProc` decodes the second `ExitProc` the branch targets.
    #[test]
    fn decoding_continues_past_a_terminator_a_jump_reaches() {
        // Branch loc_0004; ExitProc; ExitProc
        let bytes = [0x1E, 0x04, 0x00, 0x14, 0x14];
        let decoded: Vec<Instruction> = InstructionIterator::new(&bytes, bytes.len() as u16)
            .collect::<Result<_, _>>()
            .expect("decodes");
        let offsets: Vec<u16> = decoded
            .iter()
            .map(|instruction| instruction.offset)
            .collect();
        assert_eq!(offsets, vec![0, 3, 4]);
    }

    #[test]
    fn test_invalid_variable_length_size() {
        // FFreeVar with byte_count that exceeds remaining stream
        let bytes = [
            0x36, // FFreeVar
            0xFF, 0x00, // byte_count = 255 (way too large)
        ];
        let iter = InstructionIterator::new(&bytes, bytes.len() as u16);
        let results: Vec<_> = iter.collect();
        assert_eq!(results.len(), 1);
        assert!(results[0].is_err());
    }

    #[test]
    fn test_decode_all_single_byte_primary_opcodes() {
        // Verify that every size-1 primary opcode decodes to exactly 1 byte
        for i in 0..=0xFA_u8 {
            // Skip lead bytes 0xFB-0xFF
            let info = &PRIMARY_TABLE[i as usize];
            if info.size == 1 && info.is_implemented() {
                let bytes = [i];
                let insns = decode_all(&bytes);
                assert_eq!(
                    insns.len(),
                    1,
                    "Opcode 0x{:02X} ({}) should decode to 1 instruction",
                    i,
                    info.mnemonic
                );
                assert_eq!(insns[0].raw_len, 1);
            }
        }
    }
}
