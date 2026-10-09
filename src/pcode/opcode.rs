//! Opcode definitions and dispatch table lookup.
//!
//! Contains static lookup tables for all 6 VB6 P-Code dispatch tables
//! (1536 entries total), generated at build time from `data/opcodes.csv`.
//!
//! # Dispatch Table Organization
//!
//! | Table | Lead Byte | Purpose |
//! |-------|-----------|---------|
//! | Primary | None | Core instruction set (256 opcodes) |
//! | Lead0 | `0xFB` | Extended comparisons, logic, math |
//! | Lead1 | `0xFC` | Type conversions, array ops, I/O |
//! | Lead2 | `0xFD` | Branches, print, member ops |
//! | Lead3 | `0xFE` | VCalls, For/Next, late binding, ReDim |
//! | Lead4 | `0xFF` | Misc, array records, UDT ops |

use crate::pcode::stackeffect::Pop;

/// Dispatch table identifier.
///
/// The VB6 VM uses 6 dispatch tables: one primary table and five
/// extended tables accessed via lead bytes `0xFB`-`0xFF`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum DispatchTable {
    /// Primary table (no lead byte prefix). 256 opcodes.
    Primary = 0,
    /// Lead0 table (prefix `0xFB`). Extended comparisons, logic, math.
    Lead0 = 1,
    /// Lead1 table (prefix `0xFC`). Type conversions, array ops, I/O.
    Lead1 = 2,
    /// Lead2 table (prefix `0xFD`). Branches, print, member ops.
    Lead2 = 3,
    /// Lead3 table (prefix `0xFE`). VCalls, For/Next, late binding, ReDim.
    Lead3 = 4,
    /// Lead4 table (prefix `0xFF`). Misc, array records, UDT ops.
    Lead4 = 5,
}

/// The operand-dependent part of an opcode's evaluation-stack pops, on top
/// of [`OpcodeInfo::pops`].
///
/// Read from the handlers: the runtime releases these slots by an amount an
/// operand gives, or the callee releases its own arguments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StackRule {
    /// No operand-dependent pops.
    Fixed,
    /// The callee releases its arguments (`VCall*`, `ThisVCall*`): only its
    /// signature says how many slots.
    Callee,
    /// The argument byte count of an [`ExternalCall`](super::operand::Operand::ExternalCall)
    /// operand, divided by 4 (`ImpAdCall*`; the runtime checks the callee
    /// released exactly that many).
    ArgBytes {
        /// Index of the operand.
        operand: u8,
    },
    /// Four slots per unit of an operand: Variants passed by value (late-bound
    /// calls and default-member indexing).
    Variants {
        /// Index of the count operand.
        operand: u8,
    },
    /// One slot per unit of an operand (array indices).
    Count {
        /// Index of the count operand.
        operand: u8,
    },
    /// Two slots per unit of an operand (lower and upper bounds).
    Pairs {
        /// Index of the dimension-count operand.
        operand: u8,
    },
    /// Two slots per unit of an operand, at least one pair (`ReDim`).
    PairsAtLeastOne {
        /// Index of the dimension-count operand.
        operand: u8,
    },
    /// An operand's byte count less the 4 bytes of the descriptor the handler
    /// pushes, divided by 4 (`Print #` and kin, cdecl helpers).
    Bytes {
        /// Index of the byte-count operand.
        operand: u8,
    },
}

/// The object an opcode works on, or the receiver of a call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Receiver {
    /// No object.
    None,
    /// Pr, the object register (`[ebp-0x4c]`): `VCall*`, `Late*`, `Mem*`.
    Pr,
    /// `Me` (`[ebp+8]`): `ThisVCall*`, `FLdPrThis`, `RaiseEvent`.
    Me,
    /// The Variant whose address the opcode pops (`VarLateMem*`).
    Popped,
}

/// Where an opcode that loads Pr, the object register (`[ebp-0x4C]`), takes
/// the object from. Read from the handlers; each fails with error 91 when
/// the object is `Nothing`.
///
/// [`Instruction::pr_source`](super::decoder::Instruction::pr_source) gives
/// the source with its operands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PrLoad {
    /// The object in a frame slot: `Pr = [ebp + %a]` (`FLdPr`).
    Frame,
    /// `Me`, `[ebp+8]` (`FLdPrThis`).
    Me,
    /// The value of a constant pool entry itself, not dereferenced
    /// (`ImpAdLdPr`).
    Pool,
    /// The object a frame slot points to: `Pr = [[ebp + %a]]` (`ILdPr`).
    FrameIndirect,
    /// A member of Pr's object: `Pr = [Pr + %2]` (`MemLdPr`).
    PrMember,
    /// A member of the object whose pointer is in a frame slot:
    /// `Pr = [[ebp + %a] + %2]` (`FMemLdPr`).
    FrameMember,
    /// The address of the element of the popped array at the popped indices
    /// (`Ary1LdPr`, `AryLdPr`, `AryInRecLdPr`).
    ArrayElement,
    /// The object of the Variant whose address it pops (`LdPrVar`,
    /// `LdPrUnkVar`).
    Variant,
    /// The object variable whose address it pops, set to a new instance of
    /// the class in pool `%c` first if it is `Nothing` (`NewIfNullPr`).
    NewIfNull,
    /// `[[ebp + 0x10] + %2]` (`IWMemLdPr`); what `[ebp+0x10]` holds is not
    /// established.
    WithMember,
    /// The object a late-bound get returns (`FLdLateIdUnkVar`).
    LateGet,
}

/// Metadata for a single P-Code opcode.
///
/// Each entry describes one slot in one of the 6 dispatch tables,
/// combining encoding information with verified runtime semantics
/// traced from MSVBVM60.DLL handler disassembly.
///
/// All semantic fields (`semantics`, `data_type`) are resolved at
/// **build time** from the CSV data - no runtime string parsing.
#[derive(Debug, Clone, Copy)]
pub struct OpcodeInfo {
    /// Which dispatch table this opcode belongs to.
    pub table: DispatchTable,
    /// The opcode byte within the table (`0x00`-`0xFF`).
    pub index: u8,
    /// Total instruction size in bytes (excluding lead byte).
    ///
    /// - **Positive**: fixed-size instruction (includes the opcode byte itself).
    /// - **`-1`**: variable-length instruction (size encoded as `u16` after opcode).
    /// - **`0`**: unimplemented/invalid opcode slot.
    pub size: i8,
    /// Mnemonic name (e.g., `"FLdRfVar"`, `"AddI4"`, `"InvalidExcode"`).
    pub mnemonic: &'static str,
    /// Operand format string (e.g., `"%a"`, `"%s %2"`, `""` for no operands).
    ///
    /// Format specifiers:
    /// - `%1` - 1-byte literal
    /// - `%2` - 2-byte (Int16) literal
    /// - `%4` - 4-byte (Int32) literal
    /// - `%a` - Stack variable reference (signed Int16 EBP offset)
    /// - `%s` - Constant pool index (Int16)
    /// - `%l` - Jump target (unsigned Int16 from function start)
    /// - `%c` - Control/import index (Int16)
    /// - `%v` - VTable reference (two Int16 values)
    /// - `%x` - External call (two Int16 values)
    pub operand_format: &'static str,
    /// Evaluation stack slots consumed (4 bytes each), not counting the
    /// operand-dependent part [`stack`](Self::stack) adds.
    pub pops: i8,
    /// Evaluation stack slots produced (4 bytes each). Every opcode pushes
    /// at most one value, so this is also its width.
    pub pushes: i8,
    /// The values of the fixed part ([`pops`](Self::pops) and
    /// [`fpu_pops`](Self::fpu_pops)), from both stacks, last operand first:
    /// on one stack, top first (a binary operation's right operand, then its
    /// left); across the two stacks, in the order the handler reads them as
    /// operands (`LtCyR8` is Currency < Double: the Double, then the
    /// Currency).
    ///
    /// On the evaluation stack a Currency or a Double kept off the x87 stack
    /// is 2 slots, a Variant passed by value 4, anything else (an address,
    /// an Integer or Long, a String or object pointer) 1.
    pub popped: &'static [Pop],
    /// Where the operand-dependent values of [`stack`](Self::stack) sit
    /// among [`popped`](Self::popped): before the value at this index (0:
    /// first; `popped.len()`: after them all). The position was read from
    /// the handler where the widths around it differ.
    pub rule_at: u8,
    /// x87 FPU stack values consumed (read below the depth at entry).
    pub fpu_pops: u8,
    /// x87 FPU stack values produced by the opcode itself.
    pub fpu_push: u8,
    /// `true` for a call that pushes nothing on the evaluation stack and
    /// leaves the x87 stack as its callee left it: one value more for a
    /// callee returning a Single, Double or Date, none for a `Sub` (the
    /// `ImpAdCallFPR4` forms, `VCallFPR8`, `ThisVCall`). The mnemonics are
    /// no types: `Sleep 0` compiles to `ImpAdCallFPR4`.
    pub fpu_callee: bool,
    /// `true` if this instruction rewrites the FPU top in place.
    ///
    /// These opcodes (e.g. `FnAbsR4`, `UMiR8`, `CR4R8`) consume ST(0) and
    /// leave their result where it was: `fpu_pops` and `fpu_push` are both 1.
    pub fpu_inplace: bool,
    /// The operand-dependent pops on top of [`pops`](Self::pops).
    pub stack: StackRule,
    /// The object the opcode works on, or the call's receiver.
    pub receiver: Receiver,
    /// Where the opcode loads Pr, the object register, from; `None` if it
    /// does not load it.
    pub pr_load: Option<PrLoad>,
    /// What a load, store or literal moves, in the table's notation (empty
    /// for other opcodes); [`Instruction::movement`](super::decoder::Instruction::movement)
    /// gives it typed, with the instruction's operands.
    pub movement: &'static str,
    /// Semantic category string (e.g., `"arith"`, `"load_frame"`, `"branch"`).
    pub category: &'static str,
    /// Typed semantic classification (generated at build time).
    pub semantics: OpcodeSemantics,
    /// The type the mnemonic's suffix names (generated at build time); see
    /// [`PCodeDataType`] for what it does and does not say.
    pub data_type: Option<PCodeDataType>,
    /// The VB type the opcode fixes, where it fixes one: the operands' type
    /// of an operation (arithmetic, unary, comparison: `SubCy` works on
    /// Currencies; a comparison's result is an i32 Boolean), the target type
    /// of a conversion, the float of an x87 move (`FStFPR8`: a Double, or a
    /// Date, which travels the same way), a string or Variant literal's.
    /// `None` for a move whose mnemonic names only a width (`FStR4`,
    /// `MemLdStr`, `LitCy`; see [`PCodeDataType`]).
    pub value_type: Option<PCodeDataType>,
    /// The handler's address in `MSVBVM60.DLL` 6.00.8176 (0 for an invalid
    /// slot). Opcodes with one handler behave the same whatever their
    /// mnemonics say: `MemLdStr` and `MemLdR4` both load four bytes, `LitCy`
    /// and `LitR8` both push eight.
    pub handler: u32,
}

impl OpcodeInfo {
    /// Returns `true` if this opcode is implemented in the VM.
    ///
    /// Unimplemented slots have mnemonic `"InvalidExcode"` or `"Unknown"`.
    #[inline]
    pub fn is_implemented(&self) -> bool {
        self.mnemonic != "InvalidExcode" && self.mnemonic != "Unknown" && self.size != 0
    }

    /// Returns `true` if this is a variable-length instruction.
    ///
    /// Variable-length instructions (like `FFreeVar`, `FFreeStr`, `FFreeAd`)
    /// encode their payload size as a `u16` immediately after the opcode byte.
    #[inline]
    pub fn is_variable_length(&self) -> bool {
        self.size < 0
    }

    /// Returns `true` if this instruction has any FPU stack effect.
    ///
    /// Covers pushes, pops, **and** in-place TOS modifications.
    #[inline]
    pub fn touches_fpu(&self) -> bool {
        self.fpu_pops > 0 || self.fpu_push > 0 || self.fpu_inplace || self.fpu_callee
    }

    /// Returns `true` if the opcode loads Pr, the object register.
    #[inline]
    pub fn writes_pr(&self) -> bool {
        self.pr_load.is_some()
    }

    /// Returns `true` if this opcode is a lead byte (`0xFB`-`0xFF`).
    #[inline]
    pub fn is_lead_byte(&self) -> bool {
        matches!(
            self.mnemonic,
            "Lead0" | "Lead1" | "Lead2" | "Lead3" | "Lead4"
        )
    }

    /// Returns `true` if control never falls through to the next instruction.
    ///
    /// Includes returns ([`OpcodeSemantics::Return`]), unconditional
    /// branches ([`OpcodeSemantics::Branch`] with `conditional: false`, among
    /// them `Exit For`), the `Return` of a `GoSub`
    /// ([`OpcodeSemantics::GoSubReturn`]), [`OpcodeSemantics::Resume`], `Error`
    /// ([`OpcodeSemantics::Raise`]) and `End` / `Stop`
    /// ([`OpcodeSemantics::End`]).
    /// Conditional branches **do not** terminate - control falls through
    /// to the next instruction on the not-taken path - and neither does
    /// [`OpcodeSemantics::GoSub`], whose `Return` comes back after it.
    ///
    /// Useful for CFG construction and basic-block splitting.
    #[inline]
    pub fn is_terminator(&self) -> bool {
        matches!(
            self.semantics,
            OpcodeSemantics::Return
                | OpcodeSemantics::Branch { conditional: false }
                | OpcodeSemantics::GoSubReturn
                | OpcodeSemantics::Resume
                | OpcodeSemantics::Raise
                | OpcodeSemantics::End
        )
    }

    /// Returns `true` if this opcode is a call instruction.
    ///
    /// Matches any [`OpcodeSemantics::Call`] regardless of [`CallKind`]
    /// (vtable, this-vtable, import-address, late-bound, or other).
    /// Useful for CFG construction (calls split basic blocks in some
    /// analyses) and call-graph extraction.
    #[inline]
    pub fn is_call(&self) -> bool {
        matches!(self.semantics, OpcodeSemantics::Call { .. })
    }

    /// Returns `true` if this opcode is a beginning-of-statement marker.
    ///
    /// Matches [`OpcodeSemantics::Bos`] - the `LargeBos` markers (primary
    /// `0x00`/`0x02`, Lead1 `0xC4`, Lead4 `0x1C`) the compiler emits at the
    /// start of each source statement. Their 1-byte operand is the byte
    /// distance to the next BOS (`0` = last statement). Useful for grouping a
    /// procedure's instruction stream into statements.
    #[inline]
    pub fn is_bos(&self) -> bool {
        matches!(self.semantics, OpcodeSemantics::Bos)
    }
}

/// Sentinel returned by [`lookup`] when a lead byte's secondary opcode index
/// somehow falls outside the 256-entry dispatch table.
///
/// Statically the cast `u8 as usize` cannot exceed 255 and the tables are
/// `[OpcodeInfo; 256]`, so this fallback is unreachable at runtime - it
/// exists to satisfy `clippy::indexing_slicing` without resorting to
/// unchecked indexing in the generated code.
pub static UNKNOWN_OPCODE: OpcodeInfo = OpcodeInfo {
    table: DispatchTable::Primary,
    index: 0,
    size: 0,
    mnemonic: "Unknown",
    operand_format: "",
    pops: 0,
    pushes: 0,
    popped: &[],
    rule_at: 0,
    fpu_pops: 0,
    fpu_push: 0,
    fpu_callee: false,
    fpu_inplace: false,
    stack: StackRule::Fixed,
    receiver: Receiver::None,
    pr_load: None,
    movement: "",
    category: "",
    semantics: crate::pcode::semantics::OpcodeSemantics::Unclassified,
    data_type: None,
    value_type: None,
    handler: 0,
};

// Include the build-time generated tables and lookup function.
include!(concat!(env!("OUT_DIR"), "/opcode_generated.rs"));

/// Returns the total number of implemented (non-Invalid/Unknown) opcodes
/// across all 6 dispatch tables.
pub fn implemented_count() -> usize {
    let tables: [&[OpcodeInfo; 256]; 6] = [
        &PRIMARY_TABLE,
        &LEAD0_TABLE,
        &LEAD1_TABLE,
        &LEAD2_TABLE,
        &LEAD3_TABLE,
        &LEAD4_TABLE,
    ];
    tables
        .iter()
        .flat_map(|t| t.iter())
        .filter(|o| o.is_implemented())
        .count()
}

/// Returns a reference to one of the 6 dispatch tables by index.
///
/// # Arguments
///
/// * `table` - The dispatch table to retrieve.
///
/// # Returns
///
/// A reference to the static `[OpcodeInfo; 256]` array.
pub fn table_by_index(table: DispatchTable) -> &'static [OpcodeInfo; 256] {
    match table {
        DispatchTable::Primary => &PRIMARY_TABLE,
        DispatchTable::Lead0 => &LEAD0_TABLE,
        DispatchTable::Lead1 => &LEAD1_TABLE,
        DispatchTable::Lead2 => &LEAD2_TABLE,
        DispatchTable::Lead3 => &LEAD3_TABLE,
        DispatchTable::Lead4 => &LEAD4_TABLE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_tables_have_256_entries() {
        assert_eq!(PRIMARY_TABLE.len(), 256);
        assert_eq!(LEAD0_TABLE.len(), 256);
        assert_eq!(LEAD1_TABLE.len(), 256);
        assert_eq!(LEAD2_TABLE.len(), 256);
        assert_eq!(LEAD3_TABLE.len(), 256);
        assert_eq!(LEAD4_TABLE.len(), 256);
    }

    #[test]
    fn test_lead_byte_slots_in_primary() {
        assert_eq!(PRIMARY_TABLE[0xFB].mnemonic, "Lead0");
        assert_eq!(PRIMARY_TABLE[0xFC].mnemonic, "Lead1");
        assert_eq!(PRIMARY_TABLE[0xFD].mnemonic, "Lead2");
        assert_eq!(PRIMARY_TABLE[0xFE].mnemonic, "Lead3");
        assert_eq!(PRIMARY_TABLE[0xFF].mnemonic, "Lead4");
    }

    #[test]
    fn test_lead_bytes_size_is_1() {
        for (i, entry) in PRIMARY_TABLE.iter().enumerate().skip(0xFB) {
            assert_eq!(entry.size, 1, "Lead byte 0x{i:02X} should have size 1");
        }
    }

    #[test]
    fn test_known_primary_opcodes() {
        // 0x14 = ExitProc, size 1
        assert_eq!(PRIMARY_TABLE[0x14].mnemonic, "ExitProc");
        assert_eq!(PRIMARY_TABLE[0x14].size, 1);

        // 0x1E = Branch, size 3
        assert_eq!(PRIMARY_TABLE[0x1E].mnemonic, "Branch");
        assert_eq!(PRIMARY_TABLE[0x1E].size, 3);

        // 0xF3 = LitI2, size 3
        assert_eq!(PRIMARY_TABLE[0xF3].mnemonic, "LitI2");
        assert_eq!(PRIMARY_TABLE[0xF3].size, 3);

        // 0xF5 = LitI4, size 5
        assert_eq!(PRIMARY_TABLE[0xF5].mnemonic, "LitI4");
        assert_eq!(PRIMARY_TABLE[0xF5].size, 5);

        // 0xA9 = AddI2, size 1
        assert_eq!(PRIMARY_TABLE[0xA9].mnemonic, "AddI2");
        assert_eq!(PRIMARY_TABLE[0xA9].size, 1);
    }

    #[test]
    fn test_variable_length_opcodes() {
        // 0x29 = FFreeAd, size -1
        assert_eq!(PRIMARY_TABLE[0x29].mnemonic, "FFreeAd");
        assert!(PRIMARY_TABLE[0x29].is_variable_length());

        // 0x32 = FFreeStr, size -1
        assert_eq!(PRIMARY_TABLE[0x32].mnemonic, "FFreeStr");
        assert!(PRIMARY_TABLE[0x32].is_variable_length());

        // 0x36 = FFreeVar, size -1
        assert_eq!(PRIMARY_TABLE[0x36].mnemonic, "FFreeVar");
        assert!(PRIMARY_TABLE[0x36].is_variable_length());
    }

    #[test]
    fn test_lookup_primary() {
        let (info, consumed) = lookup(0x14, None);
        assert_eq!(info.mnemonic, "ExitProc");
        assert_eq!(consumed, 1);
    }

    #[test]
    fn test_lookup_lead0() {
        // Lead0 (0xFB), then some opcode
        let (info, consumed) = lookup(0xFB, Some(0x00));
        assert_eq!(consumed, 2);
        assert_eq!(info.table, DispatchTable::Lead0);
    }

    #[test]
    fn test_lookup_lead4() {
        let (info, consumed) = lookup(0xFF, Some(0x10));
        assert_eq!(consumed, 2);
        assert_eq!(info.table, DispatchTable::Lead4);
    }

    #[test]
    fn test_implemented_count() {
        let count = implemented_count();
        // 1163 implemented slots over 774 distinct handlers in 6.00.8176.
        assert!(count > 1000, "Expected >1000 named opcodes, got {count}");
        assert!(count < 1300, "Expected <1300 named opcodes, got {count}");
    }

    #[test]
    fn test_is_implemented() {
        assert!(PRIMARY_TABLE[0x14].is_implemented()); // ExitProc
        assert!(!PRIMARY_TABLE[0x01].is_implemented()); // InvalidExcode
    }

    #[test]
    fn test_is_lead_byte() {
        assert!(PRIMARY_TABLE[0xFB].is_lead_byte());
        assert!(!PRIMARY_TABLE[0x14].is_lead_byte());
    }

    #[test]
    fn test_is_terminator() {
        // ExitProc (0x14) - Return semantics, terminates the block.
        assert!(PRIMARY_TABLE[0x14].is_terminator());
        // Branch (0x1E) - unconditional Branch{conditional:false}, terminates.
        assert!(PRIMARY_TABLE[0x1E].is_terminator());
        // BranchT (0x1C) - conditional, does NOT terminate (falls through).
        assert!(!PRIMARY_TABLE[0x1C].is_terminator());
        // BranchF (0x1D) - conditional, does NOT terminate.
        assert!(!PRIMARY_TABLE[0x1D].is_terminator());
        // Error (0x45) raises; End (Lead1 0xC8) and Stop (Lead1 0xC2) end.
        assert!(PRIMARY_TABLE[0x45].is_terminator());
        assert!(LEAD1_TABLE[0xC8].is_terminator());
        assert!(LEAD1_TABLE[0xC2].is_terminator());
        // AddI2 (0xA9) - arithmetic, not a terminator.
        assert!(!PRIMARY_TABLE[0xA9].is_terminator());
        // FLdRfVar (0x04) - load, not a terminator.
        assert!(!PRIMARY_TABLE[0x04].is_terminator());
    }

    #[test]
    fn test_is_call() {
        // ImpAdCallI4 lives in Lead3 - find any Call-classified opcode.
        let any_primary_call = PRIMARY_TABLE.iter().any(|o| o.is_call());
        let any_lead3_call = LEAD3_TABLE.iter().any(|o| o.is_call());
        // At least one of the primary or Lead3 tables must contain calls.
        assert!(
            any_primary_call || any_lead3_call,
            "expected at least one Call opcode across primary+lead3 tables"
        );
        // ExitProc is not a call.
        assert!(!PRIMARY_TABLE[0x14].is_call());
        // AddI2 is not a call.
        assert!(!PRIMARY_TABLE[0xA9].is_call());
        // Branch is not a call.
        assert!(!PRIMARY_TABLE[0x1E].is_call());
    }

    #[test]
    fn test_terminator_and_call_are_disjoint() {
        // No opcode should be both a terminator and a call.
        let tables: [&[OpcodeInfo; 256]; 6] = [
            &PRIMARY_TABLE,
            &LEAD0_TABLE,
            &LEAD1_TABLE,
            &LEAD2_TABLE,
            &LEAD3_TABLE,
            &LEAD4_TABLE,
        ];
        for entry in tables.iter().flat_map(|t| t.iter()) {
            assert!(
                !(entry.is_terminator() && entry.is_call()),
                "{} is both terminator and call",
                entry.mnemonic
            );
        }
    }

    #[test]
    fn test_table_by_index() {
        let primary = table_by_index(DispatchTable::Primary);
        assert_eq!(primary[0x14].mnemonic, "ExitProc");

        let lead0 = table_by_index(DispatchTable::Lead0);
        assert_eq!(lead0.len(), 256);
    }

    #[test]
    fn test_operand_format_specifiers_normalized() {
        // LitI2 should have format %2
        assert_eq!(PRIMARY_TABLE[0xF3].operand_format, "%2");

        // Branch should have format %l
        assert_eq!(PRIMARY_TABLE[0x1E].operand_format, "%l");

        // FLdRfVar should have format %a
        assert_eq!(PRIMARY_TABLE[0x04].operand_format, "%a");

        // LitStr should have format %s
        assert_eq!(PRIMARY_TABLE[0x1B].operand_format, "%s");
    }

    #[test]
    fn test_semantics_fields_populated() {
        // AddI4 (0xA9): pops=2, pushes=1, category=arith
        assert_eq!(PRIMARY_TABLE[0xA9].pops, 2);
        assert_eq!(PRIMARY_TABLE[0xA9].pushes, 1);
        assert_eq!(PRIMARY_TABLE[0xA9].category, "arith");

        // FLdRfVar (0x04): pops=0, pushes=1, pushes the slot's address
        assert_eq!(PRIMARY_TABLE[0x04].pops, 0);
        assert_eq!(PRIMARY_TABLE[0x04].pushes, 1);
        assert_eq!(PRIMARY_TABLE[0x04].movement, "ad:f(%a)");
        assert_eq!(PRIMARY_TABLE[0x04].category, "load_frame");

        // FStR8 (0x72): pops=2, pushes=0, stores 8 bytes
        assert_eq!(PRIMARY_TABLE[0x72].pops, 2);
        assert_eq!(PRIMARY_TABLE[0x72].pushes, 0);
        assert_eq!(PRIMARY_TABLE[0x72].movement, "st:f(%a):8");

        // FLdFPR4 (0x6E): fpu_push=1, no eval stack change
        assert_eq!(PRIMARY_TABLE[0x6E].fpu_push, 1);
        assert_eq!(PRIMARY_TABLE[0x6E].pops, 0);
        assert_eq!(PRIMARY_TABLE[0x6E].pushes, 0);

        // FStFPR8 (0x74): fpu_pops=1
        assert_eq!(PRIMARY_TABLE[0x74].fpu_pops, 1);

        // InvalidExcode (0x01): all semantics zero, empty category
        assert_eq!(PRIMARY_TABLE[0x01].pops, 0);
        assert_eq!(PRIMARY_TABLE[0x01].pushes, 0);
        assert_eq!(PRIMARY_TABLE[0x01].category, "");

        // Lead0 table: AddVar (0x94) pops the two operands' addresses and
        // pushes the address of the result temp
        assert_eq!(LEAD0_TABLE[0x94].mnemonic, "AddVar");
        assert_eq!(LEAD0_TABLE[0x94].pops, 2);
        assert_eq!(LEAD0_TABLE[0x94].pushes, 1);
        assert_eq!(LEAD0_TABLE[0x94].category, "arith");

        // Lead1 table: VCallHresult's arguments are the callee's to release
        assert_eq!(LEAD1_TABLE[0x69].stack, StackRule::Callee);
        assert_eq!(
            PRIMARY_TABLE[0x0A].stack,
            StackRule::ArgBytes { operand: 0 }
        );

        // Lead1 table: CStrR8 (0x00) fpu_pops=1, pushes=1
        assert_eq!(LEAD1_TABLE[0x00].fpu_pops, 1);
        assert_eq!(LEAD1_TABLE[0x00].pushes, 1);
        assert_eq!(LEAD1_TABLE[0x00].category, "convert");
    }

    #[test]
    fn test_value_type_only_where_fixed() {
        use crate::pcode::semantics::PCodeDataType;
        // Moves name a width: no type.
        assert_eq!(PRIMARY_TABLE[0x71].mnemonic, "FStR4");
        assert_eq!(PRIMARY_TABLE[0x71].value_type, None);
        assert_eq!(PRIMARY_TABLE[0xF6].mnemonic, "LitCy");
        assert_eq!(PRIMARY_TABLE[0xF6].value_type, None);
        // Operations, conversions, x87 moves and typed literals have one.
        assert_eq!(PRIMARY_TABLE[0xAA].value_type, Some(PCodeDataType::I4));
        assert_eq!(PRIMARY_TABLE[0xEB].mnemonic, "CR8I2");
        assert_eq!(PRIMARY_TABLE[0xEB].value_type, Some(PCodeDataType::R8));
        assert_eq!(PRIMARY_TABLE[0x74].value_type, Some(PCodeDataType::FPR8));
        assert_eq!(PRIMARY_TABLE[0x1B].value_type, Some(PCodeDataType::Str));
    }

    #[test]
    fn test_all_opcodes_have_valid_table_field() {
        let tables: [(DispatchTable, &[OpcodeInfo; 256]); 6] = [
            (DispatchTable::Primary, &PRIMARY_TABLE),
            (DispatchTable::Lead0, &LEAD0_TABLE),
            (DispatchTable::Lead1, &LEAD1_TABLE),
            (DispatchTable::Lead2, &LEAD2_TABLE),
            (DispatchTable::Lead3, &LEAD3_TABLE),
            (DispatchTable::Lead4, &LEAD4_TABLE),
        ];
        for (expected_table, table) in &tables {
            for entry in table.iter() {
                assert_eq!(
                    entry.table, *expected_table,
                    "Opcode 0x{:02X} in {:?} has wrong table field {:?}",
                    entry.index, expected_table, entry.table
                );
            }
        }
    }
}
