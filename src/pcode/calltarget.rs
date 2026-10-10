//! Call-signature resolution for the P-Code call opcodes.
//!
//! A call opcode's evaluation-stack effect depends on its callee: a
//! `VCallHresult` pops whatever arguments the method it reaches takes, an
//! `ImpAdCall*` pops the argument bytes its operand states, a late-bound call
//! four slots per Variant argument. [`CallResolver`] finds the callee of each
//! call opcode from the project and the constant pool of the calling object,
//! and returns a [`CallSignature`]: who is called, how many slots the
//! arguments take and each one's width, and whether a float result comes
//! back on the x87 stack. [`Instruction::stack_effect`] turns it into the
//! instruction's [`StackEffect`](super::stackeffect::StackEffect).
//!
//! | Opcode family | Callee from |
//! |---------------|-------------|
//! | `VCallHresult` (`%v`) | the IID in the pool: the project object whose default interface it is, then the method at the vtable offset; otherwise [`Callee::External`] |
//! | `ThisVCall*` | `Me`'s object (the calling object): the method at the vtable offset |
//! | `VCall*` (`%2`) | the receiver's class is not in the instruction: [`Callee::Unknown`]; a caller that knows Pr's source asks [`CallResolver::resolve_with_pr`] (Pr loaded by `FLdPrThis` is `Me`), one that knows its class asks [`CallResolver::resolve_vtable`] |
//! | `ImpAdCall*` (`%x`) | the pool entry: a procedure thunk (module procedure, `Friend` method), a `Declare` stub or an import thunk; the operand's byte count is exact either way |
//! | `Late*` | the member name (pool, [`ConstantPool::name_at`]) or the DISPID, and the argument count operand; a `LateId*` on a receiver whose class the frame slots know reaches the member its DISPID names ([`LateTarget`], [`CallResolver::resolve_with_slots`]) |
//!
//! # The vtable of a project object
//!
//! The method link table
//! ([`OptionalObjectInfo::method_link_table_va`](crate::vb::object::OptionalObjectInfo::method_link_table_va),
//! [`method_link_count`](crate::vb::object::OptionalObjectInfo::method_link_count)
//! entries) lists
//! the object's own vtable slots in order: entry `k` is the slot at
//! `user_base + 4 * k`. An entry is a P-Code stub (`33 C0 BA <ProcDscInfo>`
//! or `BA <ProcDscInfo>`) naming one of the object's methods, a runtime
//! accessor stub for a member variable (`58 81 04 24 <offset> ... jmp` for
//! a `WithEvents` variable, `81 44 24 04 <offset> ... jmp` for a `Public`
//! one, reaching `GetMem*` / `PutMem*` / `SetMem*`), or null (three per
//! `Implements`). Public variables' accessors come first, then the null
//! entries, then public `WithEvents` accessors, the public procedures at
//! their [`FuncTypDesc`] offsets, every other method (`Private`, `Friend`,
//! event handlers, `Class_Initialize`) in method-table order, and last the
//! private `WithEvents` accessors (see [`MethodLink`](crate::project::MethodLink)
//! for the measured order).
//!
//! `user_base` is `0x1C + 4 * n`: the runtime builds an object's vtable from
//! `IDispatch`'s 7 slots, `n` inherited slots
//! ([`OptionalObjectInfo::inherited_vtable_slots`](crate::vb::object::OptionalObjectInfo::inherited_vtable_slots))
//! and the method links. `n` is 0 for a class. A designer object's
//! inherited slots are its [designer](crate::vb::designer::Designer)'s
//! built-in interface (`_Form` 0x2F8 bytes, `_UserControl` 0x3A4; the size
//! is the interface's `cbSizeVft` in `VB6.OLB`) after `IDispatch`, then 256
//! control getters (the control with
//! [`ControlInfo::index`](crate::vb::control::ControlInfo::index) `i` at
//! `size + 4 * i`, taking no argument and returning the control in eax), so
//! `user_base` is `size + 0x400`: 0x6F8 for a Form or MDIForm, 0x7A4 for a
//! UserControl, 0x770 for a UserDocument, 0x710 for a PropertyPage
//! (`tests/fixtures/forms`, `mdi`, `ocx`, `docs`). A member of the built-in
//! interface resolves to [`Callee::External`] with the built-in interface's
//! IID, a getter to [`Callee::Control`].
//!
//! A form's or UserControl's vtable is named by two IIDs: its default
//! interface (OptionalObjectInfo, [`VbObject::default_iids`]) and its own
//! full interface, which the form data header records at +0x15 and which
//! `Me.Caption`-style calls name.
//!
//! External interfaces (the intrinsic controls and forms of `VB6.OLB`, the
//! VBA objects of the runtime, ActiveX controls, referenced type libraries)
//! are not described in the executable: their signatures are in the type
//! libraries that define them, keyed by [`Callee::External`]'s IID and
//! vtable offset. A caller that has them supplies an [`InterfaceCatalog`]
//! ([`CallResolver::with_interfaces`]), and such calls resolve to
//! [`Callee::Interface`] with the member's argument widths. The runtime's
//! own interfaces that no type library describes are built in
//! ([`RuntimeInterfaces`]) and resolve without a catalog: the control-array
//! object a form's getter returns for a control array, which the compiler
//! records under the all-zero IID (`Item` at 0x40 takes the index and the
//! return pointer; `LBound` 0x44, `UBound` 0x48 and `Count` 0x4C the return
//! pointer).
//!
//! # Hosted controls
//!
//! A form's getter for a control the project hosts (an ActiveX control, or
//! one of the project's own UserControls) returns the control's extender:
//! the compiler calls it late, by DISPID. The control's form record names
//! its class by ProgID, and [`VbProject::components`] gives the class's
//! default interface, which the getter's result is typed with
//! ([`CallResolver::returned_interface`]). A DISPID of the class names one
//! of its members: a UserControl's procedure by its prototype's
//! [`FuncTypDesc::dispid`], an ActiveX control's member through the
//! caller's catalog ([`InterfaceCatalog::dispatched`]). The extender's own
//! members are called 0x3000 below the DISPIDs `_VBControlExtender`
//! declares ([`RuntimeInterfaces::extender_member`]).
//!
//! [`Instruction::stack_effect`]: super::decoder::Instruction::stack_effect

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fmt,
    sync::Arc,
};

use crate::{
    error::Error,
    imports::ImportSymbol,
    pcode::{
        decoder::Instruction,
        opcode::{Receiver, StackRule},
        operand::Operand,
        semantics::{CallKind, OpcodeSemantics},
        stackeffect::PrSource,
        stacksim::{InstructionStack, ProcedureStack, StackValue},
    },
    project::{MethodEntry, MethodLinkKind, VbObject, VbProject},
    vb::{
        constantpool::{ConstantPool, PoolEntry},
        control::Guid,
        exports::VbParamType,
        functype::{ArgType, FuncTypDesc, PropertyKind},
    },
};

/// Vtable offset of the first slot after `IDispatch`'s 7: a class's first
/// own slot, a designer object's first inherited one.
const CLASS_USER_BASE: u16 = 0x1C;

/// One entry of an object's method link table.
enum Link {
    /// A null entry: a slot with no code.
    Empty,
    /// A P-Code stub: the method's ProcDscInfo VA.
    Procedure(u32),
    /// A runtime accessor for the member variable at `offset` in the instance.
    Variable {
        /// Offset of the variable in the instance.
        offset: u32,
        /// The runtime function the stub jumps to.
        function: String,
    },
    /// Anything else (a native thunk).
    Other,
}

/// Who a call reaches.
#[derive(Debug, Clone)]
pub enum Callee {
    /// A procedure of the project: method `method` of object `object` (the
    /// indices of [`VbProject::objects`] and [`VbObject::methods`]).
    Procedure {
        /// Index of the object.
        object: u16,
        /// Index of the method in the object's method table.
        method: u16,
    },
    /// A `Declare` function, called through its stub and `DllFunctionCall`.
    Declare {
        /// DLL name (e.g. `"kernel32"`).
        library: String,
        /// Function name (e.g. `"GetTickCount"`).
        function: String,
    },
    /// A function the executable imports, called through its import thunk:
    /// in P-Code, a runtime function such as `rtcIsMissing` or `VarPtr`.
    Import {
        /// The library (e.g. `"MSVBVM60.DLL"`).
        library: String,
        /// The function's name ([`Import::function`](crate::imports::Import::function): an ordinal of the
        /// runtime is named from its export table, another as `#<ordinal>`).
        function: String,
        /// The ordinal, for an import by ordinal.
        ordinal: Option<u16>,
    },
    /// A member of an interface the project does not implement (a control,
    /// a referenced type library): its signature is in the type library that
    /// defines the interface, not in the executable.
    External {
        /// The interface's IID.
        iid: Guid,
        /// Byte offset of the member in the interface's vtable.
        vtable_offset: u16,
    },
    /// A member of an external interface the caller's
    /// [`InterfaceCatalog`] describes.
    Interface {
        /// The interface's IID.
        iid: Guid,
        /// Byte offset of the member in the interface's vtable.
        vtable_offset: u16,
        /// The member, under each name the catalog gives the slot.
        members: Vec<InterfaceMember>,
    },
    /// The getter a form's or UserControl's vtable has for one of its
    /// controls: no argument, the control (for a control array, the array
    /// object) returned in eax.
    Control {
        /// Index of the object that owns the control.
        object: u16,
        /// The control's index ([`ControlInfo::index`](crate::vb::control::ControlInfo::index)).
        index: u16,
        /// The control's name.
        name: String,
    },
    /// The runtime accessor a vtable slot has for a member variable of the
    /// object (a `WithEvents` or `Public` variable): `GetMem*` takes the
    /// `[out, retval]` pointer, `PutMem*` / `SetMem*` the value: two slots
    /// for `PutMem8` (a `Double`, `Date` or `Currency`), the Variant's
    /// address for `PutMemVar` / `SetMemVar`, one slot otherwise.
    Variable {
        /// Index of the object that owns the variable.
        object: u16,
        /// Offset of the variable in the instance.
        offset: u32,
        /// The runtime function (`GetMemEvent`, `PutMem4`, `SetMemObj`, ...).
        function: String,
    },
    /// The handlers of an event the calling object raises (`RaiseEvent`).
    Event {
        /// The event's id, the instruction's `%4` operand (1 for the first
        /// event the object declares, `tests/fixtures/events`).
        id: i32,
    },
    /// A late-bound `IDispatch` member.
    Late {
        /// The member's name (`LateMem*`), from the constant pool.
        name: Option<String>,
        /// The member's DISPID (`LateId*`).
        dispid: Option<i32>,
        /// The member the DISPID names, when the receiver's class is known
        /// ([`CallResolver::resolve_with_slots`]). The call still goes
        /// through `IDispatch::Invoke`, its arguments Variants by value.
        target: Option<LateTarget>,
    },
    /// The instruction does not say (`VCall*` on Pr of unknown interface).
    Unknown {
        /// Byte offset of the member in the receiver's vtable, if the
        /// opcode carries one.
        vtable_offset: Option<u16>,
    },
}

/// The member a late-bound call by DISPID reaches ([`Callee::Late`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LateTarget {
    /// A procedure of the project: a public method or property procedure
    /// of one of its classes or UserControls, whose
    /// [`FuncTypDesc::dispid`] is the call's.
    Procedure {
        /// Index of the object.
        object: u16,
        /// Index of the method in the object's method table.
        method: u16,
        /// How the call invokes it.
        invoke: InvokeKind,
    },
    /// Members of an external interface the [`InterfaceCatalog`] describes
    /// ([`InterfaceCatalog::dispatched`]): a hosted control's own, or its
    /// extender's (`_VBControlExtender`,
    /// [`RuntimeInterfaces::extender_member`]). Each is invoked the way the
    /// call invokes it.
    Members {
        /// The interface's IID.
        iid: Guid,
        /// The member's DISPID in that interface.
        dispid: i32,
        /// The member, under each name the catalog gives the DISPID.
        members: Vec<InterfaceMember>,
    },
}

/// What a call opcode passes and gets back.
#[derive(Debug, Clone)]
pub struct CallSignature<'a> {
    /// Who the call reaches.
    pub callee: Callee,
    /// Evaluation-stack slots the arguments take, which the call pops; not
    /// counting a receiver the opcode pushes itself. `None` when only the
    /// callee's (unknown) signature could say.
    pub arg_slots: Option<u16>,
    /// Each argument's width in slots, in the order they are popped (top of
    /// the stack first), the `[out, retval]` pointer last when the callee
    /// has one. `None` when the callee's parameter types are unknown.
    ///
    /// A vtable, procedure, `Declare` or runtime call (`VCall*`,
    /// `ThisVCall*`, `ImpAdCall*`) is stdcall: the compiler pushes the last
    /// parameter first, so this is parameter order (the first parameter on
    /// top). A late-bound call (`Late*`) and `RaiseEvent` push their Variants
    /// in source order, the order `IDispatch::Invoke` takes them (the last
    /// argument first in `DISPPARAMS`), so the last argument is on top
    /// (`tests/fixtures/late`: `o.Add "a", 1` pushes `"a"`, then `1`).
    pub arg_widths: Option<Vec<u8>>,
    /// The callee's prototype, for a project method that has one.
    pub signature: Option<FuncTypDesc<'a>>,
    /// `Some(true)` when the callee leaves a Single, Double or Date result on
    /// the x87 stack (a P-Code procedure ending in `ExitProcR4` /
    /// `ExitProcR8`, a runtime function returning a float), `Some(false)`
    /// when it does not, `None` when unknown (a `Declare` function, an
    /// external interface). Only the calls with
    /// [`OpcodeInfo::fpu_callee`](super::opcode::OpcodeInfo::fpu_callee)
    /// pass the callee's x87 result on.
    pub float_result: Option<bool>,
}

/// What the resolver knows of one method of the project.
#[derive(Debug, Clone)]
struct MethodFacts<'a> {
    /// The method's prototype, if it has one.
    ftd: Option<FuncTypDesc<'a>>,
    /// The evaluation-stack slots of its arguments, from its ProcDscInfo's
    /// argument size (less the `ebp+8` slot every P-Code frame has).
    arg_slots: Option<u16>,
    /// Whether it returns through the x87 stack (`ExitProcR4` / `ExitProcR8`).
    float_result: Option<bool>,
}

/// What the resolver knows of one object of the project.
#[derive(Debug, Clone, Default)]
struct ObjectFacts<'a> {
    /// The IIDs that name the object's vtable: its default interface and,
    /// for a form or UserControl, its own full interface.
    iids: Vec<Guid>,
    /// Its methods, by method-table index.
    methods: Vec<MethodFacts<'a>>,
    /// Method index by vtable offset, for the methods whose slot is known.
    by_vtable: HashMap<u16, u16>,
    /// Control getters by vtable offset.
    controls: HashMap<u16, ControlGetter>,
    /// Member variable accessors by vtable offset: the variable's instance
    /// offset and the runtime function.
    variables: HashMap<u16, (u32, String)>,
    /// A designer object's built-in interface (`_Form`, `_UserControl`, ...)
    /// and its vtable size: offsets below it are that interface's members.
    builtin: Option<(Guid, u16)>,
    /// The VA of its ObjectInfo, which a prototype's class type names.
    info_va: u32,
}

/// The getter a form's or UserControl's vtable has for one of its controls.
#[derive(Debug, Clone, Default)]
struct ControlGetter {
    /// The control's index.
    index: u16,
    /// Its name.
    name: String,
    /// The default interface of a hosted control's class, the class its
    /// form record names ([`FormControlRecord::prog_id`]); `None` for an
    /// intrinsic control and a control array.
    ///
    /// [`FormControlRecord::prog_id`]: crate::vb::formdata::FormControlRecord::prog_id
    interface: Option<Guid>,
    /// For a control array of a hosted class, that class's default
    /// interface: the array's `Item` returns an element.
    elements: Option<Guid>,
}

/// The interfaces a procedure's own code states for the objects in its frame
/// slots, which a `VCall*` without an IID needs.
///
/// A module's procedure records no parameter or local types, but its code
/// names them where it uses them (`tests/fixtures/vtable`: `Vtable(ByVal r
/// As IRaw)` copies `r` to a local whose typed `VCallUI1` / `VCallFPR8`
/// calls carry no IID, and whose `VCallHresult` names `IRaw`). A slot's
/// interface comes from:
///
/// - a `VCallHresult` (an IID operand) on a Pr loaded from the slot;
/// - `CastAd [IID]` (a QueryInterface) stored into the slot (`FStAdFunc`,
///   `FStAd`);
/// - a call whose result's interface is known
///   ([`CallResolver::returned_interface`]: a catalog member's
///   [`InterfaceMember::returns_interface`], a project function returning
///   one of the project's classes, a runtime export such as `rtcErrObj`),
///   stored into the slot when the call pushes it, or written to the slot
///   whose address it takes as its `[out, retval]` pointer
///   (`FLdRfVar slot; VCallHresult`);
/// - a copy of another slot (`ILdRf a; FStAd b`, or `FLdZeroAd a; FStAdFunc
///   b`, which moves the reference), both ways.
///
/// A slot given two different interfaces has none as a whole: it is a
/// temporary the compiler reuses (`tests/fixtures/vtable`: `var_E4` holds
/// `r.GetUnknown()`'s result, then `o.GetSelf()`'s). A call on a Pr loaded
/// from it takes the interface of the latest store into it before the call
/// ([`for_pr_at`](Self::for_pr_at)), which [`CallResolver::resolve_with_slots`]
/// uses.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SlotInterfaces {
    slots: HashMap<i16, Guid>,
    conflicting: HashSet<i16>,
    /// Each store into a frame slot, by slot and P-Code offset, with the
    /// interface of what it stores when known.
    stores: BTreeMap<i16, BTreeMap<u16, Option<Guid>>>,
    /// The interface of the elements of the control array a slot holds
    /// ([`CallResolver::returned_elements`]); `None` for a slot two arrays
    /// of different classes pass through.
    elements: HashMap<i16, Option<Guid>>,
}

/// What one store into a frame slot stores.
enum StoredValue {
    /// An object of this interface.
    Known(Guid),
    /// A copy of another slot.
    Copy(i16),
    /// Something else.
    Unknown,
}

impl SlotInterfaces {
    /// Infers the slots' interfaces from `code` and the Pr sources of a
    /// simulation of it, repeating until they settle: a call on a slot
    /// resolves (and names what it returns) only once the slot's own
    /// interface is known (`Set o = r.GetSelf()`).
    pub fn infer(
        resolver: &CallResolver<'_, '_>,
        object: u16,
        code: &[Instruction],
        stack: &ProcedureStack,
    ) -> Self {
        Self::infer_seeded(resolver, object, code, stack, &Self::default())
    }

    /// Infers the slots' interfaces like [`infer`](Self::infer), starting
    /// from the interfaces `seed` gives (what the procedure's callers and
    /// callees state for its parameters and arguments, [`ProjectSlots`]).
    pub fn infer_seeded(
        resolver: &CallResolver<'_, '_>,
        object: u16,
        code: &[Instruction],
        stack: &ProcedureStack,
        seed: &Self,
    ) -> Self {
        let mut slots = seed.clone();
        for _ in 0..8 {
            let next = Self::infer_once(resolver, object, code, stack, &slots, seed);
            if next == slots {
                break;
            }
            slots = next;
        }
        slots
    }

    /// One inference pass from `seed`, resolving calls through the slots
    /// `known` so far.
    fn infer_once(
        resolver: &CallResolver<'_, '_>,
        object: u16,
        code: &[Instruction],
        stack: &ProcedureStack,
        known: &Self,
        seed: &Self,
    ) -> Self {
        let mut found = seed.clone();
        let pool = VbObject::parse(resolver.project, object)
            .ok()
            .and_then(|o| o.constants_pool().ok());
        let frame_operand = |insn: &Instruction| match insn.operands.first() {
            Some(Some(Operand::StackVar(offset))) => Some(*offset),
            _ => None,
        };
        let is_store = |insn: &Instruction| {
            matches!(
                insn.info.mnemonic,
                "FStAdFunc" | "FStAdFuncNoPop" | "FStAd" | "FStAdNoPop"
            )
        };
        let mut copies = Vec::new();
        let mut sites: Vec<(i16, u16, StoredValue)> = Vec::new();
        // Pr at each instruction: the simulation's where it reached, else
        // the last Pr load before it in code order (a cut hides the rest of
        // a path, and `FLdPr slot; VCall*` are adjacent).
        let mut last_load: Option<PrSource> = None;
        for (index, insn) in code.iter().enumerate() {
            let pr = match stack.instructions.get(index) {
                Some(InstructionStack::Reached { pr, .. } | InstructionStack::Cut { pr, .. }) => {
                    *pr
                }
                _ => last_load,
            };
            if let Some(source) = insn.pr_source() {
                last_load = Some(source);
            }
            let call = resolver.resolve_with_slots(object, insn, pr.as_ref(), known);
            let returned = call.as_ref().and_then(|call| {
                resolver
                    .returned_interface(call)
                    .or_else(|| known.element_at(call, pr.as_ref()))
            });
            // A call that returns through its [retval] pointer fills the slot
            // whose address it consumes last (pushed first).
            if let (Some(iid), Some(call)) = (returned, call.as_ref())
                && insn.info.pushes == 0
                && let Some(slot) = Self::retval_slot(code, stack, index, call)
            {
                found.add(slot, iid);
                sites.push((slot, insn.offset, StoredValue::Known(iid)));
            }
            // A VCallHresult on a Pr loaded from a slot names its interface.
            if let Some(Operand::VTableRef { interface, .. }) =
                insn.operands.first().copied().flatten()
                && insn.info.receiver == Receiver::Pr
                && let Some(PrSource::Frame { offset }) = pr
                && let Some(iid) = pool
                    .as_ref()
                    .and_then(|p| p.guid_at(interface).ok().flatten())
            {
                found.add(offset, iid);
            }
            let Some(next) = code.get(index.wrapping_add(1)) else {
                continue;
            };
            let Some(target) = frame_operand(next).filter(|_| is_store(next)) else {
                continue;
            };
            if insn.info.pushes != 0
                && let Some(class) = call
                    .as_ref()
                    .and_then(|call| resolver.returned_elements(call))
            {
                found
                    .elements
                    .entry(target)
                    .and_modify(|known| {
                        if *known != Some(class) {
                            *known = None;
                        }
                    })
                    .or_insert(Some(class));
            }
            let stored = match insn.info.mnemonic {
                "CastAd" => match insn.operands.first() {
                    Some(Some(Operand::ConstPoolIndex(entry))) => pool
                        .as_ref()
                        .and_then(|p| p.guid_at(*entry).ok().flatten())
                        .map_or(StoredValue::Unknown, StoredValue::Known),
                    _ => StoredValue::Unknown,
                },
                "ILdRf" | "ILdAd" | "FLdZeroAd" => match frame_operand(insn) {
                    Some(source) => {
                        copies.push((source, target));
                        StoredValue::Copy(source)
                    }
                    None => StoredValue::Unknown,
                },
                // A call that pushes the object it returns, stored.
                _ => match returned {
                    Some(iid) if insn.info.pushes != 0 => StoredValue::Known(iid),
                    _ => StoredValue::Unknown,
                },
            };
            if let StoredValue::Known(iid) = stored {
                found.add(target, iid);
            }
            sites.push((target, next.offset, stored));
        }
        // Copies carry an interface both ways, to a fixed point.
        let mut changed = true;
        while changed {
            changed = false;
            for &(a, b) in &copies {
                for (from, to) in [(a, b), (b, a)] {
                    if let Some(&iid) = found.slots.get(&from)
                        && !found.conflicting.contains(&to)
                        && found.slots.get(&to) != Some(&iid)
                    {
                        found.add(to, iid);
                        changed = true;
                    }
                }
            }
        }
        for (slot, offset, stored) in sites {
            let iid = match stored {
                StoredValue::Known(iid) => Some(iid),
                StoredValue::Copy(source) => found.get(source),
                StoredValue::Unknown => None,
            };
            found.stores.entry(slot).or_default().insert(offset, iid);
        }
        found
    }

    /// The frame slot whose address the call at `index` takes as its
    /// `[out, retval]` pointer: the value its last popped argument is
    /// (`FLdRfVar slot` pushed it).
    fn retval_slot(
        code: &[Instruction],
        stack: &ProcedureStack,
        index: usize,
        call: &CallSignature<'_>,
    ) -> Option<i16> {
        let arguments = call.arg_widths.as_ref()?.len();
        let last = arguments.checked_sub(1)?;
        let insn = Self::pusher(code, stack, index, last)?;
        match (insn.info.mnemonic, insn.operands.first()) {
            ("FLdRfVar" | "FLdRf", Some(Some(Operand::StackVar(offset)))) => Some(*offset),
            _ => None,
        }
    }

    /// The instruction that last set the value `from_top` values below the
    /// top of the evaluation stack as the instruction at `index` finds it,
    /// from the simulated depths: the latest one before it that popped down
    /// to that position or below and pushed it (`CastAd`, which replaces the
    /// value, rather than the load before it). `None` if that instruction
    /// pushed nothing there or was not simulated.
    fn pusher<'c>(
        code: &'c [Instruction],
        stack: &ProcedureStack,
        index: usize,
        from_top: usize,
    ) -> Option<&'c Instruction> {
        let depth = |at: usize| match stack.instructions.get(at) {
            Some(InstructionStack::Reached { entry, .. } | InstructionStack::Cut { entry, .. }) => {
                Some(entry.eval.len())
            }
            _ => None,
        };
        let position = depth(index)?.checked_sub(from_top)?.checked_sub(1)?;
        for at in (0..index).rev() {
            let Some(InstructionStack::Reached { entry, effect, .. }) = stack.instructions.get(at)
            else {
                return None;
            };
            let lowest = entry.eval.len().checked_sub(effect.popped.len())?;
            if lowest > position {
                continue;
            }
            // It reached the position: it set the value if it left the stack
            // just above it.
            return (depth(at.wrapping_add(1)) == Some(position.wrapping_add(1))
                && effect.pushed.is_some())
            .then(|| code.get(at))
            .flatten();
        }
        None
    }

    /// The arguments of the call at `index` that are the value of one of the
    /// caller's frame slots (pushed by `ILdRf slot`, or left by
    /// `FStAdNoPop slot` / `FStAdFuncNoPop slot`), each with the
    /// callee's parameter slot that receives it: `ebp+0x0C` plus the widths
    /// of the arguments popped before it.
    fn passed_slots(code: &[Instruction], stack: &ProcedureStack, index: usize) -> Vec<(i16, i16)> {
        let Some(InstructionStack::Reached { effect, .. }) = stack.instructions.get(index) else {
            return Vec::new();
        };
        let mut found = Vec::new();
        let mut offset: i16 = 0x0C;
        for (k, value) in effect.popped.iter().enumerate() {
            let StackValue::Eval(width) = *value else {
                break;
            };
            if width == 1
                && let Some(insn) = Self::pusher(code, stack, index, k)
                && matches!(
                    insn.info.mnemonic,
                    "ILdRf" | "ILdAd" | "FStAdNoPop" | "FStAdFuncNoPop"
                )
                && let Some(Some(Operand::StackVar(slot))) = insn.operands.first()
            {
                found.push((*slot, offset));
            }
            offset = offset.saturating_add(i16::from(width).saturating_mul(4));
        }
        found
    }

    /// The element a control array's `Item` returns when `call` is that
    /// `Item` ([`RuntimeInterfaces::CONTROL_ARRAY_ITEM`]) on an array held
    /// in the frame slot Pr was loaded from.
    fn element_at(&self, call: &CallSignature<'_>, pr: Option<&PrSource>) -> Option<Guid> {
        let Callee::Interface {
            iid, vtable_offset, ..
        } = &call.callee
        else {
            return None;
        };
        let Some(PrSource::Frame { offset }) = pr else {
            return None;
        };
        if iid.bytes != [0; 16] || *vtable_offset != RuntimeInterfaces::CONTROL_ARRAY_ITEM {
            return None;
        }
        self.elements.get(offset).copied().flatten()
    }

    /// Records `iid` for slot `offset`; a second, different one makes the
    /// slot conflicting.
    fn add(&mut self, offset: i16, iid: Guid) {
        if self.conflicting.contains(&offset) {
            return;
        }
        match self.slots.get(&offset) {
            Some(&known) if known != iid => {
                self.slots.remove(&offset);
                self.conflicting.insert(offset);
            }
            _ => {
                self.slots.insert(offset, iid);
            }
        }
    }

    /// Returns the interface known for frame slot `ebp + offset`.
    pub fn get(&self, offset: i16) -> Option<Guid> {
        self.slots.get(&offset).copied()
    }

    /// Returns the interface of the object Pr holds when it was loaded
    /// from a frame slot ([`PrSource::Frame`]).
    pub fn for_pr(&self, pr: Option<&PrSource>) -> Option<Guid> {
        match pr {
            Some(PrSource::Frame { offset }) => self.get(*offset),
            _ => None,
        }
    }

    /// Returns the interface of the object Pr holds at the instruction at
    /// P-Code offset `at`, when it was loaded from a frame slot: the slot's
    /// own interface ([`for_pr`](Self::for_pr)), or for a slot that holds
    /// objects of several interfaces (a temporary the compiler reuses), the
    /// interface of the latest store into it before `at`.
    pub fn for_pr_at(&self, pr: Option<&PrSource>, at: u16) -> Option<Guid> {
        let Some(PrSource::Frame { offset }) = pr else {
            return None;
        };
        if let Some(iid) = self.get(*offset) {
            return Some(iid);
        }
        if !self.conflicting.contains(offset) {
            return None;
        }
        self.stores
            .get(offset)?
            .range(..at)
            .next_back()
            .and_then(|(_, iid)| *iid)
    }

    /// Returns the slots with a known interface, by offset.
    pub fn iter(&self) -> impl Iterator<Item = (i16, Guid)> + '_ {
        self.slots.iter().map(|(&offset, &iid)| (offset, iid))
    }
}

/// The frame-slot interfaces of every P-Code procedure of a project
/// ([`SlotInterfaces`]), with the interfaces arguments carry between a
/// caller's slot and the callee's parameter.
///
/// A procedure's own code may not state the interface of a parameter it
/// only makes typed `VCall*` calls on, nor a caller that of the variable it
/// passes (`tests/fixtures/vtable`: `Sub Main` passes `r`, which its code
/// only compares with `Nothing`, to `Vtable(ByVal r As IRaw)`, whose
/// `VCallHresult` names `IRaw`). An argument the caller pushed from a frame
/// slot (`ILdRf slot`) is the object the callee's parameter slot holds
/// (`ebp+0x0C` plus the widths of the arguments before it), so the
/// interface either one's code states is the other's. A `ByRef` argument
/// (`FLdRfVar slot`, the slot's address) carries none.
#[derive(Debug, Clone, Default)]
pub struct ProjectSlots {
    procedures: HashMap<(u16, u16), SlotInterfaces>,
}

impl ProjectSlots {
    /// Returns the slot interfaces of method `method` (its method-table
    /// index) of object `object`.
    pub fn get(&self, object: u16, method: u16) -> Option<&SlotInterfaces> {
        self.procedures.get(&(object, method))
    }

    /// Returns each P-Code procedure's slot interfaces, by object and
    /// method-table index.
    pub fn iter(&self) -> impl Iterator<Item = ((u16, u16), &SlotInterfaces)> + '_ {
        self.procedures.iter().map(|(&key, slots)| (key, slots))
    }
}

/// Resolves the call opcodes of a project's procedures to [`CallSignature`]s.
///
/// Built once per project ([`CallResolver::new`]); [`resolve`](Self::resolve)
/// then answers per instruction.
pub struct CallResolver<'a, 'p> {
    project: &'p VbProject<'a>,
    objects: Vec<ObjectFacts<'a>>,
    /// `(object, method)` by the VA of the method's ProcDscInfo.
    by_proc_dsc: HashMap<u32, (u16, u16)>,
    /// The descriptions of external interfaces, if supplied.
    interfaces: Option<Arc<dyn InterfaceCatalog + Send + Sync>>,
    /// The default interfaces of the classes the project hosts as controls
    /// ([`VbProject::components`]): an object of one, reached through its
    /// control getter, is the control's extender.
    hosted: Vec<Guid>,
}

/// How a member of an interface is invoked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InvokeKind {
    /// A method.
    Method,
    /// A property get.
    Get,
    /// A property let.
    Let,
    /// A property set (`Set` of an object property).
    Set,
}

impl InvokeKind {
    /// How the late-bound call `mnemonic` invokes its member: a store
    /// (`LateIdSt`, `LateIdCallSt`, `LateMemSt` ...) lets a property, a
    /// store of an object (`*StAd`) sets one, every other form (`LateIdCall`,
    /// `LateIdLdVar`, `LateIdCallLdVar` ...) calls a method or gets a
    /// property, which [`admits`](Self::admits) both stand for.
    pub fn of_late_call(mnemonic: &str) -> Self {
        if mnemonic.ends_with("StAd") {
            Self::Set
        } else if mnemonic.ends_with("St") {
            Self::Let
        } else {
            Self::Method
        }
    }

    /// The prefix COM gives the function of a property accessor invoked
    /// this way (`get_`, `put_`, `putref_`); none for a method.
    pub fn accessor_prefix(self) -> &'static str {
        match self {
            Self::Method => "",
            Self::Get => "get_",
            Self::Let => "put_",
            Self::Set => "putref_",
        }
    }

    /// Returns whether a call invoking this way reaches a member invoked
    /// as `member`: the same kind, or for a call, a method or a property
    /// get ([`of_late_call`](Self::of_late_call)).
    pub fn admits(self, member: Self) -> bool {
        match self {
            Self::Method => matches!(member, Self::Method | Self::Get),
            _ => self == member,
        }
    }
}

/// A member of an external interface, as its type library describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceMember {
    /// The interface's name (e.g. `"_TextBox"`).
    pub interface: String,
    /// The member's name.
    pub name: String,
    /// How it is invoked.
    pub invoke: InvokeKind,
    /// Its DISPID, if the type library gives one.
    pub dispid: Option<i32>,
    /// Each argument's width in evaluation-stack slots, in the order the
    /// call pops them (see [`CallSignature::arg_widths`]): parameter order,
    /// the `[out, retval]` pointer last; `this` not included.
    pub arg_widths: Vec<u8>,
    /// `true` if the member returns a Single, Double or Date on the x87
    /// stack (a non-`HRESULT` member).
    pub float_result: bool,
    /// The IID of the object the member returns, through its return value
    /// or its `[out, retval]` pointer, when the type library names it:
    /// [`SlotInterfaces`] gives it to the frame slot the result is stored
    /// in.
    pub returns_interface: Option<Guid>,
}

/// The descriptions of interfaces the executable uses but does not
/// describe: the intrinsic controls and forms of `VB6.OLB`, the runtime's
/// objects (`Collection`, `ErrObject`), ActiveX controls and referenced type
/// libraries. A caller that has them (from the type libraries) supplies them
/// to [`CallResolver::with_interfaces`]. The runtime's interfaces that no
/// type library describes are the crate's own ([`RuntimeInterfaces`]) and
/// take precedence.
pub trait InterfaceCatalog {
    /// Returns the members at `vtable_offset` of interface `iid`: one
    /// function, which a type library may list under several names (a
    /// property's `Let` and `Set`, a `_Default` alias), all with the same
    /// arguments. Empty when the interface or the slot is unknown.
    fn members(&self, iid: &Guid, vtable_offset: u16) -> Vec<InterfaceMember>;

    /// Returns the members of interface `iid` with DISPID `dispid`, which a
    /// late-bound call names it by: a property's get and its let or set
    /// share one. Empty when the interface or the DISPID is unknown, which
    /// it is unless the catalog overrides this.
    fn dispatched(&self, _iid: &Guid, _dispid: i32) -> Vec<InterfaceMember> {
        Vec::new()
    }
}

/// The runtime's interfaces that no type library describes, built into the
/// crate and consulted by every [`CallResolver`] before a caller's
/// [`InterfaceCatalog`].
///
/// One interface: the control-array object (the all-zero IID), which a
/// form's or UserControl's getter for a control array returns
/// ([`Callee::Control`]). Its vtable is the runtime's (MSVBVM60 6.00.8176,
/// 0x66036758, read from a live object): after `IDispatch`, slot 0x1C
/// returns `0x800A01A9` (error 425, "Invalid object use") taking no
/// argument, 0x20-0x34 return the same error taking one, 0x38 and 0x3C do
/// nothing; the members are `Item` (0x40: an `Integer` index and the return
/// pointer), `LBound`, `UBound` and `Count` (0x44-0x4C: the return pointer,
/// an `Integer` written through it). Only those four are described.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RuntimeInterfaces;

impl RuntimeInterfaces {
    /// The name given to the control-array object's interface.
    pub const CONTROL_ARRAY: &'static str = "ControlArray";

    /// The vtable offset of the control-array object's `Item`.
    pub const CONTROL_ARRAY_ITEM: u16 = 0x40;

    /// The VBA library's `_ErrObject` interface,
    /// `{A4C466B8-499F-101B-BB78-00AA00383CBB}` (`Err.Raise` at 0x44,
    /// `Err.Clear` at 0x48).
    pub const ERR_OBJECT: Guid = Guid {
        bytes: [
            0xB8, 0x66, 0xC4, 0xA4, 0x9F, 0x49, 0x1B, 0x10, 0xBB, 0x78, 0x00, 0xAA, 0x00, 0x38,
            0x3C, 0xBB,
        ],
    };

    /// VB's `_VBControlExtender` interface,
    /// `{164CBDD0-7321-11D1-A1E8-00A0C90F2731}` (`VB6.OLB`): the properties
    /// and methods the extender of a hosted control adds to the control's
    /// own (`Name`, `Left`, `Visible`, `Tag`, `SetFocus`, `Move`, ...).
    pub const CONTROL_EXTENDER: Guid = Guid {
        bytes: [
            0xD0, 0xBD, 0x4C, 0x16, 0x21, 0x73, 0xD1, 0x11, 0xA1, 0xE8, 0x00, 0xA0, 0xC9, 0x0F,
            0x27, 0x31,
        ],
    };

    /// The first DISPID the compiler calls an extender member by.
    const EXTENDER_FIRST: u32 = 0x8001_0000;

    /// One past the last DISPID the compiler calls an extender member by.
    const EXTENDER_END: u32 = 0x8001_2000;

    /// How far above the DISPID a call names an extender member by
    /// `_VBControlExtender` declares it.
    const EXTENDER_SHIFT: u32 = 0x3000;

    /// Returns the DISPID [`CONTROL_EXTENDER`](Self::CONTROL_EXTENDER)
    /// declares for the extender member a late-bound call on a hosted
    /// control names by `dispid`, or `None` for a DISPID outside the
    /// extender's.
    ///
    /// The compiler calls the extender's members 0x3000 below the DISPIDs
    /// the interface declares: its properties from 0x80010000 (`Name`,
    /// declared 0x80013000), its methods from 0x80011000 (`SetFocus`,
    /// declared 0x80014000). Measured on every extender member
    /// `tests/fixtures/dispid` calls: the properties `Name`, `Left`, `Top`,
    /// `Width`, `Height`, `Visible`, `Parent`, `DragMode`, `Tag`,
    /// `TabIndex`, `object`, `HelpContextID`, `WhatsThisHelpID`,
    /// `Container`, `CausesValidation` and `ToolTipText`, and the methods
    /// `SetFocus`, `ZOrder`, `Move`, `Drag` and `ShowWhatsThis`.
    pub fn extender_member(dispid: i32) -> Option<i32> {
        let raw = dispid.cast_unsigned();
        (Self::EXTENDER_FIRST..Self::EXTENDER_END)
            .contains(&raw)
            .then(|| raw.wrapping_add(Self::EXTENDER_SHIFT).cast_signed())
    }

    /// Returns the interface of the object a runtime export returns, for
    /// the exports that return one fixed interface: `rtcErrObj` (`Err`)
    /// returns `_ErrObject` (`tests/fixtures/flow`: its `VCallHresult`s on
    /// that object name `_ErrObject`).
    pub fn import_return(function: &str) -> Option<Guid> {
        (function == "rtcErrObj").then_some(Self::ERR_OBJECT)
    }
}

impl InterfaceCatalog for RuntimeInterfaces {
    fn members(&self, iid: &Guid, vtable_offset: u16) -> Vec<InterfaceMember> {
        if iid.bytes != [0; 16] {
            return Vec::new();
        }
        let (name, invoke, arg_widths) = match vtable_offset {
            Self::CONTROL_ARRAY_ITEM => ("Item", InvokeKind::Get, vec![1, 1]),
            0x44 => ("LBound", InvokeKind::Get, vec![1]),
            0x48 => ("UBound", InvokeKind::Get, vec![1]),
            0x4C => ("Count", InvokeKind::Get, vec![1]),
            _ => return Vec::new(),
        };
        vec![InterfaceMember {
            interface: Self::CONTROL_ARRAY.to_string(),
            name: name.to_string(),
            invoke,
            dispid: None,
            arg_widths,
            float_result: false,
            returns_interface: None,
        }]
    }
}

impl<'a, 'p: 'a> CallResolver<'a, 'p> {
    /// Collects the project's objects, methods, prototypes and vtable slots.
    ///
    /// The slots come from each object's method link table (see the module
    /// documentation); an object without one falls back to its publics at
    /// their [`FuncTypDesc`] offsets, then, for a class, its other methods in
    /// method-table order.
    ///
    /// # Errors
    ///
    /// Returns an error if the project's object table cannot be read.
    pub fn new(project: &'p VbProject<'a>) -> Result<Self, Error> {
        // The hosted classes' default interfaces, by ProgID.
        let classes: HashMap<String, Guid> = project
            .components()
            .map(|components| {
                components
                    .filter_map(|component| {
                        Some((component.prog_id().into_owned(), component.default_iid()?))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut objects = Vec::new();
        let mut by_proc_dsc = HashMap::new();
        for (object_index, object) in project.objects()?.enumerate() {
            let object_index = u16::try_from(object_index).unwrap_or(u16::MAX);
            objects.push(match object {
                Ok(object) => Self::object_facts(&object, object_index, &classes, &mut by_proc_dsc),
                Err(_) => ObjectFacts::default(),
            });
        }
        Ok(Self {
            project,
            objects,
            by_proc_dsc,
            interfaces: None,
            hosted: classes.into_values().collect(),
        })
    }

    /// Supplies the descriptions of external interfaces: a call that reaches
    /// one ([`Callee::External`]) then resolves to [`Callee::Interface`]
    /// with the member's argument widths. [`RuntimeInterfaces`] is
    /// consulted first.
    #[must_use]
    pub fn with_interfaces(mut self, catalog: Arc<dyn InterfaceCatalog + Send + Sync>) -> Self {
        self.interfaces = Some(catalog);
        self
    }

    /// The signature of member `vtable_offset` of external interface `iid`:
    /// from [`RuntimeInterfaces`] or the caller's catalog when one describes
    /// it, else [`Callee::External`].
    fn external(&self, iid: Guid, vtable_offset: u16) -> CallSignature<'a> {
        let mut members = RuntimeInterfaces.members(&iid, vtable_offset);
        if members.is_empty()
            && let Some(catalog) = &self.interfaces
        {
            members = catalog.members(&iid, vtable_offset);
        }
        let Some(first) = members.first() else {
            return CallSignature {
                callee: Callee::External { iid, vtable_offset },
                ..Self::unknown(Some(vtable_offset))
            };
        };
        let arg_widths = first.arg_widths.clone();
        CallSignature {
            arg_slots: Some(arg_widths.iter().map(|&w| u16::from(w)).sum()),
            float_result: Some(first.float_result),
            arg_widths: Some(arg_widths),
            signature: None,
            callee: Callee::Interface {
                iid,
                vtable_offset,
                members,
            },
        }
    }

    /// Collects one object's facts, recording its methods' ProcDscInfo VAs.
    /// `classes` gives the default interface of each class the project
    /// hosts as a control, by ProgID.
    fn object_facts(
        object: &VbObject<'a, 'p>,
        object_index: u16,
        classes: &HashMap<String, Guid>,
        by_proc_dsc: &mut HashMap<u32, (u16, u16)>,
    ) -> ObjectFacts<'a> {
        let mut iids: Vec<Guid> = object.default_iids().map(|(_, guid)| guid).collect();
        iids.extend(Self::own_interface(object, object_index));
        let mut ftds: HashMap<u32, FuncTypDesc<'a>> = HashMap::new();
        if let Ok(iter) = object.func_type_descs() {
            ftds.extend(iter);
        }
        let mut methods = Vec::new();
        let mut method_by_dsc = HashMap::new();
        if let Ok(iter) = object.methods() {
            for (index, entry) in iter.enumerate() {
                let index_u16 = u16::try_from(index).unwrap_or(u16::MAX);
                let ftd = u32::try_from(index)
                    .ok()
                    .and_then(|i| ftds.get(&i).copied());
                let (arg_slots, float_result) = match &entry {
                    Ok(MethodEntry::PCode(method)) => {
                        by_proc_dsc.insert(method.proc_dsc_va(), (object_index, index_u16));
                        method_by_dsc.insert(method.proc_dsc_va(), index_u16);
                        let slots = method
                            .proc_dsc()
                            .arg_size()
                            .ok()
                            .map(|bytes| (bytes / 4).saturating_sub(1));
                        let float = method.instructions().ok().map(|iter| {
                            iter.flatten()
                                .any(|i| matches!(i.info.mnemonic, "ExitProcR4" | "ExitProcR8"))
                        });
                        (slots, float)
                    }
                    _ => (None, None),
                };
                methods.push(MethodFacts {
                    ftd,
                    arg_slots,
                    float_result,
                });
            }
        }

        // The designer's built-in interface and the control getters.
        let builtin = object
            .designer()
            .map(|designer| (designer.interface_iid(), designer.builtin_vtable_size()));
        let mut controls = HashMap::new();
        let mut named = Vec::new();
        if let Ok(iter) = object.controls() {
            for control in iter.flatten() {
                match control.index() {
                    Ok(0xFFFF) | Err(_) => {}
                    Ok(index) => named.push((index, control.name().into_owned())),
                }
            }
        }
        // A control with no event handler has no ControlInfo; the form data
        // names every control by the same index (`activex`
        // `CommonDialog1`, getter 0x318).
        let form_data = object.project().gui_entries().ok().and_then(|mut entries| {
            entries.find_map(|entry| {
                (entry.object_index().ok()? == u32::from(object_index))
                    .then(|| object.form_data_from_gui_entry(&entry))
                    .flatten()
            })
        });
        // A hosted control's record names its class. A control array has a
        // record per element, each with its array index, and its getter
        // returns the array.
        let mut hosted: HashMap<u16, (Option<Guid>, bool)> = HashMap::new();
        if let Some(form_data) = form_data {
            for record in form_data.controls() {
                let index = u16::from(record.cid());
                if !named.iter().any(|(known, _)| *known == index) {
                    named.push((index, record.name().into_owned()));
                }
                let class = record
                    .prog_id()
                    .and_then(|prog_id| classes.get(prog_id.as_ref()).copied());
                let array = record.array_index().is_some();
                hosted
                    .entry(index)
                    .and_modify(|(known, in_array)| {
                        if *known != class {
                            *known = None;
                        }
                        *in_array |= array;
                    })
                    .or_insert((class, array));
            }
        }
        if let Some((_, size)) = builtin {
            for (index, name) in named {
                if let Some(offset) = index.checked_mul(4).and_then(|o| o.checked_add(size)) {
                    let (interface, elements) = match hosted.get(&index) {
                        Some(&(class, false)) => (class, None),
                        Some(&(class, true)) => (None, class),
                        None => (None, None),
                    };
                    controls.insert(
                        offset,
                        ControlGetter {
                            index,
                            name,
                            interface,
                            elements,
                        },
                    );
                }
            }
        }

        // The object's own slots, in method-link order.
        let links = Self::links(object);
        let mut by_vtable = HashMap::new();
        let mut variables = HashMap::new();
        // The base: a public method's FuncTypDesc offset less its link
        // position, else the slot after `IDispatch`'s 7 and the inherited
        // ones (OptionalObjectInfo +0x2A), as the runtime lays it out.
        let base = links
            .iter()
            .enumerate()
            .find_map(|(k, link)| {
                let Link::Procedure(dsc) = link else {
                    return None;
                };
                let method = method_by_dsc.get(dsc)?;
                let offset = methods
                    .get(usize::from(*method))?
                    .ftd?
                    .vtable_offset()
                    .ok()?;
                offset.checked_sub(u16::try_from(k).ok()?.checked_mul(4)?)
            })
            .or_else(|| {
                let inherited = object
                    .optional_info()
                    .and_then(|opt| opt.inherited_vtable_slots().ok())
                    .unwrap_or(0);
                inherited.checked_mul(4)?.checked_add(CLASS_USER_BASE)
            });
        if let Some(base) = base
            && !links.is_empty()
        {
            for (k, link) in links.into_iter().enumerate() {
                let Some(offset) = u16::try_from(k)
                    .ok()
                    .and_then(|k| k.checked_mul(4))
                    .and_then(|o| o.checked_add(base))
                else {
                    break;
                };
                match link {
                    Link::Procedure(dsc) => {
                        if let Some(&method) = method_by_dsc.get(&dsc) {
                            by_vtable.insert(offset, method);
                        }
                    }
                    Link::Variable {
                        offset: field,
                        function,
                    } => {
                        variables.insert(offset, (field, function));
                    }
                    Link::Empty | Link::Other => {}
                }
            }
        } else {
            // No link table: the publics at their FuncTypDesc offsets, then a
            // class's other methods in method-table order.
            let mut last = None;
            for (index, method) in methods.iter().enumerate() {
                if let Some(offset) = method.ftd.and_then(|f| f.vtable_offset().ok()) {
                    by_vtable.insert(offset, u16::try_from(index).unwrap_or(u16::MAX));
                    last = last.max(Some(offset));
                }
            }
            if builtin.is_none()
                && let Some(mut offset) = last
            {
                for (index, method) in methods.iter().enumerate() {
                    if method.ftd.is_none() {
                        offset = offset.saturating_add(4);
                        by_vtable.insert(offset, u16::try_from(index).unwrap_or(u16::MAX));
                    }
                }
            }
        }
        ObjectFacts {
            iids,
            methods,
            by_vtable,
            controls,
            variables,
            builtin,
            info_va: object.descriptor().object_info_va().unwrap_or(0),
        }
    }

    /// The IID of a form's or UserControl's own full interface: the form
    /// data header's GUID at +0x15, found through the GUI table entry whose
    /// [`object_index`](crate::vb::guitable::GuiTableEntry::object_index) is
    /// the object's.
    fn own_interface(object: &VbObject<'a, 'p>, object_index: u16) -> Option<Guid> {
        object
            .project()
            .gui_entries()
            .ok()?
            .find(|entry| entry.object_index().ok() == Some(u32::from(object_index)))
            .and_then(|entry| object.form_data_from_gui_entry(&entry))
            .and_then(|data| data.header().secondary_guid())
    }

    /// Reads the object's method link table ([`VbObject::method_links`]),
    /// naming the runtime function each variable accessor jumps to.
    fn links(object: &VbObject<'a, 'p>) -> Vec<Link> {
        let project = object.project();
        let map = project.address_map();
        // The runtime function behind an import thunk (`jmp [IAT slot]`).
        let function = |thunk_va: u32| {
            map.slice_from_va(thunk_va, 6)
                .ok()
                .and_then(|code| match code {
                    [0xFF, 0x25, s0, s1, s2, s3, ..] => project
                        .imports()
                        .by_slot(u32::from_le_bytes([*s0, *s1, *s2, *s3])),
                    _ => None,
                })
                .map(|import| import.function().into_owned())
                .unwrap_or_default()
        };
        let Ok(links) = object.method_links() else {
            return Vec::new();
        };
        links
            .map(|link| match link.map(|link| link.kind) {
                Ok(MethodLinkKind::Empty) => Link::Empty,
                Ok(MethodLinkKind::Procedure { proc_dsc_va }) => Link::Procedure(proc_dsc_va),
                Ok(MethodLinkKind::Variable {
                    offset, thunk_va, ..
                }) => Link::Variable {
                    offset,
                    function: function(thunk_va),
                },
                Ok(MethodLinkKind::Jump | MethodLinkKind::Other) | Err(_) => Link::Other,
            })
            .collect()
    }

    /// Resolves the call `instruction` makes from a procedure of object
    /// `object` (an index of [`VbProject::objects`]).
    ///
    /// Returns `None` for an instruction that is not a call.
    pub fn resolve(&self, object: u16, instruction: &Instruction) -> Option<CallSignature<'a>> {
        let OpcodeSemantics::Call { kind } = instruction.info.semantics else {
            return None;
        };
        let pool = VbObject::parse(self.project, object)
            .ok()
            .and_then(|o| o.constants_pool().ok());
        let operands: Vec<Operand> = instruction.operands.iter().flatten().copied().collect();
        Some(match kind {
            CallKind::VCall | CallKind::ThisVCall => self.vtable_call(object, instruction, pool),
            CallKind::ImpAdCall => self.address_call(instruction, &operands, pool),
            CallKind::LateCall => Self::late_call(instruction, &operands, pool),
            CallKind::Event => Self::event_call(instruction, &operands),
            CallKind::Other => Self::unknown(None),
        })
    }

    /// A call nothing more is known of.
    fn unknown(vtable_offset: Option<u16>) -> CallSignature<'a> {
        CallSignature {
            callee: Callee::Unknown { vtable_offset },
            arg_slots: None,
            arg_widths: None,
            signature: None,
            float_result: None,
        }
    }

    /// The signature of method `method` of object `object`.
    fn procedure(&self, object: u16, method: u16) -> CallSignature<'a> {
        let facts = self
            .objects
            .get(usize::from(object))
            .and_then(|o| o.methods.get(usize::from(method)));
        let ftd = facts.and_then(|m| m.ftd);
        let arg_widths = ftd.and_then(|f| {
            let mut widths: Vec<u8> = f.arg_types().iter().map(|t| t.slots()).collect();
            widths.extend(f.return_type().map(ArgType::slots));
            (widths.len() == usize::from(f.entry_count())).then_some(widths)
        });
        let arg_slots = ftd
            .and_then(|f| f.arg_slots())
            .or_else(|| facts.and_then(|m| m.arg_slots));
        CallSignature {
            callee: Callee::Procedure { object, method },
            arg_slots,
            arg_widths,
            signature: ftd,
            float_result: facts.and_then(|m| m.float_result),
        }
    }

    /// Resolves the slot at `offset` in the vtable of project object
    /// `object`: the call a `VCall*` makes when its receiver (Pr) is known to
    /// be an instance of that object.
    ///
    /// The slot is one of the object's methods ([`Callee::Procedure`]), a
    /// control getter ([`Callee::Control`]), a member variable accessor
    /// ([`Callee::Variable`]) or, below a designer object's own slots, a
    /// member of its built-in interface ([`Callee::External`] with
    /// `_Form`'s, `_UserControl`'s ... IID). Returns [`Callee::Unknown`]
    /// otherwise.
    pub fn resolve_vtable(&self, object: u16, offset: u16) -> CallSignature<'a> {
        let Some(facts) = self.objects.get(usize::from(object)) else {
            return Self::unknown(Some(offset));
        };
        if let Some(&method) = facts.by_vtable.get(&offset) {
            return self.procedure(object, method);
        }
        if let Some(getter) = facts.controls.get(&offset) {
            return CallSignature {
                callee: Callee::Control {
                    object,
                    index: getter.index,
                    name: getter.name.clone(),
                },
                arg_slots: Some(0),
                arg_widths: Some(Vec::new()),
                signature: None,
                float_result: Some(false),
            };
        }
        if let Some((field, function)) = facts.variables.get(&offset) {
            // GetMem*: the [out, retval] pointer. PutMem8 (a Double, Date or
            // Currency): the 8-byte value, two slots (`ret 0xC` with
            // `this`). Every other PutMem* / SetMem*: one slot, the value or,
            // for a Variant, its address (`ret 8`; `tests/fixtures/members`).
            let width: u8 = if function == "PutMem8" { 2 } else { 1 };
            return CallSignature {
                callee: Callee::Variable {
                    object,
                    offset: *field,
                    function: function.clone(),
                },
                arg_slots: Some(u16::from(width)),
                arg_widths: Some(vec![width]),
                signature: None,
                float_result: Some(false),
            };
        }
        if let Some((iid, size)) = facts.builtin
            && offset < size
        {
            return self.external(iid, offset);
        }
        Self::unknown(Some(offset))
    }

    /// Resolves `instruction` like [`resolve`](Self::resolve), with what
    /// the caller knows of Pr, the object register: a `VCall*` whose Pr was
    /// loaded by `FLdPrThis` ([`PrSource::Me`]) or from `ebp+8` (`FLdPr` of
    /// the `Me` slot) calls the calling object's own vtable, such as the
    /// control getters of a form (`FLdPrThis`, `VCallAd 0x02FC`).
    pub fn resolve_with_pr(
        &self,
        object: u16,
        instruction: &Instruction,
        pr: Option<&PrSource>,
    ) -> Option<CallSignature<'a>> {
        let resolved = self.resolve(object, instruction)?;
        // `ebp+8` is `Me` in an object's method; in a module's procedure it
        // is the module's data block.
        let is_module = VbObject::parse(self.project, object)
            .ok()
            .and_then(|o| o.object_type_flags().ok())
            .is_none_or(|flags| flags.is_module());
        let is_me = match pr {
            Some(PrSource::Me) => true,
            Some(PrSource::Frame { offset: 8 }) => !is_module,
            _ => false,
        };
        match resolved.callee {
            Callee::Unknown {
                vtable_offset: Some(offset),
            } if is_me && instruction.info.receiver == Receiver::Pr => {
                Some(self.resolve_vtable(object, offset))
            }
            _ => Some(resolved),
        }
    }

    /// `VCall*` / `ThisVCall*`.
    fn vtable_call(
        &self,
        object: u16,
        instruction: &Instruction,
        pool: Option<ConstantPool<'a>>,
    ) -> CallSignature<'a> {
        let (offset, interface) = match instruction.operands.first() {
            Some(Some(Operand::VTableRef { offset, interface })) => (*offset, Some(*interface)),
            Some(Some(Operand::Int16(offset))) => (*offset as u16, None),
            _ => return Self::unknown(None),
        };
        // ThisVCall: Me is an instance of the calling object.
        if instruction.info.receiver == Receiver::Me {
            return self.resolve_vtable(object, offset);
        }
        // VCallHresult: the IID names the interface.
        let Some(iid) = interface.and_then(|i| pool.as_ref()?.guid_at(i).ok().flatten()) else {
            return Self::unknown(Some(offset));
        };
        self.interface_vtable(iid, offset)
    }

    /// Member `offset` of interface `iid`: a project object's vtable when the
    /// IID is one of its own, else an external interface.
    fn interface_vtable(&self, iid: Guid, offset: u16) -> CallSignature<'a> {
        match self
            .objects
            .iter()
            .position(|facts| facts.iids.contains(&iid))
        {
            Some(index) => self.resolve_vtable(u16::try_from(index).unwrap_or(u16::MAX), offset),
            None => self.external(iid, offset),
        }
    }

    /// Returns the interface of the object a resolved call returns: a
    /// catalog member's [`InterfaceMember::returns_interface`], a project
    /// method whose prototype returns one of the project's classes (that
    /// class's default interface), a runtime export with a fixed object
    /// return ([`RuntimeInterfaces::import_return`]), or the getter of a
    /// hosted control (its class's default interface: the getter returns
    /// the control's extender, whose members late-bound calls reach,
    /// [`resolve_with_slots`](Self::resolve_with_slots)).
    pub fn returned_interface(&self, call: &CallSignature<'_>) -> Option<Guid> {
        match &call.callee {
            Callee::Interface { members, .. } => members.first()?.returns_interface,
            Callee::Import { function, .. } => RuntimeInterfaces::import_return(function),
            Callee::Control { object, index, .. } => {
                self.objects
                    .get(usize::from(*object))?
                    .controls
                    .values()
                    .find(|getter| getter.index == *index)?
                    .interface
            }
            Callee::Procedure { .. } => {
                let returns = call.signature.as_ref()?.return_type()?;
                let info_va = returns
                    .descriptor_va()
                    .filter(|_| returns.code() & 0x5F == 0x13)?;
                self.objects
                    .iter()
                    .find(|facts| facts.info_va == info_va)?
                    .iids
                    .first()
                    .copied()
            }
            _ => None,
        }
    }

    /// Returns the interface of the elements of the control array a
    /// resolved call returns: the getter of a control array of a hosted
    /// class gives that class's default interface, which the array's `Item`
    /// returns ([`SlotInterfaces`] carries it from the slot holding the
    /// array to the one `Item` writes).
    pub fn returned_elements(&self, call: &CallSignature<'_>) -> Option<Guid> {
        let Callee::Control { object, index, .. } = &call.callee else {
            return None;
        };
        self.objects
            .get(usize::from(*object))?
            .controls
            .values()
            .find(|getter| getter.index == *index)?
            .elements
    }

    /// Resolves `instruction` like [`resolve_with_pr`](Self::resolve_with_pr),
    /// and through the interface `slots` know for the frame slot Pr was
    /// loaded from: a `VCall*` that carries no IID (the typed forms,
    /// `VCallUI1`, `VCallFPR8`, ...) to the member at its vtable offset, a
    /// `LateId*` to the member its DISPID names ([`LateTarget`]).
    pub fn resolve_with_slots(
        &self,
        object: u16,
        instruction: &Instruction,
        pr: Option<&PrSource>,
        slots: &SlotInterfaces,
    ) -> Option<CallSignature<'a>> {
        let mut resolved = self.resolve_with_pr(object, instruction, pr)?;
        if instruction.info.receiver != Receiver::Pr {
            return Some(resolved);
        }
        let Some(iid) = slots.for_pr_at(pr, instruction.offset) else {
            return Some(resolved);
        };
        match &mut resolved.callee {
            Callee::Unknown {
                vtable_offset: Some(offset),
            } => Some(self.interface_vtable(iid, *offset)),
            Callee::Late {
                dispid: Some(dispid),
                target,
                ..
            } => {
                *target = self.late_target(iid, *dispid, instruction.info.mnemonic);
                Some(resolved)
            }
            _ => Some(resolved),
        }
    }

    /// The member DISPID `dispid` names on an object of interface `iid`,
    /// invoked the way the late-bound call `mnemonic` invokes it
    /// ([`InvokeKind::of_late_call`]): a hosted control's extender member
    /// ([`RuntimeInterfaces::extender_member`], whichever class the control
    /// is), a project class's procedure whose prototype carries the DISPID,
    /// or a member the catalog describes. `None` when none is invoked that
    /// way.
    fn late_target(&self, iid: Guid, dispid: i32, mnemonic: &str) -> Option<LateTarget> {
        let invoke = InvokeKind::of_late_call(mnemonic);
        let (iid, dispid) = match RuntimeInterfaces::extender_member(dispid) {
            Some(declared) if self.hosted.contains(&iid) => {
                (RuntimeInterfaces::CONTROL_EXTENDER, declared)
            }
            _ => (iid, dispid),
        };
        if let Some(object) = self
            .objects
            .iter()
            .position(|facts| facts.iids.contains(&iid))
        {
            let methods = &self.objects.get(object)?.methods;
            return methods.iter().enumerate().find_map(|(method, facts)| {
                let ftd = facts.ftd?;
                let kind = match ftd.property_kind() {
                    PropertyKind::None => InvokeKind::Method,
                    PropertyKind::Get => InvokeKind::Get,
                    PropertyKind::Let => InvokeKind::Let,
                    PropertyKind::Set => InvokeKind::Set,
                    PropertyKind::Unknown(_) => return None,
                };
                (ftd.dispid().ok()? == dispid && invoke.admits(kind)).then(|| {
                    LateTarget::Procedure {
                        object: u16::try_from(object).unwrap_or(u16::MAX),
                        method: u16::try_from(method).unwrap_or(u16::MAX),
                        invoke: kind,
                    }
                })
            });
        }
        let members: Vec<InterfaceMember> = self
            .interfaces
            .as_ref()?
            .dispatched(&iid, dispid)
            .into_iter()
            .filter(|member| invoke.admits(member.invoke))
            .collect();
        (!members.is_empty()).then_some(LateTarget::Members {
            iid,
            dispid,
            members,
        })
    }

    /// Simulates a procedure's stacks ([`ProcedureStack::simulate`]) with
    /// the interfaces its frame slots hold: it alternates simulation and
    /// [`SlotInterfaces::infer`] until they agree (a resolved call can
    /// reach code a cut hid), at most four rounds.
    pub fn simulate_procedure(
        &self,
        object: u16,
        code: &[Instruction],
        bytes: &[u8],
    ) -> (ProcedureStack, SlotInterfaces) {
        self.simulate_procedure_seeded(object, code, bytes, &SlotInterfaces::default())
    }

    /// Simulates a procedure like
    /// [`simulate_procedure`](Self::simulate_procedure), starting from the
    /// slot interfaces `seed` gives ([`SlotInterfaces::infer_seeded`]): for
    /// instance a procedure's entry of [`ProjectSlots`].
    pub fn simulate_procedure_seeded(
        &self,
        object: u16,
        code: &[Instruction],
        bytes: &[u8],
        seed: &SlotInterfaces,
    ) -> (ProcedureStack, SlotInterfaces) {
        let mut slots = seed.clone();
        let mut stack = ProcedureStack::simulate(code, bytes, &|insn, pr| {
            self.resolve_with_slots(object, insn, pr, &slots)
        });
        for _ in 0..4 {
            let next = SlotInterfaces::infer_seeded(self, object, code, &stack, seed);
            if next == slots {
                break;
            }
            slots = next;
            stack = ProcedureStack::simulate(code, bytes, &|insn, pr| {
                self.resolve_with_slots(object, insn, pr, &slots)
            });
        }
        (stack, slots)
    }

    /// Infers the frame-slot interfaces of every P-Code procedure of the
    /// project ([`ProjectSlots`]): each procedure is simulated
    /// ([`simulate_procedure_seeded`](Self::simulate_procedure_seeded)), the
    /// interfaces its calls' arguments carry seed its callees' parameters
    /// and its callers' argument slots, and the procedures are simulated
    /// again until the seeds settle (at most eight rounds).
    pub fn infer_project_slots(&self) -> ProjectSlots {
        let mut procedures: Vec<(u16, u16, Vec<Instruction>, &'a [u8])> = Vec::new();
        if let Ok(objects) = self.project.objects() {
            for (object, entry) in objects.enumerate() {
                let (Ok(object), Ok(entry)) = (u16::try_from(object), entry) else {
                    continue;
                };
                let Ok(methods) = entry.methods() else {
                    continue;
                };
                for (method, method_entry) in methods.enumerate() {
                    if let (Ok(method), Ok(MethodEntry::PCode(pcode))) =
                        (u16::try_from(method), method_entry)
                        && let Ok(code) = pcode.instructions()
                    {
                        procedures.push((
                            object,
                            method,
                            code.flatten().collect(),
                            pcode.pcode_bytes(),
                        ));
                    }
                }
            }
        }
        let mut seeds: HashMap<(u16, u16), SlotInterfaces> = HashMap::new();
        let mut result = ProjectSlots::default();
        for _ in 0..8 {
            let simulated: Vec<(ProcedureStack, SlotInterfaces)> = procedures
                .iter()
                .map(|(object, method, code, bytes)| {
                    let seed = seeds.get(&(*object, *method)).cloned().unwrap_or_default();
                    self.simulate_procedure_seeded(*object, code, bytes, &seed)
                })
                .collect();
            result.procedures = procedures
                .iter()
                .zip(&simulated)
                .map(|((object, method, ..), (_, slots))| ((*object, *method), slots.clone()))
                .collect();
            let mut next: HashMap<(u16, u16), SlotInterfaces> = HashMap::new();
            for ((object, method, code, _), (stack, slots)) in procedures.iter().zip(&simulated) {
                let caller = (*object, *method);
                let mut last_load: Option<PrSource> = None;
                for (index, insn) in code.iter().enumerate() {
                    let pr = match stack.instructions.get(index) {
                        Some(
                            InstructionStack::Reached { pr, .. } | InstructionStack::Cut { pr, .. },
                        ) => *pr,
                        _ => last_load,
                    };
                    if let Some(source) = insn.pr_source() {
                        last_load = Some(source);
                    }
                    let Some(Callee::Procedure {
                        object: callee_object,
                        method: callee_method,
                    }) = self
                        .resolve_with_slots(*object, insn, pr.as_ref(), slots)
                        .map(|call| call.callee)
                    else {
                        continue;
                    };
                    let callee = (callee_object, callee_method);
                    for (slot, parameter) in SlotInterfaces::passed_slots(code, stack, index) {
                        if let Some(iid) = slots.get(slot) {
                            next.entry(callee).or_default().add(parameter, iid);
                        }
                        if let Some(iid) = result
                            .get(callee.0, callee.1)
                            .and_then(|s| s.get(parameter))
                        {
                            next.entry(caller).or_default().add(slot, iid);
                        }
                    }
                }
            }
            if next == seeds {
                break;
            }
            seeds = next;
        }
        result
    }

    /// `ImpAdCall*`: the procedure address in the pool, the exact byte count.
    fn address_call(
        &self,
        instruction: &Instruction,
        operands: &[Operand],
        pool: Option<ConstantPool<'a>>,
    ) -> CallSignature<'a> {
        let StackRule::ArgBytes { operand } = instruction.info.stack else {
            return Self::unknown(None);
        };
        let Some(&Operand::ExternalCall { import, arg_bytes }) = operands.get(usize::from(operand))
        else {
            return Self::unknown(None);
        };
        let arg_slots = Some(arg_bytes / 4);
        match pool.and_then(|p| p.entry_at(import).ok()) {
            Some(PoolEntry::Procedure { proc_dsc_va }) => {
                match self.by_proc_dsc.get(&proc_dsc_va) {
                    Some(&(object, method)) => CallSignature {
                        arg_slots,
                        ..self.procedure(object, method)
                    },
                    None => CallSignature {
                        arg_slots,
                        ..Self::unknown(None)
                    },
                }
            }
            Some(PoolEntry::Declare(stub)) => {
                let map = self.project.address_map();
                let text = |b: Result<&[u8], Error>| {
                    b.map(|b| String::from_utf8_lossy(b).into_owned())
                        .unwrap_or_default()
                };
                CallSignature {
                    callee: Callee::Declare {
                        library: text(stub.library_name_bytes(map)),
                        function: text(stub.function_name_bytes(map)),
                    },
                    arg_slots,
                    ..Self::unknown(None)
                }
            }
            Some(PoolEntry::Import { iat_va }) => match self.project.imports().by_slot(iat_va) {
                Some(import) => CallSignature {
                    callee: Callee::Import {
                        library: import.library.clone(),
                        function: import.function().into_owned(),
                        ordinal: match import.symbol {
                            ImportSymbol::Ordinal(ordinal) => Some(ordinal),
                            ImportSymbol::Name(_) => None,
                        },
                    },
                    arg_slots,
                    arg_widths: import.runtime_export().and_then(|export| {
                        let widths: Vec<u8> = export
                            .params
                            .iter()
                            .map(|param| match param.ty {
                                VbParamType::Double | VbParamType::Int64 => 2,
                                _ => 1,
                            })
                            .collect();
                        let known = export.params.iter().all(|p| p.ty != VbParamType::Unknown);
                        let slots: u16 = widths.iter().map(|&w| u16::from(w)).sum();
                        (known && Some(slots) == arg_slots).then_some(widths)
                    }),
                    float_result: import.runtime_export().and_then(|export| {
                        match export.return_type {
                            VbParamType::Float | VbParamType::Double => Some(true),
                            VbParamType::Unknown => None,
                            _ => Some(false),
                        }
                    }),
                    ..Self::unknown(None)
                },
                None => CallSignature {
                    arg_slots,
                    ..Self::unknown(None)
                },
            },
            _ => CallSignature {
                arg_slots,
                ..Self::unknown(None)
            },
        }
    }

    /// `RaiseEvent` (0x6610a847): the runtime calls the sinks with `Me`,
    /// the event id and the Variants by value the instruction pops.
    fn event_call(instruction: &Instruction, operands: &[Operand]) -> CallSignature<'a> {
        let id = operands.iter().find_map(|operand| match *operand {
            Operand::Int32(id) => Some(id),
            _ => None,
        });
        let argc = match instruction.info.stack {
            StackRule::Variants { operand } => match operands.get(usize::from(operand)) {
                Some(Operand::Int16(n)) => u16::try_from(*n).ok(),
                _ => None,
            },
            _ => None,
        };
        CallSignature {
            callee: id.map_or(
                Callee::Unknown {
                    vtable_offset: None,
                },
                |id| Callee::Event { id },
            ),
            arg_slots: argc.map(|n| n.saturating_mul(4)),
            arg_widths: argc.map(|n| vec![4; usize::from(n)]),
            signature: None,
            float_result: Some(false),
        }
    }

    /// `Late*`: Variants by value, four slots each.
    fn late_call(
        instruction: &Instruction,
        operands: &[Operand],
        pool: Option<ConstantPool<'a>>,
    ) -> CallSignature<'a> {
        let mut name = None;
        let mut dispid = None;
        for operand in operands {
            match *operand {
                Operand::ConstPoolIndex(index) if name.is_none() => {
                    name = pool.as_ref().and_then(|p| p.name_at(index).ok().flatten());
                }
                Operand::Int32(id) if dispid.is_none() => dispid = Some(id),
                _ => {}
            }
        }
        let argc = match instruction.info.stack {
            StackRule::Variants { operand } => match operands.get(usize::from(operand)) {
                Some(Operand::Int16(n)) => Some(*n as u16),
                _ => None,
            },
            _ => Some(0),
        };
        CallSignature {
            callee: Callee::Late {
                name,
                dispid,
                target: None,
            },
            arg_slots: argc.map(|n| n.saturating_mul(4)),
            arg_widths: argc.map(|n| vec![4; usize::from(n)]),
            signature: None,
            float_result: Some(false),
        }
    }
}

/// Formats as `LIB!Func` for API calls, `{iid}+0xOFFSET` for external
/// members, the member for late-bound calls.
impl fmt::Display for Callee {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Procedure { object, method } => write!(f, "object{object}.method{method}"),
            Self::Declare { library, function }
            | Self::Import {
                library, function, ..
            } => {
                write!(f, "{library}!{function}")
            }
            Self::External { iid, vtable_offset } => write!(f, "{iid}+0x{vtable_offset:04X}"),
            Self::Interface { members, .. } => match members.first() {
                Some(member) => write!(f, "{}.{}", member.interface, member.name),
                None => f.write_str("<interface>"),
            },
            Self::Control { object, name, .. } => write!(f, "object{object}.{name}"),
            Self::Variable {
                object,
                offset,
                function,
            } => write!(f, "object{object}.var_{offset:X}:{function}"),
            Self::Event { id } => write!(f, "event#{id}"),
            Self::Late {
                target: Some(LateTarget::Procedure { object, method, .. }),
                ..
            } => write!(f, "late:object{object}.method{method}"),
            Self::Late {
                target: Some(LateTarget::Members { members, .. }),
                ..
            } => match members.first() {
                Some(member) => write!(f, "late:{}.{}", member.interface, member.name),
                None => f.write_str("late:<interface>"),
            },
            Self::Late {
                name: Some(name), ..
            } => write!(f, "late:{name}"),
            Self::Late {
                dispid: Some(id), ..
            } => write!(f, "late:#{id}"),
            Self::Late { .. } => write!(f, "late:?"),
            Self::Unknown {
                vtable_offset: Some(o),
            } => write!(f, "vtbl+0x{o:04X}"),
            Self::Unknown {
                vtable_offset: None,
            } => write!(f, "<unknown>"),
        }
    }
}
