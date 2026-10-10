//! The resolvers, checked on the compiled fixtures of known source: the call
//! resolver, the constant pool's entry kinds, the frame resolver and the
//! import table.

#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]

mod common;

use std::collections::BTreeMap;

use visualbasic::{
    VbProject,
    imports::ImportSymbol,
    pcode::{
        calltarget::{CallResolver, CallSignature, Callee},
        decoder::Instruction,
        framevar::{FrameOwner, FrameResolver, FrameVar},
        stackeffect::{Pop, Push},
    },
    project::{MethodEntry, PCodeMethod},
    vb::constantpool::PoolEntry,
};

use crate::common::fixture;

/// Reads fixture `name`, leaked so its project outlives the test's borrows.
fn project(name: &str) -> &'static VbProject<'static> {
    let bytes: &'static [u8] = Box::leak(std::fs::read(fixture(name)).unwrap().into_boxed_slice());
    Box::leak(Box::new(VbProject::from_bytes(bytes).unwrap()))
}

/// Object `object`'s P-Code method `method`.
fn method(
    project: &'static VbProject<'static>,
    object: u16,
    method: usize,
) -> PCodeMethod<'static> {
    let object = project
        .objects()
        .unwrap()
        .nth(usize::from(object))
        .unwrap()
        .unwrap();
    match object.methods().unwrap().nth(method).unwrap().unwrap() {
        MethodEntry::PCode(method) => method,
        _ => panic!("not P-Code"),
    }
}

/// Each P-Code method of object `object`: its instructions and code bytes.
fn pcode_methods(
    project: &'static VbProject<'static>,
    object: u16,
) -> Vec<(Vec<Instruction>, &'static [u8])> {
    project
        .objects()
        .unwrap()
        .nth(usize::from(object))
        .unwrap()
        .unwrap()
        .methods()
        .unwrap()
        .filter_map(|entry| match entry {
            Ok(MethodEntry::PCode(method)) => Some((
                method.instructions().unwrap().flatten().collect(),
                method.pcode_bytes(),
            )),
            _ => None,
        })
        .collect()
}

/// The calls of a method with their resolved signatures, in order.
fn calls(
    project: &'static VbProject<'static>,
    resolver: &CallResolver<'static, 'static>,
    object: u16,
    index: usize,
) -> Vec<(Instruction, CallSignature<'static>)> {
    method(project, object, index)
        .instructions()
        .unwrap()
        .flatten()
        .filter_map(|insn| {
            let call = resolver.resolve(object, &insn)?;
            Some((insn, call))
        })
        .collect()
}

/// The index of method `name` of object `object`.
fn method_index(project: &'static VbProject<'static>, object: u16, name: &str) -> u16 {
    let object = project
        .objects()
        .unwrap()
        .nth(usize::from(object))
        .unwrap()
        .unwrap();
    (0..object.method_count().unwrap())
        .find(|&i| {
            object
                .method_name(i)
                .ok()
                .and_then(|n| n.as_str().map(|s| s.eq_ignore_ascii_case(name)))
                == Some(true)
        })
        .unwrap_or_else(|| panic!("no method {name}"))
}

/// `types`: `Sub Main` (Program #3) calls each `Kinds` method early-bound,
/// each module function, three `Declare`s; `Kinds.Opts` calls `IsMissing`.
#[test]
fn resolves_the_calls_of_types() {
    let project = project("types");
    let resolver = CallResolver::new(project).unwrap();
    let main = calls(project, &resolver, 0, 3);

    // `bo = k.B(True)`: Kinds.B, a Boolean and the return value's pointer.
    let (insn, b) = &main[0];
    assert_eq!(insn.info.mnemonic, "VCallHresult");
    let kinds_b = method_index(project, 1, "B");
    assert!(
        matches!(b.callee, Callee::Procedure { object: 1, method } if method == kinds_b),
        "{:?}",
        b.callee
    );
    assert_eq!(b.arg_widths.as_deref(), Some(&[1u8, 1][..]));
    assert_eq!(b.arg_slots, Some(2));
    // Every early-bound call of Main reaches a Kinds method.
    let vcalls: Vec<_> = main
        .iter()
        .filter(|(i, _)| i.info.mnemonic == "VCallHresult")
        .collect();
    assert!(vcalls.len() >= 20);
    for (insn, call) in &vcalls {
        assert!(
            matches!(call.callee, Callee::Procedure { object: 1, .. }),
            "{:04x}: {:?}",
            insn.offset,
            call.callee
        );
        assert!(insn.stack_effect(Some(call)).is_resolved());
    }
    // `k.R(2.25)` passes the Double in two slots.
    let r = method_index(project, 1, "R");
    let (_, r_call) = vcalls
        .iter()
        .find(|(_, c)| matches!(c.callee, Callee::Procedure { method, .. } if method == r))
        .unwrap();
    assert_eq!(r_call.arg_widths.as_deref(), Some(&[2u8, 1][..]));

    // The module functions: P-Code procedures of Program, a float result
    // from the Single, Double and Date ones.
    let module: Vec<_> = main
        .iter()
        .filter(|(_, c)| matches!(c.callee, Callee::Procedure { object: 0, .. }))
        .collect();
    assert_eq!(module.len(), 11, "MB .. MO");
    let floats = module
        .iter()
        .filter(|(_, c)| c.float_result == Some(true))
        .count();
    assert_eq!(floats, 3, "MD, MS, MR");

    // The Declares.
    let declares: Vec<String> = main
        .iter()
        .filter_map(|(_, c)| match &c.callee {
            Callee::Declare { library, function } => Some(format!("{library}!{function}")),
            _ => None,
        })
        .collect();
    assert_eq!(
        declares,
        ["kernel32!Sleep", "kernel32!MulDiv", "kernel32!lstrcmpA"]
    );
    let sleep = main
        .iter()
        .find(|(_, c)| matches!(&c.callee, Callee::Declare { function, .. } if function == "Sleep"))
        .unwrap();
    // `Sleep 0` compiles to the "FPR4" form, which pushes nothing; a Declare's
    // return type is not in the executable.
    assert_eq!(sleep.0.info.mnemonic, "ImpAdCallFPR4");
    let effect = sleep.0.stack_effect(Some(&sleep.1));
    assert_eq!(effect.popped, vec![Pop::Slots(1)]);
    assert_eq!(effect.pushed, Some(Push::MaybeX87));

    // `IsMissing(va)` in Kinds.Opts: the runtime's rtcIsMissing, by ordinal.
    let opts = method_index(project, 1, "Opts");
    let opts_calls = calls(project, &resolver, 1, usize::from(opts));
    assert!(
        opts_calls.iter().any(|(_, c)| matches!(
            &c.callee,
            Callee::Import { library, function, ordinal: Some(592) }
                if library == "MSVBVM60.DLL" && function == "rtcIsMissing"
        )),
        "{:?}",
        opts_calls
            .iter()
            .map(|(_, c)| &c.callee)
            .collect::<Vec<_>>()
    );
}

/// `calls`: Counter's private `Bump`, called through `Me` at vtable offset
/// 0x50 after its 13 public members.
#[test]
fn resolves_a_private_method_through_me() {
    let project = project("calls");
    let resolver = CallResolver::new(project).unwrap();
    // Private methods have no names in the executable: Bump is method 12.
    let bump = 12;
    let call = resolver.resolve_vtable(3, 0x50);
    assert!(matches!(call.callee, Callee::Procedure { object: 3, method } if method == bump));
    // A private method has no prototype: its arguments come from its
    // ProcDscInfo (one Long) and the return value's pointer is not counted.
    assert!(call.signature.is_none());
    assert_eq!(call.arg_slots, Some(2));
    assert!(matches!(
        resolver.resolve_vtable(3, 0x54).callee,
        Callee::Unknown {
            vtable_offset: Some(0x54)
        }
    ));
}

/// `controls`: the form's `VCallHresult`s on its TextBox and CommandButton
/// reach interfaces the executable does not describe.
#[test]
fn external_interfaces_stay_external() {
    let project = project("controls");
    let resolver = CallResolver::new(project).unwrap();
    let mut externals = 0;
    for (index, entry) in project
        .objects()
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .methods()
        .unwrap()
        .enumerate()
    {
        if !matches!(entry, Ok(MethodEntry::PCode(_))) {
            continue;
        }
        for (_, call) in calls(project, &resolver, 0, index) {
            if let Callee::External { vtable_offset, .. } = call.callee {
                externals += 1;
                assert_eq!(call.arg_slots, None);
                assert!(vtable_offset >= 0x1C);
            }
        }
    }
    assert!(externals > 0);
}

/// `late` and `dispid`: late-bound calls name their member, or give its
/// DISPID, and pass their arguments as Variants by value.
#[test]
fn resolves_late_bound_members() {
    let project = project("late");
    let resolver = CallResolver::new(project).unwrap();
    let names: Vec<String> = calls(project, &resolver, 0, 0)
        .iter()
        .filter_map(|(_, c)| match &c.callee {
            Callee::Late {
                name: Some(name), ..
            } => Some(name.clone()),
            _ => None,
        })
        .collect();
    for expected in ["Add", "Item", "Count", "Exists", "RemoveAll", "Owner"] {
        assert!(
            names.iter().any(|n| n == expected),
            "{expected} in {names:?}"
        );
    }

    let project = project_with_dispids();
    let resolver = CallResolver::new(project).unwrap();
    let mut dispids = 0;
    for (object_index, object) in project.objects().unwrap().enumerate() {
        let object_index = u16::try_from(object_index).unwrap();
        for (index, entry) in object.unwrap().methods().unwrap().enumerate() {
            if !matches!(entry, Ok(MethodEntry::PCode(_))) {
                continue;
            }
            for (insn, call) in calls(project, &resolver, object_index, index) {
                if let Callee::Late {
                    dispid: Some(_), ..
                } = call.callee
                {
                    dispids += 1;
                    let effect = insn.stack_effect(Some(&call));
                    assert!(
                        effect
                            .popped
                            .iter()
                            .all(|p| *p == Pop::Eval(4) || *p == Pop::Eval(1))
                    );
                }
            }
        }
    }
    assert!(dispids >= 10, "{dispids} calls by DISPID");
}

fn project_with_dispids() -> &'static VbProject<'static> {
    project("dispid")
}

/// The constant pool's entry kinds, as the opcodes naming them use them.
#[test]
fn pool_entries_match_their_opcodes() {
    let mut kinds: BTreeMap<(&'static str, &'static str), usize> = BTreeMap::new();
    for name in ["types", "calls", "exprs", "late", "data", "events"] {
        let project = project(name);
        for object in project.objects().unwrap() {
            let object = object.unwrap();
            let pool = object.constants_pool().unwrap();
            for entry in object.methods().unwrap() {
                let Ok(MethodEntry::PCode(method)) = entry else {
                    continue;
                };
                for insn in method.instructions().unwrap().flatten() {
                    let index = match insn.operands[0] {
                        Some(visualbasic::pcode::operand::Operand::ConstPoolIndex(i)) => i,
                        Some(visualbasic::pcode::operand::Operand::ExternalCall {
                            import, ..
                        }) => import,
                        _ => continue,
                    };
                    let family = match insn.info.mnemonic {
                        "New" => "New",
                        m if m.starts_with("ImpAdCall") => "ImpAdCall",
                        "LitStr" | "LitVarStr" => "LitStr",
                        _ => continue,
                    };
                    let kind = match pool.entry_at(index).unwrap_or_else(|e| {
                        panic!("{name} {:04x} {}: {e}", insn.offset, insn.info.mnemonic)
                    }) {
                        PoolEntry::Null => "null",
                        PoolEntry::String(_) => "string",
                        PoolEntry::Procedure { .. } => "procedure",
                        PoolEntry::Declare(_) => "declare",
                        PoolEntry::Import { .. } => "import",
                        PoolEntry::ObjectInfo { .. } => "object",
                        PoolEntry::Address(_) => "address",
                    };
                    *kinds.entry((family, kind)).or_default() += 1;
                }
            }
        }
    }
    println!("{kinds:?}");
    // `New` names an ObjectInfo, an `ImpAdCall*` a procedure, a Declare or an
    // import, a string literal a BSTR (or the empty string, an address).
    for (family, kind) in kinds.keys() {
        let ok = match *family {
            "New" => *kind == "object",
            "ImpAdCall" => matches!(*kind, "procedure" | "declare" | "import"),
            _ => matches!(*kind, "string" | "address"),
        };
        assert!(ok, "{family} names a {kind}: {kinds:?}");
    }
    assert!(kinds.contains_key(&("New", "object")));
    assert!(kinds.contains_key(&("ImpAdCall", "import")));
}

/// The frame resolver: a module procedure's arguments start at `ebp+0x0C`
/// after the module data slot; a public method's are named by its
/// prototype, the return value's pointer last.
#[test]
fn resolves_frame_offsets() {
    let project = project("types");
    let map = project.address_map();
    // Program.MR (a module function of a Double).
    let mr = method(project, 0, 9);
    let resolver = FrameResolver::new(mr.proc_dsc(), None, map);
    assert_eq!(resolver.owner(), FrameOwner::Module);
    assert!(matches!(
        resolver.resolve(0x0C),
        FrameVar::Argument { index: 0, .. }
    ));
    assert!(matches!(
        resolver.resolve(0x08),
        FrameVar::Housekeeping {
            name: "module_data"
        }
    ));

    // Kinds.R(ByVal x As Double) As Double.
    let kinds = project.objects().unwrap().nth(1).unwrap().unwrap();
    let r = method_index(project, 1, "R");
    let ftd = kinds
        .func_type_descs()
        .unwrap()
        .find(|(i, _)| *i == u32::from(r))
        .map(|(_, f)| f)
        .unwrap();
    let resolver = FrameResolver::new(
        method(project, 1, usize::from(r)).proc_dsc(),
        Some(&ftd),
        map,
    );
    assert_eq!(resolver.owner(), FrameOwner::Object);
    assert!(matches!(
        resolver.resolve(0x08),
        FrameVar::Housekeeping { name: "me" }
    ));
    assert!(matches!(
        resolver.resolve(0x0C),
        FrameVar::Argument { index: 0, name: Some(ref n), .. } if n == "x"
    ));
    assert!(matches!(
        resolver.resolve(0x14),
        FrameVar::ReturnValue { .. }
    ));
}

/// The import table names the runtime's ordinal imports.
#[test]
fn names_runtime_imports() {
    let project = project("data");
    let imports = project.imports();
    assert!(imports.len() > 10);
    let runtime: Vec<_> = imports
        .iter()
        .filter(|(_, import)| import.is_runtime())
        .collect();
    assert!(
        runtime
            .iter()
            .any(|(_, i)| matches!(i.symbol, ImportSymbol::Ordinal(_))
                && i.function() == "rtcRandomNext")
    );
    // Every runtime import has a name.
    for (va, import) in runtime {
        assert!(!import.function().starts_with('#'), "{va:#x}: {import:?}");
    }
}

/// Every instruction that loads Pr says where from, with its operands.
#[test]
fn every_pr_load_has_a_source() {
    use visualbasic::pcode::stackeffect::PrSource;
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    for name in crate::common::projects() {
        let project = project(&name);
        for object in project.objects().unwrap() {
            for entry in object.unwrap().methods().unwrap() {
                let Ok(MethodEntry::PCode(method)) = entry else {
                    continue;
                };
                for insn in method.instructions().unwrap().flatten() {
                    if !insn.info.writes_pr() {
                        assert!(insn.pr_source().is_none());
                        continue;
                    }
                    let source = insn
                        .pr_source()
                        .unwrap_or_else(|| panic!("{name}: {insn}: no source"));
                    if insn.info.mnemonic.starts_with("FLdPr") && insn.info.mnemonic != "FLdPrThis"
                    {
                        assert!(matches!(source, PrSource::Frame { .. }));
                    }
                    let kind = format!("{source:?}");
                    *kinds
                        .entry(kind.split([' ', '{']).next().unwrap().to_string())
                        .or_default() += 1;
                }
            }
        }
    }
    println!("{kinds:?}");
    for kind in [
        "Frame",
        "Me",
        "FrameMember",
        "PrMember",
        "ArrayElement",
        "NewIfNull",
    ] {
        assert!(kinds.contains_key(kind), "{kind} in {kinds:?}");
    }
}

/// A procedure's frame declarations: every slot its operands name, and the
/// cleanup table's types for the locals the runtime releases.
#[test]
fn declares_frame_slots() {
    use visualbasic::vb::controlprop::ControlPropertyType;
    let project = project("types");
    let main = method(project, 0, 3);
    let instructions: Vec<Instruction> = main.instructions().unwrap().flatten().collect();
    let resolver = FrameResolver::new(main.proc_dsc(), None, project.address_map());
    let declarations = resolver.declarations(&instructions, main.pcode_bytes());
    // Sorted, unique offsets.
    assert!(declarations.windows(2).all(|w| w[0].offset < w[1].offset));
    // `Dim al() As Long`: an array local the runtime destroys on exit.
    let array = declarations
        .iter()
        .find(|d| d.cleanup == Some(ControlPropertyType::Array))
        .unwrap();
    assert!(matches!(array.var, FrameVar::Local { .. }), "{array:?}");
    // Every slot an FFree* payload lists is declared.
    for insn in &instructions {
        for slot in insn.frame_slots(main.pcode_bytes()) {
            assert!(
                declarations.iter().any(|d| d.offset == slot),
                "{insn}: {slot}"
            );
        }
    }
    // `k` (var_88), a local.
    let k = declarations.iter().find(|d| d.offset == -0x88).unwrap();
    assert!(matches!(k.var, FrameVar::Local { frame_offset: 4 }));
}

/// `ProcDscInfo`'s option flags: `Friend`, and entry without a reference on
/// `Me`; error handling sets neither.
#[test]
fn procedure_option_flags() {
    let events = project("events");
    let ring = events
        .objects()
        .unwrap()
        .position(|o| o.unwrap().name().unwrap() == "Ring")
        .unwrap();
    let ring = u16::try_from(ring).unwrap();
    // Class_Initialize, then Class_Terminate; the Friend function Diameter.
    assert!(
        method(events, ring, 0)
            .proc_dsc()
            .enters_adjusted_me_as_primary()
    );
    assert!(
        !method(events, ring, 1)
            .proc_dsc()
            .enters_adjusted_me_as_primary()
    );
    assert!(method(events, ring, 4).proc_dsc().is_friend());

    let flow = project("flow");
    for entry in flow
        .objects()
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .methods()
        .unwrap()
    {
        let Ok(MethodEntry::PCode(method)) = entry else {
            continue;
        };
        assert_eq!(method.proc_dsc().proc_opt_flags_raw().unwrap(), 0);
    }
}

/// A procedure's error handling, from its code.
#[test]
fn summarizes_error_handling() {
    let flow = project("flow");
    let mut handlers = 0;
    let mut resume_next = 0;
    let mut resumes = 0;
    for entry in flow
        .objects()
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .methods()
        .unwrap()
    {
        let Ok(MethodEntry::PCode(method)) = entry else {
            continue;
        };
        let handling = method.error_handling().unwrap();
        handlers += usize::from(handling.has_handler());
        resume_next += usize::from(handling.resumes_next_on_error());
        resumes += usize::from(handling.resumes());
    }
    assert!(handlers >= 5, "{handlers}");
    assert!(resume_next >= 1, "{resume_next}");
    assert!(resumes >= 3, "{resumes}");
}

/// Every runtime function the fixtures call has the parameters its export
/// signature states: their bytes are the `ImpAdCall*`'s operand, and the
/// call's argument widths come from them.
#[test]
fn runtime_signatures_match_their_calls() {
    let mut checked = 0;
    for name in crate::common::projects() {
        let project = project(&name);
        let resolver = CallResolver::new(project).unwrap();
        for (object_index, object) in project.objects().unwrap().enumerate() {
            let object_index = u16::try_from(object_index).unwrap();
            for entry in object.unwrap().methods().unwrap() {
                let Ok(MethodEntry::PCode(method)) = entry else {
                    continue;
                };
                for insn in method.instructions().unwrap().flatten() {
                    let Some(call) = resolver.resolve(object_index, &insn) else {
                        continue;
                    };
                    let Callee::Import { function, .. } = &call.callee else {
                        continue;
                    };
                    let widths = call
                        .arg_widths
                        .as_ref()
                        .unwrap_or_else(|| panic!("{name} {insn}: {function} has no widths"));
                    let slots: u16 = widths.iter().map(|&w| u16::from(w)).sum();
                    assert_eq!(Some(slots), call.arg_slots, "{name} {insn}: {function}");
                    checked += 1;
                }
            }
        }
    }
    assert!(checked > 100, "{checked} runtime calls");
}

/// The index of the object named `name`.
fn object_index(project: &'static VbProject<'static>, name: &str) -> u16 {
    let index = project
        .objects()
        .unwrap()
        .position(|o| o.unwrap().name().unwrap() == name)
        .unwrap_or_else(|| panic!("no object {name}"));
    u16::try_from(index).unwrap()
}

/// Forms and UserControls: the vtable is the designer's built-in interface,
/// 256 control getters, then the object's own slots in method-link order.
#[test]
fn resolves_form_vtables() {
    use visualbasic::vb::designer::Designer;
    let forms = project("forms");
    let resolver = CallResolver::new(forms).unwrap();
    let board = object_index(forms, "Board");
    let gauge = object_index(forms, "Gauge");
    let board_object = forms
        .objects()
        .unwrap()
        .nth(usize::from(board))
        .unwrap()
        .unwrap();
    assert_eq!(board_object.designer(), Some(Designer::Form));
    let gauge_object = forms
        .objects()
        .unwrap()
        .nth(usize::from(gauge))
        .unwrap()
        .unwrap();
    assert_eq!(gauge_object.designer(), Some(Designer::UserControl));
    assert_eq!(gauge_object.object_kind().unwrap(), "UserControl");

    // Board: Reset (method 3) at 0x6F8, then the private ShowCount (method
    // 0) at 0x708 after the four publics.
    assert!(matches!(
        resolver.resolve_vtable(board, 0x6F8).callee,
        Callee::Procedure { method: 3, .. }
    ));
    assert!(matches!(
        resolver.resolve_vtable(board, 0x708).callee,
        Callee::Procedure { method: 0, .. }
    ));
    // The getter of Text1 (control index 1), and a member of _Form.
    let text1 = resolver.resolve_vtable(board, 0x2FC);
    assert!(
        matches!(&text1.callee, Callee::Control { index: 1, name, .. } if name == "Text1"),
        "{:?}",
        text1.callee
    );
    assert_eq!(text1.arg_slots, Some(0));
    assert!(matches!(
        resolver.resolve_vtable(board, 0x54).callee,
        Callee::External { iid, vtable_offset: 0x54 } if iid == Designer::Form.interface_iid()
    ));
    // Gauge: its Value property at 0x7A4.
    assert!(matches!(
        resolver.resolve_vtable(gauge, 0x7A4).callee,
        Callee::Procedure { .. }
    ));

    // A class with `Implements`: three empty slots at 0x1C-0x24, then the
    // members; Ring's private Count (method 9) at 0x4C.
    let events = project("events");
    let resolver = CallResolver::new(events).unwrap();
    let ring = object_index(events, "Ring");
    assert!(matches!(
        resolver.resolve_vtable(ring, 0x1C).callee,
        Callee::Unknown { .. }
    ));
    assert!(matches!(
        resolver.resolve_vtable(ring, 0x4C).callee,
        Callee::Procedure { method: 9, .. }
    ));
    // Listener's `WithEvents m_Source`: get, let and set accessors.
    let listener = object_index(events, "Listener");
    for (offset, function) in [(0x30, "GetMemEvent"), (0x38, "SetMemEvent")] {
        let call = resolver.resolve_vtable(listener, offset);
        assert!(
            matches!(&call.callee, Callee::Variable { offset: 0x38, function: f, .. } if f == function),
            "{offset:#x}: {:?}",
            call.callee
        );
        assert_eq!(call.arg_slots, Some(1));
    }
}

/// Every `ExitProc*` says how its procedure returns; the result always comes
/// from the frame.
#[test]
fn describes_procedure_returns() {
    use visualbasic::pcode::decoder::ProcedureReturn;
    let mut forms = BTreeMap::new();
    for name in crate::common::projects() {
        let project = project(&name);
        for object in project.objects().unwrap() {
            for entry in object.unwrap().methods().unwrap() {
                let Ok(MethodEntry::PCode(method)) = entry else {
                    continue;
                };
                for insn in method.instructions().unwrap().flatten() {
                    if !matches!(
                        insn.info.semantics,
                        visualbasic::pcode::semantics::OpcodeSemantics::Return
                    ) {
                        assert!(insn.procedure_return().is_none());
                        continue;
                    }
                    let ret = insn
                        .procedure_return()
                        .unwrap_or_else(|| panic!("{name}: {insn}"));
                    *forms.entry(format!("{ret:?}")).or_insert(0) += 1;
                }
            }
        }
    }
    println!("{forms:#?}");
    // types: MV returns its Variant through the hidden pointer, from the
    // top of the locals (FStVarCopy var_94; ExitProcCb 0x10).
    let types = project("types");
    let last = types
        .objects()
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .methods()
        .unwrap()
        .filter_map(|entry| match entry {
            Ok(MethodEntry::PCode(method)) => method.instructions().unwrap().flatten().last(),
            _ => None,
        })
        .find(|insn| insn.info.mnemonic == "ExitProcCb")
        .unwrap();
    assert_eq!(
        last.procedure_return(),
        Some(ProcedureReturn::CopyToHidden {
            from: -0x94,
            bytes: 0x10
        }),
        "{last}"
    );
    // A Byte result goes through the even slot ebp-0x86 like an Integer.
    assert!(forms.contains_key("CopyToRetval { from: -134, bytes: 1, retval_arg: 12 }"));
    assert!(forms.keys().any(|k| k.starts_with("X87")));
}

/// A module function returning a Variant takes a hidden result pointer at
/// `ebp+0x0C`; its arguments follow.
#[test]
fn frame_of_a_hidden_result() {
    let types = project("types");
    let module = types.objects().unwrap().next().unwrap().unwrap();
    let mv = module
        .methods()
        .unwrap()
        .filter_map(|entry| match entry {
            Ok(MethodEntry::PCode(method)) => Some(method),
            _ => None,
        })
        .find(|method| {
            method
                .instructions()
                .unwrap()
                .flatten()
                .any(|insn| insn.info.mnemonic == "ExitProcCb")
        })
        .unwrap();
    let resolver = FrameResolver::for_method(&mv, None, types.address_map());
    assert!(matches!(resolver.resolve(0x0C), FrameVar::HiddenResult));
    assert!(matches!(
        resolver.resolve(0x10),
        FrameVar::Argument { index: 0, .. }
    ));
    // A function returning a Long has no hidden pointer.
    let ml = method(types, 0, 7);
    let resolver = FrameResolver::for_method(&ml, None, types.address_map());
    assert!(matches!(
        resolver.resolve(0x0C),
        FrameVar::Argument { index: 0, .. }
    ));
}

/// The line-number table behind `Erl`: `flow`'s `ErrLines` numbers its
/// lines 10 to 60.
#[test]
fn reads_line_numbers() {
    let flow = project("flow");
    let mut found = Vec::new();
    for entry in flow
        .objects()
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .methods()
        .unwrap()
    {
        let Ok(MethodEntry::PCode(method)) = entry else {
            continue;
        };
        let lines = method.proc_dsc().line_numbers();
        if !lines.is_empty() {
            assert!(lines.windows(2).all(|w| w[0].offset < w[1].offset));
            found.push(lines.iter().map(|l| l.line).collect::<Vec<_>>());
        }
    }
    assert_eq!(found, vec![vec![10, 20, 30, 40, 50, 60]]);
}

/// `RaiseEvent` names the event and passes Variants by value, the last
/// argument on top like a late-bound call.
#[test]
fn resolves_raised_events() {
    use visualbasic::pcode::semantics::{ArgumentOrder, CallKind, OpcodeSemantics};
    let events = project("events");
    let resolver = CallResolver::new(events).unwrap();
    let source = object_index(events, "Source");
    let mut raised = Vec::new();
    for (index, entry) in events
        .objects()
        .unwrap()
        .nth(usize::from(source))
        .unwrap()
        .unwrap()
        .methods()
        .unwrap()
        .enumerate()
    {
        if !matches!(entry, Ok(MethodEntry::PCode(_))) {
            continue;
        }
        for (insn, call) in calls(events, &resolver, source, index) {
            let OpcodeSemantics::Call { kind } = insn.info.semantics else {
                continue;
            };
            if let Callee::Event { id } = call.callee {
                assert_eq!(kind, CallKind::Event);
                assert_eq!(kind.argument_order(), Some(ArgumentOrder::LastOnTop));
                let widths = call.arg_widths.clone().unwrap();
                assert!(widths.iter().all(|&w| w == 4));
                raised.push((id, widths.len()));
            }
        }
    }
    raised.sort_unstable();
    // Started(), Ticked(n, ratio, Cancel), Named(s, v, o).
    assert_eq!(raised, vec![(1, 0), (2, 3), (3, 3)]);
    assert_eq!(
        CallKind::VCall.argument_order(),
        Some(ArgumentOrder::FirstOnTop)
    );
}

/// Every load, store and literal of the fixtures says what it moves, its
/// operands resolved.
#[test]
fn describes_movements() {
    use visualbasic::pcode::movement::{Fill, Move, Place};
    let mut moved = 0;
    for name in crate::common::projects() {
        let project = project(&name);
        for object in project.objects().unwrap() {
            for entry in object.unwrap().methods().unwrap() {
                let Ok(MethodEntry::PCode(method)) = entry else {
                    continue;
                };
                for insn in method.instructions().unwrap().flatten() {
                    let movable = insn.info.category.starts_with("load_")
                        || insn.info.category.starts_with("store_");
                    if movable {
                        assert!(insn.movement().is_some(), "{name}: {insn}");
                        moved += 1;
                    }
                }
            }
        }
    }
    assert!(moved > 2000, "{moved}");
    // statics: `FMemLdR4 arg_8 0x0008` loads the module variable m_Count;
    // `FLdI2` leaves its slot's high half undefined.
    let statics = project("statics");
    let use_module = method(statics, 0, 2);
    let first = use_module.instructions().unwrap().flatten().next().unwrap();
    assert_eq!(
        first.movement().unwrap().moves,
        vec![Move::Load {
            place: Place::FrameMember {
                frame: 8,
                offset: 8
            },
            bytes: 4,
            fill: Fill::Full
        }]
    );
}

/// The control-array object (all-zero IID), which no type library
/// describes, resolves without a catalog, measured on `forms`: `Item` (0x40)
/// takes the index and the return pointer, `LBound` (0x44), `UBound` (0x48)
/// and `Count` (0x4C) the return pointer. The paths through those calls
/// simulate without a failure.
#[test]
fn resolves_the_control_array_object() {
    use visualbasic::pcode::{
        calltarget::RuntimeInterfaces,
        stacksim::{Cut, InstructionStack, ProcedureStack},
    };
    let forms = project("forms");
    let board = object_index(forms, "Board");
    let resolver = CallResolver::new(forms).unwrap();
    let mut names = Vec::new();
    for (code, bytes) in pcode_methods(forms, board) {
        for insn in &code {
            if let Some(call) = resolver.resolve(board, insn)
                && let Callee::Interface { members, .. } = &call.callee
            {
                assert_eq!(members[0].interface, RuntimeInterfaces::CONTROL_ARRAY);
                names.push(members[0].name.clone());
            }
        }
        let stack = ProcedureStack::simulate(&code, bytes, &|insn, pr| {
            resolver.resolve_with_pr(board, insn, pr)
        });
        for record in &stack.instructions {
            if let InstructionStack::Cut { reason, .. } = record {
                assert!(!matches!(reason, Cut::Pop(_)), "{reason:?}");
            }
        }
        assert!(stack.merge_conflicts.is_empty());
    }
    names.sort();
    names.dedup();
    assert_eq!(names, ["Count", "Item", "LBound", "UBound"]);
}

/// A caller-supplied [`InterfaceCatalog`] resolves the external interfaces
/// it describes; without it they stay [`Callee::External`].
#[test]
fn resolves_through_an_interface_catalog() {
    use std::sync::Arc;
    use visualbasic::{
        pcode::calltarget::{InterfaceCatalog, InterfaceMember, InvokeKind},
        vb::control::Guid,
    };
    let forms = project("forms");
    let board = object_index(forms, "Board");
    let plain = CallResolver::new(forms).unwrap();
    // The first external member Board calls.
    let (iid, offset) = pcode_methods(forms, board)
        .iter()
        .flat_map(|(code, _)| code)
        .find_map(|insn| match plain.resolve(board, insn)?.callee {
            Callee::External { iid, vtable_offset } => Some((iid, vtable_offset)),
            _ => None,
        })
        .unwrap();
    struct One(Guid, u16);
    impl InterfaceCatalog for One {
        fn members(&self, iid: &Guid, vtable_offset: u16) -> Vec<InterfaceMember> {
            if *iid != self.0 || vtable_offset != self.1 {
                return Vec::new();
            }
            vec![InterfaceMember {
                interface: "_Host".to_string(),
                name: "Member".to_string(),
                invoke: InvokeKind::Method,
                dispid: None,
                arg_widths: vec![1, 2],
                float_result: false,
                returns_interface: None,
            }]
        }
    }
    let resolver = CallResolver::new(forms)
        .unwrap()
        .with_interfaces(Arc::new(One(iid, offset)));
    let mut resolved = 0;
    for (code, _) in pcode_methods(forms, board) {
        for insn in &code {
            let before = plain.resolve(board, insn);
            let Some(Callee::External {
                iid: i,
                vtable_offset: o,
            }) = before.as_ref().map(|call| &call.callee)
            else {
                continue;
            };
            let call = resolver.resolve(board, insn).unwrap();
            if (*i, *o) == (iid, offset) {
                assert!(
                    matches!(&call.callee, Callee::Interface { members, .. } if members[0].name == "Member")
                );
                assert_eq!(call.arg_slots, Some(3));
                assert_eq!(call.arg_widths, Some(vec![1, 2]));
                resolved += 1;
            } else {
                assert!(matches!(call.callee, Callee::External { .. }));
            }
        }
    }
    assert!(resolved >= 1);
}

/// `members`: every public member of `Holder`, written and read from a
/// module through its accessor. `PutMem8` (a `Double`, `Currency` or
/// `Date`) takes the 8-byte value, two slots; `PutMemVar` / `SetMemVar` the
/// Variant's address; every other `PutMem*` / `SetMem*` and every `GetMem*`
/// one slot. The procedure simulates without a failure.
#[test]
fn resolves_member_variable_accessors() {
    use visualbasic::pcode::stacksim::{Cut, InstructionStack, ProcedureStack};
    let members = project("members");
    let program = object_index(members, "Program");
    let resolver = CallResolver::new(members).unwrap();
    let mut widths = BTreeMap::new();
    for (_, call) in calls(members, &resolver, program, 0) {
        if let Callee::Variable { function, .. } = &call.callee {
            widths.insert(function.clone(), call.arg_widths.clone().unwrap());
        }
    }
    assert_eq!(widths["PutMem8"], [2]);
    assert_eq!(widths["GetMem8"], [1]);
    assert_eq!(widths["PutMemVar"], [1]);
    assert_eq!(widths["SetMemVar"], [1]);
    assert_eq!(widths["PutMem4"], [1]);
    assert_eq!(widths["SetMemObj"], [1]);
    assert_eq!(widths["GetMemNewObj"], [1]);

    let (code, bytes) = &pcode_methods(members, program)[0];
    let stack = ProcedureStack::simulate(code, bytes, &|insn, pr| {
        resolver.resolve_with_pr(program, insn, pr)
    });
    for record in &stack.instructions {
        assert!(!matches!(
            record,
            InstructionStack::Cut {
                reason: Cut::Pop(_),
                ..
            }
        ));
    }
}

/// The other designers' vtables: an MDIForm's own members start at 0x6F8,
/// a UserDocument's at 0x770 (its control getters at 0x370 + 4i), a
/// PropertyPage's control getters at 0x310 + 4i. A control with no event
/// handler has a getter too.
#[test]
fn resolves_designer_vtables() {
    for (fixture, name, offset, expected) in [
        ("mdi", "Frame", 0x6F8, "OpenChild"),
        ("docs", "Page", 0x774, "Title"),
    ] {
        let project = project(fixture);
        let object = object_index(project, name);
        let resolver = CallResolver::new(project).unwrap();
        let call = resolver.resolve_vtable(object, offset);
        let Callee::Procedure { method, .. } = call.callee else {
            panic!("{name} {offset:#x}: {:?}", call.callee);
        };
        let names = project
            .objects()
            .unwrap()
            .nth(usize::from(object))
            .unwrap()
            .unwrap();
        assert_eq!(
            names.method_name(method).unwrap().as_str().as_deref(),
            Some(expected)
        );
    }
    for (fixture, name, offset, control) in [
        ("docs", "Page", 0x374, "Body"),
        ("ocx", "KnobPage", 0x314, "PositionText"),
        // No event handler, so no ControlInfo: named from the form data.
        ("activex", "Client", 0x318, "CommonDialog1"),
    ] {
        let project = project(fixture);
        let object = object_index(project, name);
        let resolver = CallResolver::new(project).unwrap();
        let Callee::Control { name: found, .. } = resolver.resolve_vtable(object, offset).callee
        else {
            panic!("{name} {offset:#x}");
        };
        assert_eq!(found, control);
    }
}

/// A `VCall*` without an IID resolves through the interface its frame slot
/// holds, which the procedure's own code states ([`SlotInterfaces`]):
/// `vtable`'s `Vtable(ByVal r As IRaw)` copies `r` to a local whose
/// `VCallHresult` names `IRaw`, so its typed `VCallUI1` / `VCallFPR8` calls are
/// `IRaw` members; `flow`'s `Err.Clear` (`VCallFPR8 0x48`) is on the object
/// `rtcErrObj` returned, `_ErrObject`.
#[test]
fn resolves_calls_through_slot_interfaces() {
    use visualbasic::pcode::calltarget::{RuntimeInterfaces, SlotInterfaces};
    let iraw = "{5D1E7A10-6C2B-4E1A-9B7F-3A2C1D0E0F02}";
    let externals = |name: &str| {
        let project = project(name);
        let resolver = CallResolver::new(project).unwrap();
        let mut found = Vec::new();
        for (object, _) in project.objects().unwrap().enumerate() {
            let object = u16::try_from(object).unwrap();
            for (code, bytes) in pcode_methods(project, object) {
                let (stack, slots): (_, SlotInterfaces) =
                    resolver.simulate_procedure(object, &code, bytes);
                // Pr where the simulation reached, else the last Pr load
                // before the instruction (a call with no catalog cuts the
                // path).
                let mut last_load = None;
                for (insn, record) in code.iter().zip(&stack.instructions) {
                    let pr = match record {
                        visualbasic::pcode::stacksim::InstructionStack::Reached { pr, .. }
                        | visualbasic::pcode::stacksim::InstructionStack::Cut { pr, .. } => *pr,
                        _ => last_load,
                    };
                    if let Some(source) = insn.pr_source() {
                        last_load = Some(source);
                    }
                    if !matches!(insn.info.mnemonic, "VCallUI1" | "VCallFPR8") {
                        continue;
                    }
                    let plain = resolver.resolve_with_pr(object, insn, pr.as_ref()).unwrap();
                    let typed = resolver
                        .resolve_with_slots(object, insn, pr.as_ref(), &slots)
                        .unwrap();
                    assert!(matches!(plain.callee, Callee::Unknown { .. }));
                    if let Callee::External { iid, vtable_offset } = typed.callee {
                        found.push((insn.info.mnemonic, iid.to_string(), vtable_offset));
                    }
                }
            }
        }
        found
    };
    let vtable = externals("vtable");
    assert!(
        vtable.contains(&("VCallUI1", iraw.to_string(), 0x0C)),
        "{vtable:?}"
    );
    assert!(
        vtable
            .iter()
            .any(|(m, iid, _)| *m == "VCallFPR8" && iid == iraw)
    );
    let flow = externals("flow");
    assert!(flow.contains(&("VCallFPR8", RuntimeInterfaces::ERR_OBJECT.to_string(), 0x48)));
}

/// A slot filled from a call's result takes the interface the call returns:
/// `Set o = r.GetSelf()` on a catalog member that returns `IRaw`, and
/// `Set c = c.Self()` on a project function returning its own class. A
/// temporary that holds two interfaces in turn has, at each use, the one
/// stored last.
#[test]
fn slots_take_the_interface_a_call_returns() {
    use std::sync::Arc;

    use visualbasic::{
        pcode::{
            calltarget::{InterfaceCatalog, InterfaceMember, InvokeKind, SlotInterfaces},
            stackeffect::PrSource,
        },
        vb::control::Guid,
    };

    /// `IRaw`'s `GetSelf` (0x38), `GetDouble` (0x1C) and `GetUnknown`
    /// (0x34), `GetSelf` returning `IRaw` and `GetUnknown` `IUnknown` when
    /// `typed`. Both results pass through the same temporary (`var_E4`).
    const IUNKNOWN: Guid = Guid {
        bytes: [0, 0, 0, 0, 0, 0, 0, 0, 0xC0, 0, 0, 0, 0, 0, 0, 0x46],
    };
    struct Raw {
        iid: Guid,
        typed: bool,
    }
    impl InterfaceCatalog for Raw {
        fn members(&self, iid: &Guid, vtable_offset: u16) -> Vec<InterfaceMember> {
            if *iid != self.iid {
                return Vec::new();
            }
            let (name, float_result, returns_interface) = match vtable_offset {
                0x38 => ("GetSelf", false, self.typed.then_some(self.iid)),
                0x1C => ("GetDouble", true, None),
                0x34 => ("GetUnknown", false, self.typed.then_some(IUNKNOWN)),
                _ => return Vec::new(),
            };
            vec![InterfaceMember {
                interface: "IRaw".to_string(),
                name: name.to_string(),
                invoke: InvokeKind::Method,
                dispid: None,
                arg_widths: Vec::new(),
                float_result,
                returns_interface,
            }]
        }
    }

    // The slots stored from a call to `name` (the store after it), and
    // whether a later call on such a slot resolved to `then`.
    fn stored_after(
        resolver: &CallResolver<'_, '_>,
        project: &'static VbProject<'static>,
        object: u16,
        name: &str,
        then: &str,
    ) -> Vec<(Option<Guid>, bool)> {
        let mut found = Vec::new();
        for (code, bytes) in pcode_methods(project, object) {
            let (stack, slots): (_, SlotInterfaces) =
                resolver.simulate_procedure(object, &code, bytes);
            // Pr where the simulation reached, else the last Pr load before
            // the instruction (a call with no catalog entry cuts the path).
            let mut last_load = None;
            let prs: Vec<_> = code
                .iter()
                .zip(&stack.instructions)
                .map(|(insn, record)| {
                    let pr = match record {
                        visualbasic::pcode::stacksim::InstructionStack::Reached { pr, .. }
                        | visualbasic::pcode::stacksim::InstructionStack::Cut { pr, .. } => *pr,
                        _ => last_load,
                    };
                    if let Some(source) = insn.pr_source() {
                        last_load = Some(source);
                    }
                    pr
                })
                .collect();
            for (index, insn) in code.iter().enumerate() {
                let pr = prs[index];
                let Some(call) = resolver.resolve_with_slots(object, insn, pr.as_ref(), &slots)
                else {
                    continue;
                };
                let named = match &call.callee {
                    Callee::Interface { members, .. } => members[0].name == name,
                    Callee::Procedure { object, method } => format!("{object}.{method}") == name,
                    _ => false,
                };
                if !named {
                    continue;
                }
                // The store of the result: next instruction (pushed) or
                // after `FLdZeroAd retval` (moved).
                let store = code[index + 1..]
                    .iter()
                    .find(|i| i.info.mnemonic == "FStAdFunc")
                    .unwrap();
                let Some(Some(visualbasic::pcode::operand::Operand::StackVar(target))) =
                    store.operands.first()
                else {
                    panic!("{store:?}");
                };
                let then_resolved = code.iter().enumerate().any(|(at, later)| {
                    prs[at] == Some(PrSource::Frame { offset: *target })
                        && matches!(
                            resolver
                                .resolve_with_slots(object, later, prs[at].as_ref(), &slots)
                                .map(|c| c.callee),
                            Some(Callee::Interface { members, .. }) if members[0].name == then
                        )
                });
                let after_store = PrSource::Frame { offset: *target };
                found.push((
                    slots.for_pr_at(Some(&after_store), store.offset + 1),
                    then_resolved,
                ));
            }
        }
        found
    }

    let vtable = project("vtable");
    let program = object_index(vtable, "Program");
    let iraw: Guid = "{5D1E7A10-6C2B-4E1A-9B7F-3A2C1D0E0F02}".parse().unwrap();
    let typed = CallResolver::new(vtable)
        .unwrap()
        .with_interfaces(Arc::new(Raw {
            iid: iraw,
            typed: true,
        }));
    let found = stored_after(&typed, vtable, program, "GetSelf", "GetDouble");
    assert!(found.contains(&(Some(iraw), true)), "{found:?}");
    // Control: the same catalog without the return type leaves the slot
    // untyped and the call on it unresolved.
    let untyped = CallResolver::new(vtable)
        .unwrap()
        .with_interfaces(Arc::new(Raw {
            iid: iraw,
            typed: false,
        }));
    let found = stored_after(&untyped, vtable, program, "GetSelf", "GetDouble");
    assert!(
        !found.is_empty() && found.iter().all(|f| *f == (None, false)),
        "{found:?}"
    );

    // `Self() As Counter` returns through its [retval] slot, which
    // `FLdZeroAd` moves into `c`.
    let calls = project("calls");
    let resolver = CallResolver::new(calls).unwrap();
    let program = object_index(calls, "Program");
    let counter = object_index(calls, "Counter");
    let iid = calls
        .objects()
        .unwrap()
        .nth(usize::from(counter))
        .unwrap()
        .unwrap()
        .default_iids()
        .next()
        .unwrap()
        .1;
    let found = stored_after(&resolver, calls, program, &format!("{counter}.11"), "");
    assert_eq!(found, vec![(Some(iid), false)]);
}

/// Arguments carry interfaces between procedures: `Sub Main` passes `r`,
/// which its own code never types, to `Vtable(ByVal r As IRaw)`, whose
/// `VCallHresult` names `IRaw` on its copy of the parameter.
#[test]
fn arguments_carry_interfaces_between_procedures() {
    use visualbasic::pcode::calltarget::SlotInterfaces;

    let vtable = project("vtable");
    let program = object_index(vtable, "Program");
    let resolver = CallResolver::new(vtable).unwrap();
    let methods: Vec<_> = vtable
        .objects()
        .unwrap()
        .nth(usize::from(program))
        .unwrap()
        .unwrap()
        .methods()
        .unwrap()
        .map(Result::unwrap)
        .collect();
    // `Sub Main` is the last method, `Vtable` the first P-Code one.
    let main = u16::try_from(methods.len() - 1).unwrap();
    let callee = u16::try_from(
        methods
            .iter()
            .position(|m| matches!(m, MethodEntry::PCode(_)))
            .unwrap(),
    )
    .unwrap();
    let iraw = "{5D1E7A10-6C2B-4E1A-9B7F-3A2C1D0E0F02}";

    let slots = resolver.infer_project_slots();
    // The callee's parameter, from its own code.
    let parameter = slots.get(program, callee).unwrap().get(0x0C).unwrap();
    assert_eq!(parameter.to_string(), iraw);
    // Main's `r` (var_88), from the call.
    let r = slots.get(program, main).unwrap().get(-0x88);
    assert_eq!(r.map(|g| g.to_string()).as_deref(), Some(iraw));

    // Control: Main's own code alone leaves `r` untyped.
    let MethodEntry::PCode(method) = &methods[usize::from(main)] else {
        panic!();
    };
    let code: Vec<_> = method.instructions().unwrap().flatten().collect();
    let (_, own): (_, SlotInterfaces) =
        resolver.simulate_procedure(program, &code, method.pcode_bytes());
    assert_eq!(own.get(-0x88), None);
}

/// The late-bound calls by DISPID of method `index` of object `object`, in
/// code order: each DISPID with the member it reaches, resolved through the
/// interfaces the procedure's frame slots hold.
fn late_targets(
    project: &'static VbProject<'static>,
    resolver: &CallResolver<'static, 'static>,
    object: u16,
    index: u16,
) -> Vec<(i32, Option<visualbasic::pcode::calltarget::LateTarget>)> {
    use visualbasic::pcode::stacksim::InstructionStack;

    let method = method(project, object, usize::from(index));
    let code: Vec<Instruction> = method.instructions().unwrap().flatten().collect();
    let (stack, slots) = resolver.simulate_procedure(object, &code, method.pcode_bytes());
    let mut last_load = None;
    let mut found = Vec::new();
    for (insn, record) in code.iter().zip(&stack.instructions) {
        let pr = match record {
            InstructionStack::Reached { pr, .. } | InstructionStack::Cut { pr, .. } => *pr,
            _ => last_load,
        };
        if let Some(source) = insn.pr_source() {
            last_load = Some(source);
        }
        if let Some(Callee::Late {
            dispid: Some(dispid),
            target,
            ..
        }) = resolver
            .resolve_with_slots(object, insn, pr.as_ref(), &slots)
            .map(|call| call.callee)
        {
            found.push((dispid, target));
        }
    }
    found
}

/// `dispid`: a form's late-bound calls on the UserControl it hosts reach
/// the control's procedures by their prototypes' DISPIDs: its control
/// getter returns the control's class, which names the procedure each call
/// invokes the way the opcode does (a load gets, a store lets, a store of
/// an object sets, a call calls).
#[test]
fn late_calls_reach_a_hosted_user_controls_procedures() {
    use visualbasic::pcode::calltarget::{InvokeKind, LateTarget};

    let project = project_with_dispids();
    let resolver = CallResolver::new(project).unwrap();
    let host = object_index(project, "Host");
    let dial = object_index(project, "Dial");
    let name = |target: &Option<LateTarget>| match target {
        Some(LateTarget::Procedure {
            object,
            method,
            invoke,
        }) => {
            assert_eq!(*object, dial);
            let object = project
                .objects()
                .unwrap()
                .nth(usize::from(*object))
                .unwrap()
                .unwrap();
            let name = object.method_name(*method).unwrap();
            let accessor = match invoke {
                InvokeKind::Method => "",
                InvokeKind::Get => "get ",
                InvokeKind::Let => "let ",
                InvokeKind::Set => "set ",
            };
            format!("{accessor}{}", name.as_str().unwrap())
        }
        other => panic!("{other:?}"),
    };
    let named = |method: &str| -> Vec<String> {
        late_targets(
            project,
            &resolver,
            host,
            method_index(project, host, method),
        )
        .iter()
        .map(|(_, target)| name(target))
        .collect()
    };
    assert_eq!(
        named("Properties"),
        ["let Value", "get Value", "set Target", "get Target"]
    );
    assert_eq!(
        named("Indexed"),
        [
            "let Item", "get Item", "let Item", "set Slot", "get Item", "get Slot"
        ]
    );
    // `Scaled(1.5) + Len(Describe("v"))` calls `Describe` first.
    assert_eq!(
        named("Methods"),
        ["Clear", "Spin", "Spin", "Describe", "Scaled"]
    );
    assert_eq!(
        named("Named"),
        ["Spin", "let Item", "get Target", "set Slot", "Describe"]
    );
}

/// `dispid`: the extender's members of a hosted control are called 0x3000
/// below the DISPIDs `_VBControlExtender` declares, and reach the members
/// it declares there when the caller's catalog describes it; with no
/// catalog they stay unresolved.
#[test]
fn late_calls_reach_a_hosted_controls_extender() {
    use std::sync::Arc;

    use visualbasic::{
        pcode::calltarget::{
            InterfaceCatalog, InterfaceMember, InvokeKind, LateTarget, RuntimeInterfaces,
        },
        vb::control::Guid,
    };

    /// `_VBControlExtender`'s members at the DISPIDs `VB6.OLB` declares.
    struct Extender;
    impl InterfaceCatalog for Extender {
        fn members(&self, _iid: &Guid, _vtable_offset: u16) -> Vec<InterfaceMember> {
            Vec::new()
        }
        fn dispatched(&self, iid: &Guid, dispid: i32) -> Vec<InterfaceMember> {
            if *iid != RuntimeInterfaces::CONTROL_EXTENDER {
                return Vec::new();
            }
            let property = match dispid.cast_unsigned() {
                0x8001_3000 => "Name",
                0x8001_3003 => "Left",
                0x8001_3004 => "Top",
                0x8001_3005 => "Width",
                0x8001_3006 => "Height",
                0x8001_3007 => "Visible",
                0x8001_3008 => "Parent",
                0x8001_3009 => "DragMode",
                0x8001_300B => "Tag",
                0x8001_300F => "TabIndex",
                0x8001_301C => "object",
                0x8001_3032 => "HelpContextID",
                0x8001_3046 => "WhatsThisHelpID",
                0x8001_3047 => "Container",
                0x8001_3048 => "CausesValidation",
                0x8001_304A => "ToolTipText",
                _ => "",
            };
            let method = match dispid.cast_unsigned() {
                0x8001_4000 => "SetFocus",
                0x8001_4001 => "ZOrder",
                0x8001_4002 => "Move",
                0x8001_4003 => "Drag",
                0x8001_4004 => "ShowWhatsThis",
                _ => "",
            };
            let member = |name: &str, invoke| InterfaceMember {
                interface: "_VBControlExtender".to_string(),
                name: name.to_string(),
                invoke,
                dispid: Some(dispid),
                arg_widths: Vec::new(),
                float_result: false,
                returns_interface: None,
            };
            match (property, method) {
                ("", "") => Vec::new(),
                ("", method) => vec![member(method, InvokeKind::Method)],
                (property, _) => vec![
                    member(property, InvokeKind::Get),
                    member(property, InvokeKind::Let),
                ],
            }
        }
    }

    assert_eq!(
        RuntimeInterfaces::CONTROL_EXTENDER.to_string(),
        "{164CBDD0-7321-11D1-A1E8-00A0C90F2731}"
    );
    let project = project_with_dispids();
    let host = object_index(project, "Host");
    let bare = CallResolver::new(project).unwrap();
    assert!(
        late_targets(
            project,
            &bare,
            host,
            method_index(project, host, "Extender")
        )
        .iter()
        .all(|(_, target)| target.is_none())
    );

    let resolver = CallResolver::new(project)
        .unwrap()
        .with_interfaces(Arc::new(Extender));
    let named = |method: &str| -> Vec<String> {
        late_targets(
            project,
            &resolver,
            host,
            method_index(project, host, method),
        )
        .iter()
        .map(|(dispid, target)| match target {
            Some(LateTarget::Members {
                iid,
                dispid: declared,
                members,
            }) => {
                assert_eq!(*iid, RuntimeInterfaces::CONTROL_EXTENDER);
                assert_eq!(declared.wrapping_sub(*dispid), 0x3000);
                let invoke = match members.as_slice() {
                    [only] => only.invoke,
                    other => panic!("{other:?}"),
                };
                let accessor = match invoke {
                    InvokeKind::Method => "",
                    InvokeKind::Get => "get ",
                    InvokeKind::Let => "let ",
                    InvokeKind::Set => "set ",
                };
                format!("{accessor}{}", members[0].name)
            }
            other => panic!("{dispid:#x}: {other:?}"),
        })
        .collect()
    };
    assert_eq!(
        named("Extender"),
        [
            "get Left",
            "let Left",
            "let Visible",
            "get Name",
            "let Tag",
            "get Tag",
            "get Width"
        ]
    );
    assert_eq!(
        named("ExtenderMembers"),
        [
            "SetFocus",
            "ZOrder",
            "Move",
            "Drag",
            "ShowWhatsThis",
            "get Height",
            "let Top",
            "let TabIndex",
            "let ToolTipText",
            "let HelpContextID",
            "let WhatsThisHelpID",
            "let CausesValidation",
            "let DragMode",
            "get Parent",
            "get Container",
            "get object"
        ]
    );
}

/// `activex`: a hosted ActiveX control's form record names its class by
/// ProgID, with or without an event handler (`CommonDialog1` has none),
/// so the late-bound calls on it, and on an element of a control array of
/// it, ask the caller's catalog for the members of its class's default
/// interface.
#[test]
fn late_calls_reach_an_activex_controls_class() {
    use std::sync::Arc;

    use visualbasic::{
        pcode::calltarget::{InterfaceCatalog, InterfaceMember, InvokeKind, LateTarget},
        vb::control::Guid,
    };

    /// Any member of any interface, named by its interface's IID.
    struct Any;
    impl InterfaceCatalog for Any {
        fn members(&self, _iid: &Guid, _vtable_offset: u16) -> Vec<InterfaceMember> {
            Vec::new()
        }
        fn dispatched(&self, iid: &Guid, dispid: i32) -> Vec<InterfaceMember> {
            [InvokeKind::Method, InvokeKind::Let]
                .into_iter()
                .map(|invoke| InterfaceMember {
                    interface: iid.to_string(),
                    name: format!("{dispid:#x}"),
                    invoke,
                    dispid: Some(dispid),
                    arg_widths: Vec::new(),
                    float_result: false,
                    returns_interface: None,
                })
                .collect()
        }
    }

    let project = project("activex");
    let components: Vec<String> = project
        .components()
        .unwrap()
        .map(|component| component.prog_id().into_owned())
        .collect();
    let client = object_index(project, "Client");
    let object = project
        .objects()
        .unwrap()
        .nth(usize::from(client))
        .unwrap()
        .unwrap();
    let gui = project
        .gui_entries()
        .unwrap()
        .find(|entry| entry.object_index().ok() == Some(u32::from(client)))
        .unwrap();
    let form = object.form_data_from_gui_entry(&gui).unwrap();
    let records: Vec<(String, String)> = form
        .controls()
        .iter()
        .map(|record| {
            (
                record.name().into_owned(),
                record.prog_id().unwrap().into_owned(),
            )
        })
        .collect();
    assert!(
        records
            .iter()
            .all(|(_, prog_id)| components.contains(prog_id))
    );
    assert!(records.contains(&(
        "CommonDialog1".to_string(),
        "MSComDlg.CommonDialog".to_string()
    )));

    let resolver = CallResolver::new(project)
        .unwrap()
        .with_interfaces(Arc::new(Any));
    let interfaces = |method: &str| -> Vec<String> {
        late_targets(
            project,
            &resolver,
            client,
            method_index(project, client, method),
        )
        .into_iter()
        .map(|(dispid, target)| match target {
            Some(LateTarget::Members { iid, members, .. }) => {
                assert!(members.iter().all(|m| m.name == format!("{dispid:#x}")));
                iid.to_string()
            }
            other => format!("{other:?}"),
        })
        .collect()
    };
    // `CommonDialog`'s default interface, `ICommonDialog`.
    assert_eq!(
        interfaces("PickFile"),
        ["{083039C2-13F4-11D1-8B7E-0000F8754DA1}"; 3]
    );
    // `Winsock`'s, `IMSWinsockControl`: the four calls on `Winsock1`, and
    // the two on `Peers(0)`, an element `Item` returns from the array.
    assert_eq!(
        interfaces("Start"),
        ["{248DD892-BB45-11CF-9ABC-0080C7E7B78D}"; 6]
    );
    // Every call by DISPID of the form, its handlers' included.
    let mut resolved = 0;
    for (index, entry) in object.methods().unwrap().enumerate() {
        if !matches!(entry, Ok(MethodEntry::PCode(_))) {
            continue;
        }
        let index = u16::try_from(index).unwrap();
        for (dispid, target) in late_targets(project, &resolver, client, index) {
            assert!(target.is_some(), "method {index}: {dispid:#x}");
            resolved += 1;
        }
    }
    assert!(resolved >= 20, "{resolved}");
}

/// Every fixture: each call by DISPID reaches a member once its receiver's
/// class is known, given a catalog that describes every external member.
#[test]
fn every_call_by_dispid_reaches_a_member() {
    use std::sync::Arc;

    use visualbasic::{
        pcode::calltarget::{InterfaceCatalog, InterfaceMember, InvokeKind},
        vb::control::Guid,
    };

    /// Every member of every interface, each invoked every way.
    struct Every;
    impl InterfaceCatalog for Every {
        fn members(&self, _iid: &Guid, _vtable_offset: u16) -> Vec<InterfaceMember> {
            Vec::new()
        }
        fn dispatched(&self, iid: &Guid, dispid: i32) -> Vec<InterfaceMember> {
            [
                InvokeKind::Method,
                InvokeKind::Get,
                InvokeKind::Let,
                InvokeKind::Set,
            ]
            .into_iter()
            .map(|invoke| InterfaceMember {
                interface: iid.to_string(),
                name: format!("{dispid:#x}"),
                invoke,
                dispid: Some(dispid),
                arg_widths: Vec::new(),
                float_result: false,
                returns_interface: None,
            })
            .collect()
        }
    }

    let mut unresolved = Vec::new();
    let mut resolved = 0;
    for name in common::projects() {
        let project = project(&name);
        let resolver = CallResolver::new(project)
            .unwrap()
            .with_interfaces(Arc::new(Every));
        for (object, entry) in project.objects().unwrap().enumerate() {
            let object = u16::try_from(object).unwrap();
            let Ok(entry) = entry else {
                continue;
            };
            for (index, method) in entry.methods().unwrap().enumerate() {
                if !matches!(method, Ok(MethodEntry::PCode(_))) {
                    continue;
                }
                let index = u16::try_from(index).unwrap();
                for (dispid, target) in late_targets(project, &resolver, object, index) {
                    match target {
                        Some(_) => resolved += 1,
                        None => unresolved.push(format!("{name} {object}.{index} {dispid:#x}")),
                    }
                }
            }
        }
    }
    assert!(unresolved.is_empty(), "{unresolved:#?}");
    assert!(resolved >= 60, "{resolved}");
}
