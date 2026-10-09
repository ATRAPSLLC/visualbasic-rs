//! P-Code: the bytecode a VB6 project compiled with `CompilationType=-1`
//! runs on the interpreter in `MSVBVM60.DLL`.
//!
//! # Modules
//!
//! - **Opcode tables** ([`opcode`]): the six dispatch tables (1536 slots),
//!   generated at build time from `data/opcodes.csv`, with each opcode's
//!   encoding and runtime effect.
//! - **Operands** ([`operand`]): typed operands and the format specifiers.
//! - **Decoder** ([`decoder`]): [`InstructionIterator`](decoder::InstructionIterator)
//!   yields a procedure's [`Instruction`](decoder::Instruction)s.
//! - **Display** ([`display`]): disassembly text.
//! - **Semantics** ([`semantics`]): opcode categories and data types.
//! - **Movement** ([`movement`]): what a load, store or literal moves, and
//!   its side effects.
//! - **Stack effects** ([`stackeffect`]): what one instruction pops and
//!   pushes on the evaluation and x87 stacks, and where it loads Pr from.
//! - **Call targets** ([`calltarget`]): who a call opcode reaches and its
//!   signature.
//! - **Stack simulation** ([`stacksim`]): both stacks over a procedure's
//!   control flow, every value resolved to the width its producer pushed.
//! - **Frame variables** ([`framevar`]): names for `%a` frame offsets.
//!
//! # The interpreter
//!
//! What follows was read from the handlers of `MSVBVM60.DLL` 6.00.8176 (the
//! Visual Basic 6.0 release; addresses below are its, image base
//! 0x66000000) and checked against 6.00.9848, whose handlers are the same
//! code at other addresses, and against the compiled fixtures in
//! `tests/fixtures`.
//!
//! ## Dispatch
//!
//! `esi` is the instruction pointer. A handler reads its operands at
//! `[esi]`, advances `esi` past them and dispatches the next instruction
//! with `xor eax, eax; mov al, [esi]; inc esi; jmp [table + eax*4]`. The
//! primary table is at 0x66106954; bytes `0xFB`-`0xFF` select the five
//! extended tables at 0x400-byte steps after it (the last has 71 entries).
//! Every invalid slot of every table jumps to one handler (0x6610c41f).
//!
//! ## Two stacks
//!
//! The **evaluation stack** is the native stack (`esp`) below the frame, in
//! 4-byte slots. Most values take one slot (an Integer is widened); a
//! Currency, or a Double kept off the x87 stack (`FLdR8`, `LitCy`), two; a
//! Variant passed by value four (late-bound arguments, `PopAdLdVar`). Most
//! Variant opcodes work on a Variant's **address**: arithmetic on Variants
//! pops two addresses and pushes the address of a frame temporary holding
//! the result. Singles, Doubles and Dates are computed on the **x87
//! stack**. [`Instruction::stack_effect`](decoder::Instruction::stack_effect)
//! gives each instruction's effect on both;
//! [`ProcedureStack`](stacksim::ProcedureStack) follows them over a
//! procedure.
//!
//! The compiler pushes operands in source order and the handlers keep it:
//! `SubI4` is `pop eax; sub [esp], eax`, `SubR8` is `fsubp st(1), st(0)`
//! (left minus right), the comparisons compare the deeper operand with the
//! top one.
//!
//! ## Pr, the object register
//!
//! `[ebp-0x4C]` holds the object the next member access or call works on.
//! A loader sets it and pushes nothing (`FLdPr`, `FLdPrThis`, `MemLdPr`,
//! ...: [`PrLoad`](opcode::PrLoad)); `Mem*` opcodes then read and write
//! `[Pr + offset]`, and `VCall*` / `Late*` call methods on it
//! ([`Receiver::Pr`](opcode::Receiver::Pr)). `ThisVCall*` call on `Me`
//! instead.
//!
//! ## The frame
//!
//! Every P-Code procedure has the same frame
//! ([`pcode_frame`](crate::vb::procedure::pcode_frame)): its arguments from
//! `ebp+0x0C`, after `ebp+8`, which holds `Me` in an object's method and the
//! module's data block in a standard module's procedure (`ProcCallEngine`,
//! 0x66104a99, inserts it). A module's own variables are members of that
//! block (`FMem*` with frame offset 8); `Static` locals live in a block
//! whose pointer is at module data + 0x3C (`Me` + 0x40 in a class). Below
//! `ebp` are 0x84 bytes of interpreter slots - Pr, the constant pool
//! (`[ebp-0x54]`), the code base jump targets are relative to
//! (`[ebp-0x58]`), the error handler state - then the procedure's locals.
//!
//! ## Calls
//!
//! - `VCall*` / `ThisVCall*` push the receiver and call `[vtable + offset]`;
//!   the callee releases the receiver and its arguments. A method of a VB
//!   class returns an `HRESULT`, its result through a last `[out, retval]`
//!   pointer the caller pushes first (`VCallHresult` checks the
//!   `HRESULT`).
//! - `ImpAdCall*` call the address in a constant pool entry - a P-Code
//!   procedure's thunk, a `Declare` stub, an import thunk to a runtime
//!   function - and check the callee released exactly the argument bytes
//!   the operand states (error 49 otherwise).
//! - The mnemonics name no types: the `ImpAdCallFPR4` form pushes nothing
//!   and serves every callee whose result is not in `eax` / `edx:eax` (a
//!   Single, Double or Date left on the x87 stack, a `Sub`, a Variant
//!   through a hidden pointer).
//! - Late-bound calls (`LateMem*` by name, `LateId*` by DISPID) pass their
//!   arguments as Variants by value.
//! - `ExitProc*` return the value the procedure stored in its frame
//!   (`ebp-0x86` for an Integer, `ebp-0x88` four bytes, `ebp-0x8C` eight;
//!   `ExitProcR4` / `ExitProcR8` load it onto the x87 stack); they never
//!   take it from the evaluation stack.
//!
//! ## Constant pool
//!
//! Each object has one pool, an array of 4-byte entries `%s` / `%c`
//! operands index ([`ConstantPool`](crate::vb::constantpool::ConstantPool)):
//! strings, IIDs, ObjectInfos, procedure thunks, `Declare` stubs, import
//! thunks, the addresses of other modules' variables.
//!
//! ## Statements and errors
//!
//! In a procedure with an error handler the compiler starts each statement
//! with `LargeBos`, which records the statement's start at `[ebp-0x14]`; its
//! operand is the statement's length in bytes, which `Resume Next` skips.
//! `OnErrorGoto` takes a handler label, or `0xFFFF` (`On Error Resume
//! Next`) or `0xFFFE` (`On Error GoTo 0`); `Resume` a label, `0xFFFF`
//! (`Resume Next`) or `0xFFFE` (`Resume`)
//! ([`Instruction::error_flow`](decoder::Instruction::error_flow)). The
//! runtime enters a handler with empty stacks.
//!
//! `GoSub` pushes its return position on the evaluation stack, and its
//! `Return` pops it: a `GoSub` body runs one slot deeper.
//!
//! ## The hypothesis the stack simulation measures
//!
//! On the fixtures (183 procedures), with the opcode table's effects and
//! the call resolver's signatures, every pop finds values of the widths it
//! expects, paths agree wherever they meet, every statement starts with no
//! value on the stack and every exit leaves both stacks empty
//! (`tests/stack.rs`). Paths through calls whose arity only an external
//! interface's description could give are not measured.

pub mod calltarget;
pub mod decoder;
pub mod display;
pub mod framevar;
pub mod movement;
pub mod opcode;
pub mod operand;
pub mod semantics;
pub mod stackeffect;
pub mod stacksim;
