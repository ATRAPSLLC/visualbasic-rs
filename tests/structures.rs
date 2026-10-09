//! The VB structures, checked on the compiled fixtures of known source: the
//! method tables and method links, the variable descriptor tables, the
//! ProjectInfo2 records, the external table, COM registration, the entry
//! point, and the bounds on counts read from the file.

#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]

mod common;

use std::{
    collections::{BTreeMap, HashSet},
    time::{Duration, Instant},
};

use visualbasic::{
    CompilationMode, EntrypointKind, Error, MethodEntry, RecognitionFailure, VbObject, VbProject,
    project::{CodeEntryKind, MethodLinkKind, MethodNameResult},
    vb::{
        comreg::ComRegData,
        constantpool::PoolEntry,
        controlprop::ControlPropertyType,
        external::{CallApiStub, ConversionKind, ExternalKind},
        projectinfo2::{ControlTypeIter, read_name_strings},
    },
};

use crate::common::{fixture, projects};

/// Reads fixture `name`, leaked so its project outlives the test's borrows.
fn project(name: &str) -> &'static VbProject<'static> {
    let bytes: &'static [u8] = Box::leak(std::fs::read(fixture(name)).unwrap().into_boxed_slice());
    Box::leak(Box::new(VbProject::from_bytes(bytes).unwrap()))
}

/// The object named `name`.
fn object(project: &'static VbProject<'static>, name: &str) -> VbObject<'static, 'static> {
    project
        .objects()
        .unwrap()
        .map(Result::unwrap)
        .find(|o| o.name().unwrap() == name)
        .unwrap_or_else(|| panic!("no object {name}"))
}

/// The index of the method named `name` in `object`'s method table.
fn method_index(object: &VbObject<'_, '_>, name: &str) -> u16 {
    (0..object.method_count().unwrap())
        .find(|&i| object.method_name(i).unwrap().as_str().as_deref() == Some(name))
        .unwrap_or_else(|| panic!("no method {name}"))
}

/// Every fixture is P-Code but the `-native` ones, and says so: an object holds
/// P-Code when a method table slot points at one of its ProcDscInfos, not
/// by OptionalObjectInfo +0x2A, which is the count of inherited vtable
/// slots.
#[test]
fn compilation_mode_comes_from_the_method_tables() {
    for name in projects() {
        let project = project(&name);
        let expected = if name.ends_with("-native") {
            CompilationMode::Native
        } else {
            CompilationMode::Pcode
        };
        assert_eq!(project.compilation_mode().unwrap(), expected, "{name}");
        assert!(
            !project
                .diagnostics()
                .unwrap()
                .iter()
                .any(|d| d.site == "compilation_mode"),
            "{name}"
        );
        for obj in project.objects().unwrap() {
            let obj = obj.unwrap();
            let pcode = obj.pcode_methods().unwrap().count();
            assert_eq!(
                obj.has_pcode().unwrap(),
                pcode > 0,
                "{name} {}",
                obj.name().unwrap()
            );
            if let Some(opt) = obj.optional_info() {
                // The designer's built-in interface and 256 control getters
                // after IDispatch: Form and MDIForm 439, UserControl 482,
                // UserDocument 469, PropertyPage 445.
                let slots = obj.designer().map_or(0, |designer| {
                    (designer.builtin_vtable_size() + 0x400 - 0x1C) / 4
                });
                assert_eq!(opt.inherited_vtable_slots().unwrap(), slots, "{name}");
            }
        }
    }
    // The objects with no procedure have no P-Code; every other one does.
    assert!(!object(project("data"), "Item").has_pcode().unwrap());
    assert!(!object(project("statics"), "Other").has_pcode().unwrap());
    assert!(object(project("forms"), "Board").has_pcode().unwrap());
}

/// The runtime's vtable block of each object is
/// `0x28 + 4 * (inherited slots + method links)` long: each block ends where
/// the next one starts. Counting methods instead of method links breaks it
/// (control).
#[test]
fn basic_class_object_blocks_are_adjacent() {
    let mut pairs = 0;
    let mut method_count_holds = true;
    for name in projects() {
        let project = project(&name);
        let mut blocks = BTreeMap::new();
        for obj in project.objects().unwrap() {
            let obj = obj.unwrap();
            if let Some(opt) = obj.optional_info() {
                let by_methods = 0x28
                    + 4 * (u32::from(opt.inherited_vtable_slots().unwrap())
                        + u32::from(obj.method_count().unwrap()));
                blocks.insert(
                    opt.basic_class_object_va().unwrap(),
                    (opt.basic_class_object_size().unwrap(), by_methods),
                );
            }
        }
        let blocks: Vec<_> = blocks.into_iter().collect();
        for pair in blocks.windows(2) {
            let ((va, (size, by_methods)), (next, _)) = (pair[0], pair[1]);
            assert_eq!(va + size, next, "{name}: block at {va:#x}");
            method_count_holds &= va + by_methods == next;
            pairs += 1;
        }
    }
    assert!(pairs >= 22, "{pairs}");
    assert!(!method_count_holds);
}

/// The COM registration flags are tested on their high byte: the
/// fixtures' UserControls (`0x2001`) are controls.
#[test]
fn comreg_flags_name_the_user_controls() {
    for (name, class) in [("dispid", "Dial"), ("forms", "Gauge")] {
        let project = project(name);
        let map = project.address_map();
        let va = project.vb_header().com_register_data_va().unwrap();
        let reg = ComRegData::parse(map.slice_from_va(va, 0x30).unwrap(), va).unwrap();
        let objects: Vec<_> = reg.objects(map).unwrap().collect();
        assert_eq!(objects.len(), 1, "{name}");
        let obj = &objects[0];
        assert_eq!(obj.object_name(map), Some(class));
        assert_eq!(obj.object_flags().unwrap(), 0x2001);
        assert!(obj.is_control().unwrap());
        assert!(obj.is_automatable().unwrap());
        assert!(!obj.is_doc_object().unwrap());
    }
}

/// `as_typelib` answers only for TypeLib entries, `as_declare` only for
/// Declares.
#[test]
fn external_entries_check_their_kind() {
    let exprs = project("exprs");
    let map = exprs.address_map();
    let entries: Vec<_> = exprs.externals().unwrap().map(Result::unwrap).collect();
    assert_eq!(entries.len(), 2);
    for entry in &entries {
        assert_eq!(entry.kind().unwrap(), ExternalKind::DeclareFunction);
        assert!(entry.as_typelib(map).is_none());
        assert!(entry.as_declare(map).is_some());
    }
    let dispid = project("dispid");
    let map = dispid.address_map();
    let typelib = dispid
        .externals()
        .unwrap()
        .map(Result::unwrap)
        .find(|e| e.kind().unwrap() == ExternalKind::TypeLib)
        .unwrap();
    assert!(typelib.as_declare(map).is_none());
    assert_eq!(
        typelib
            .as_typelib(map)
            .unwrap()
            .typelib_guid(map)
            .unwrap()
            .to_string(),
        "{FCFB3D23-A0FA-1068-A738-08002B3371B5}"
    );
}

/// A Declare's external table entry is its DllFunctionCall descriptor: by
/// name (ordinal 0, flags 4), its +0x0C a resolve cache in the zero-filled
/// .data. The descriptors the constant pools' Declare stubs push name the
/// same DLL and function strings (control).
#[test]
fn declare_entries_are_dllfunctioncall_descriptors() {
    for name in ["exprs", "types", "vtable"] {
        let project = project(name);
        let map = project.address_map();
        let mut table = HashSet::new();
        for entry in project.externals().unwrap() {
            let declare = entry.unwrap().as_declare(map).unwrap();
            assert_eq!(declare.ordinal().unwrap(), 0, "{name}");
            assert_eq!(declare.flags().unwrap(), 0x0004, "{name}");
            assert!(!declare.is_by_ordinal());
            assert!(matches!(
                map.va_to_offset(declare.resolve_cache_va().unwrap()),
                Err(Error::RvaInBssRegion { .. })
            ));
            let stub = CallApiStub::from(declare);
            assert_eq!(stub.library_name(map).unwrap(), "kernel32");
            table.insert((
                stub.library_name_va().unwrap(),
                stub.function_name_va().unwrap(),
            ));
        }
        let mut pushed = HashSet::new();
        for obj in project.objects().unwrap() {
            let obj = obj.unwrap();
            let count = obj.info().constants_count().unwrap();
            for (_, entry) in obj.constants_pool().unwrap().entries(count) {
                if let Ok(PoolEntry::Declare(stub)) = entry {
                    pushed.insert((
                        stub.library_name_va().unwrap(),
                        stub.function_name_va().unwrap(),
                    ));
                }
            }
        }
        assert!(!pushed.is_empty(), "{name}");
        assert_eq!(pushed, table, "{name}");
    }
}

/// The entries `(offset, type)` of a variable descriptor table.
fn entries(
    table: &visualbasic::vb::publicbytes::ClassFormPublicBytes<'_>,
) -> Vec<(u16, ControlPropertyType)> {
    table
        .control_entries()
        .map(|e| (e.frame_offset().unwrap(), e.property_type()))
        .collect()
}

/// Modules use the variable descriptor format of classes: `flow`'s
/// `Private m_Log As String` is a String at 0, `statics` `Other` has its
/// `o_Text` String at 4 and no entry for the `Long`.
#[test]
fn module_variable_tables_use_the_shared_format() {
    let flow = object(project("flow"), "Program").public_bytes().unwrap();
    assert_eq!(flow.instance_size().unwrap(), 0x08);
    assert_eq!(entries(&flow), [(0, ControlPropertyType::String)]);

    let statics = project("statics");
    let other = object(statics, "Other").public_bytes().unwrap();
    assert_eq!(entries(&other), [(4, ControlPropertyType::String)]);
    let program = object(statics, "Program");
    let vars = entries(&program.public_bytes().unwrap());
    assert_eq!(
        vars,
        [
            (0x0C, ControlPropertyType::String),
            (0x10, ControlPropertyType::Udt),
            (0x24, ControlPropertyType::Array),
        ]
    );
    assert_eq!(
        program.public_bytes().unwrap().instance_size().unwrap(),
        0x44
    );
    assert_eq!(
        program.static_bytes().unwrap().instance_size().unwrap(),
        0x20
    );
    let holder = object(statics, "Holder");
    assert_eq!(
        holder.static_bytes().unwrap().instance_size().unwrap(),
        0x08
    );
    // A module without Static locals has no static table.
    assert!(object(statics, "Other").static_bytes().is_none());

    // data: m_Static(1 To 10) As Integer, m_Grid() As Double, m_Text As
    // String; one entry (the fixed array) needs initialization.
    let data = object(project("data"), "Program").public_bytes().unwrap();
    assert_eq!(data.data_size().unwrap(), 0x46);
    assert_eq!(data.property_count().unwrap(), 1);
    assert_eq!(data.control_count().unwrap(), 3);
    assert_eq!(
        entries(&data)[..2],
        [
            (0x04, ControlPropertyType::Array),
            (0x1C, ControlPropertyType::Array)
        ]
    );
}

/// The third variable of `data`'s module, after the dynamic array
/// `m_Grid()`, whose entry the runtime sizes 10 bytes (MSVBVM60 6.00.8176
/// `0x66039A60`).
#[test]
fn data_module_table_has_all_three_variables() {
    let data = object(project("data"), "Program").public_bytes().unwrap();
    assert_eq!(
        entries(&data),
        [
            (0x04, ControlPropertyType::Array),
            (0x1C, ControlPropertyType::Array),
            (0x20, ControlPropertyType::String),
        ]
    );
}

/// The PrivateObjectDescriptor counts: +0x10 the module-level variables
/// plus one per `Implements`, +0x12 the `Event`s, +0x38 the instance size.
#[test]
fn private_descriptor_counts() {
    for (name, class, members, events) in [
        ("calls", "Counter", 3, 1),
        ("calls", "Square", 2, 0),
        ("calls", "Shape", 0, 0),
        ("events", "Source", 1, 3),
        ("events", "Listener", 2, 0),
        ("data", "Item", 1, 0),
    ] {
        let obj = object(project(name), class);
        let pod = obj.private_object().unwrap();
        assert_eq!(pod.member_count().unwrap(), members, "{class}");
        assert_eq!(pod.event_count().unwrap(), events, "{class}");
    }
    for name in projects() {
        for obj in project(&name).objects().unwrap() {
            let obj = obj.unwrap();
            if let Some(pod) = obj.private_object() {
                assert_eq!(
                    pod.instance_size().unwrap(),
                    u32::from(obj.public_bytes().unwrap().instance_size().unwrap()),
                    "{name}"
                );
            }
        }
    }
}

/// The ProjectInfo2 records and parameter names interleave: the walk
/// reads past names and records with no type library, and stops at the
/// P-Code.
#[test]
fn project_info2_records_and_names() {
    let records = |name: &str| -> (Vec<String>, Vec<&'static str>) {
        let project = project(name);
        let map = project.address_map();
        let pi2 = project.object_table().project_info2_va().unwrap();
        let records = ControlTypeIter::new(map, pi2)
            .map(|r| r.control_name(map).unwrap().to_string())
            .collect();
        (records, read_name_strings(map, pi2))
    };
    assert_eq!(
        records("forms"),
        (
            [
                "Text1",
                "Label1",
                "Form",
                "Command1",
                "Gauge1",
                "UserControl"
            ]
            .map(String::from)
            .to_vec(),
            vec!["start", "s", "v", "NewValue"]
        )
    );
    let (dispid, names) = records("dispid");
    assert_eq!(dispid, ["Class", "Form", "Dial1", "UserControl"]);
    assert_eq!(names.len(), 12);
    assert_eq!(records("data"), (vec!["Class".to_string()], vec!["Name"]));
    assert_eq!(
        records("calls").1,
        [
            "v", "o", "a", "b", "factor", "c", "text", "which", "values", "Value", "Shape", "side"
        ]
    );
    // Control: projects of modules only have nothing after the header.
    for name in ["hello", "flow", "exprs", "late", "vtable"] {
        assert_eq!(records(name), (Vec::new(), Vec::new()), "{name}");
    }
}

/// `Sub Main` is found through the ProcDscInfo its P-Code stub loads.
#[test]
fn sub_main_names_its_procedure() {
    let mut found = 0;
    for name in projects() {
        let project = project(&name);
        let sub_main = project.vb_header().sub_main_va().unwrap();
        let entries = project.code_entrypoints().unwrap();
        let mains: Vec<_> = entries
            .iter()
            .filter(|e| e.kind == EntrypointKind::SubMain)
            .collect();
        if sub_main == 0 {
            assert!(mains.is_empty(), "{name}");
            continue;
        }
        assert_eq!(mains.len(), 1, "{name}");
        let main = mains[0];
        assert_eq!(main.va, sub_main);
        if name.ends_with("-native") {
            // Native: no method table to find it in.
            assert!(!main.is_pcode);
            assert_eq!(main.object_index, None);
            continue;
        }
        let stub = project.address_map().slice_from_va(sub_main, 12).unwrap();
        assert_eq!(
            (stub[0], stub[5], &stub[10..12]),
            (0xBA, 0xB9, &[0xFF, 0xE1][..])
        );
        let dsc = u32::from_le_bytes(stub[1..5].try_into().unwrap());
        assert!(main.is_pcode, "{name}");
        assert_eq!(main.proc_dsc_va, Some(dsc));
        assert_eq!(main.stub_va, Some(sub_main));
        let obj = project
            .objects()
            .unwrap()
            .nth(usize::from(main.object_index.unwrap()))
            .unwrap()
            .unwrap();
        let method = obj
            .methods()
            .unwrap()
            .nth(usize::from(main.method_index.unwrap()))
            .unwrap()
            .unwrap();
        let MethodEntry::PCode(method) = method else {
            panic!("{name}: not P-Code");
        };
        assert_eq!(method.proc_dsc_va(), dsc);
        found += 1;
    }
    assert_eq!(found, 19);
    let hello = project("hello")
        .code_entrypoints()
        .unwrap()
        .into_iter()
        .find(|e| e.kind == EntrypointKind::SubMain)
        .unwrap();
    assert_eq!((hello.object_index, hello.method_index), (Some(0), Some(1)));
}

/// Each method is listed once; a class's or form's method carries the
/// method link stub that enters it, and the link entries that are left
/// are the variable accessors.
#[test]
fn code_entries_list_each_method_once() {
    for name in projects() {
        let project = project(&name);
        let map = project.address_map();
        for obj in project.objects().unwrap() {
            let obj = obj.unwrap();
            let entries = obj.code_entries(None).unwrap();
            let pcode: Vec<_> = entries
                .iter()
                .filter(|e| e.kind == CodeEntryKind::PCode)
                .collect();
            assert_eq!(pcode.len(), obj.pcode_methods().unwrap().count(), "{name}");
            let dscs: HashSet<_> = pcode.iter().map(|e| e.proc_dsc_va.unwrap()).collect();
            assert_eq!(dscs.len(), pcode.len(), "{name}");
            let has_links = obj.optional_info().is_some();
            for entry in &pcode {
                let Some(stub) = entry.stub_va else {
                    assert!(!has_links, "{name}: method without its stub");
                    continue;
                };
                let code = map.slice_from_va(stub, 7).unwrap();
                assert_eq!(code[..3], [0x33, 0xC0, 0xBA]);
                assert_eq!(
                    u32::from_le_bytes(code[3..7].try_into().unwrap()),
                    entry.proc_dsc_va.unwrap()
                );
            }
            for entry in entries
                .iter()
                .filter(|e| e.kind == CodeEntryKind::NativeThunk)
            {
                assert_eq!(entry.method_index, None, "{name}");
                assert!(entry.proc_dsc_va.is_none_or(|d| !dscs.contains(&d)));
            }
        }
    }

    // forms Board: the vtable order differs from the method order; Reset
    // (a public member) is link 0.
    let board = object(project("forms"), "Board");
    let reset = method_index(&board, "Reset");
    let link0 = board.method_links().unwrap().next().unwrap().unwrap();
    let entry = board
        .code_entries(None)
        .unwrap()
        .into_iter()
        .find(|e| e.method_index == Some(reset) && e.kind == CodeEntryKind::PCode)
        .unwrap();
    assert_eq!(entry.name.as_deref(), Some("Reset"));
    assert_eq!(entry.stub_va, Some(link0.thunk_va));

    // The accessors: data Item.Name at 0x34 (Get, Put), events
    // Listener.m_Source at 0x38 (Get, Put, Set).
    for (name, class, offset, count) in [("data", "Item", 0x34, 2), ("events", "Listener", 0x38, 3)]
    {
        let obj = object(project(name), class);
        let thunks: Vec<_> = obj
            .code_entries(None)
            .unwrap()
            .into_iter()
            .filter(|e| e.kind == CodeEntryKind::NativeThunk)
            .collect();
        assert_eq!(thunks.len(), count, "{class}");
        let variables = obj
            .method_links()
            .unwrap()
            .filter(|l| {
                matches!(l.as_ref().unwrap().kind, MethodLinkKind::Variable { offset: o, .. } if o == offset)
            })
            .count();
        assert_eq!(variables, count, "{class}");
    }
}

/// A control's event sink slots hold its handlers' event stubs on disk.
/// Each stub loads the control's dispatch offset into eax, which
/// MethCallEngine subtracts from `this`, and enters its procedure.
#[test]
fn control_event_handlers() {
    let controls = project("controls");
    let form = object(controls, "Form1");
    let bindings = form.events(None).unwrap();
    let click = bindings
        .iter()
        .find(|b| b.control_name == "Command1" && b.event_slot == 0)
        .unwrap();
    assert_eq!(click.handler_va, 0x0040_1928);
    assert_eq!(click.label(), "Command1_Click");
    let handlers: Vec<_> = form
        .code_entries(None)
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == CodeEntryKind::EventHandler)
        .collect();
    assert_eq!(handlers.len(), bindings.len());
    assert!(handlers.iter().all(|e| e.method_index.is_some()));

    let mut checked = 0;
    for name in ["controls", "dispid", "forms"] {
        let project = project(name);
        let map = project.address_map();
        for obj in project.objects().unwrap() {
            let obj = obj.unwrap();
            for control in obj.controls().unwrap() {
                let control = control.unwrap();
                let Some(sink) = control.event_sink(map) else {
                    continue;
                };
                for (slot, va) in sink.connected_handlers() {
                    assert_eq!(control.event_handler_va(slot), Some(va));
                    let thunk = sink.resolve_handler_thunk(slot, map).unwrap();
                    assert_eq!(
                        thunk.this_adjust,
                        u32::from(control.info().dispatch_offset().unwrap()),
                        "{name}"
                    );
                    let jmp = map.slice_from_va(thunk.engine_thunk_va, 6).unwrap();
                    assert_eq!(jmp[..2], [0xFF, 0x25]);
                    let slot_va = u32::from_le_bytes(jmp[2..6].try_into().unwrap());
                    assert_eq!(
                        project.imports().by_slot(slot_va).unwrap().function(),
                        "MethCallEngine"
                    );
                    checked += 1;
                }
            }
        }
    }
    assert!(checked >= 5, "{checked}");
}

/// Legitimate values that are not errors: the null method links of a
/// class that implements an interface, a 0xFFFFFFFF method name entry, and
/// a module's Declare slots.
#[test]
fn iterators_accept_legitimate_values() {
    for (name, class, links) in [("calls", "Square", 6), ("events", "Ring", 28)] {
        let obj = object(project(name), class);
        let kinds: Vec<_> = obj
            .method_links()
            .unwrap()
            .map(|l| l.unwrap().kind)
            .collect();
        assert_eq!(kinds.len(), links);
        assert_eq!(kinds[..3], [MethodLinkKind::Empty; 3]);
        assert!(kinds[3..].iter().all(|k| *k != MethodLinkKind::Empty));
    }

    let kinds = object(project("types"), "Kinds");
    for index in [18, 19] {
        assert_eq!(kinds.method_name(index).unwrap(), MethodNameResult::Unnamed);
    }

    for (name, module, declares) in [
        ("exprs", "Program", 2),
        ("types", "Program", 3),
        ("vtable", "Program", 13),
        ("hello", "Module1", 0),
    ] {
        let program = object(project(name), module);
        let methods: Vec<_> = program.methods().unwrap().map(Result::unwrap).collect();
        assert_eq!(methods.len(), usize::from(program.method_count().unwrap()));
        assert!(
            methods[..declares]
                .iter()
                .all(|m| matches!(m, MethodEntry::Declare)),
            "{name}"
        );
        assert!(
            methods[declares..]
                .iter()
                .all(|m| matches!(m, MethodEntry::PCode(_))),
            "{name}"
        );
    }
    // No fixture has a null, runtime or native slot outside the Declares.
    for name in projects() {
        for obj in project(&name).objects().unwrap() {
            for method in obj.unwrap().methods().unwrap() {
                assert!(
                    matches!(
                        method.unwrap(),
                        MethodEntry::PCode(_) | MethodEntry::Declare
                    ),
                    "{name}"
                );
            }
        }
    }
}

/// The path of the `.vbp`, decoded from UTF-16LE.
#[test]
fn project_path_is_utf16() {
    for name in projects() {
        let path = project(&name).project_data().path().unwrap();
        assert_eq!(path, format!("*\\AZ:\\work\\{name}\\{name}.vbp"));
    }
}

/// The VA the entry point pushes must hold `"VB5!"`: a PE whose entry
/// starts `push imm32` without it is not a VB6 file.
#[test]
fn entry_point_push_needs_the_magic() {
    let bytes = std::fs::read(fixture("hello")).unwrap();
    let pe = u32::from_le_bytes(bytes[0x3C..0x40].try_into().unwrap()) as usize;
    let entry_rva = u32::from_le_bytes(bytes[pe + 0x28..pe + 0x2C].try_into().unwrap());
    // .text is mapped at file offset == RVA in the fixtures.
    let entry = entry_rva as usize;
    assert_eq!(bytes[entry], 0x68);
    let parse = |code: &[u8]| {
        let mut patched = bytes.clone();
        patched[entry..entry + code.len()].copy_from_slice(code);
        VbProject::from_bytes(&patched)
            .err()
            .and_then(|e| e.recognition_failure())
    };
    let entry_va = 0x0040_0000 + entry_rva;
    let mut push_ret = vec![0x68];
    push_ret.extend_from_slice(&entry_va.to_le_bytes());
    push_ret.push(0xC3);
    assert_eq!(parse(&push_ret), Some(RecognitionFailure::NotRecognized));
    assert_eq!(
        parse(&[0x68, 0x78, 0x56, 0x34, 0x12]),
        Some(RecognitionFailure::NotRecognized)
    );
    // Controls: no push at all; the original entry.
    assert_eq!(parse(&[0x55]), Some(RecognitionFailure::NotRecognized));
    assert_eq!(parse(&bytes[entry..entry + 5]), None);
}

/// Counts read from the file are capped by the data behind them: a
/// corrupt count ends quickly and the rest still parses.
#[test]
fn corrupt_counts_end_quickly() {
    let path = fixture("calls");
    let bytes = std::fs::read(&path).unwrap();
    let clean = project("calls");
    let counter = object(clean, "Counter");
    let opt = counter.optional_info().unwrap();
    let info_va = counter.descriptor().object_info_va().unwrap();
    // OptionalObjectInfo follows the 0x38-byte ObjectInfo.
    let opt_offset = clean.address_map().va_to_offset(info_va + 0x38).unwrap();
    assert_eq!(
        u32::from_le_bytes(
            bytes[opt_offset + 0x20..opt_offset + 0x24]
                .try_into()
                .unwrap()
        ),
        opt.control_count().unwrap()
    );
    let mut mutant = bytes.clone();
    mutant[opt_offset + 0x20..opt_offset + 0x24].copy_from_slice(&u32::MAX.to_le_bytes());
    mutant[opt_offset + 0x28..opt_offset + 0x2A].copy_from_slice(&u16::MAX.to_le_bytes());
    let mutant: &'static [u8] = Box::leak(mutant.into_boxed_slice());
    let mutated = VbProject::from_bytes(mutant).unwrap();

    let start = Instant::now();
    let entries = mutated.code_entrypoints().unwrap();
    let controls = object_of(&mutated, "Counter").controls().unwrap().count();
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "{:?}",
        start.elapsed()
    );
    assert!(controls <= mutant.len() / 0x28);
    // The other objects' entry points are those of the clean file.
    let program = |entries: &[visualbasic::CodeEntrypoint<'_>]| -> Vec<u32> {
        entries
            .iter()
            .filter(|e| e.object_index == Some(0))
            .map(|e| e.va)
            .collect()
    };
    assert_eq!(
        program(&entries),
        program(&clean.code_entrypoints().unwrap())
    );
    assert!(!program(&entries).is_empty());

    // exprs: a corrupt external count still yields the two Declares first.
    let bytes = std::fs::read(fixture("exprs")).unwrap();
    let clean = project("exprs");
    let pd = clean.vb_header().project_data_va().unwrap();
    let count_offset = clean.address_map().va_to_offset(pd + 0x238).unwrap();
    let mut mutant = bytes.clone();
    mutant[count_offset..count_offset + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    let mutant: &'static [u8] = Box::leak(mutant.into_boxed_slice());
    let mutated = VbProject::from_bytes(mutant).unwrap();
    let start = Instant::now();
    let externals: Vec<_> = mutated.externals().unwrap().collect();
    assert!(start.elapsed() < Duration::from_secs(5));
    assert!(externals.len() <= mutant.len() / 8);
    for entry in &externals[..2] {
        assert_eq!(
            entry.as_ref().unwrap().kind().unwrap(),
            ExternalKind::DeclareFunction
        );
    }
}

/// The object named `name` of a project that is not leaked.
fn object_of<'a, 'p: 'a>(project: &'p VbProject<'a>, name: &str) -> VbObject<'a, 'p> {
    project
        .objects()
        .unwrap()
        .map(Result::unwrap)
        .find(|o| o.name().unwrap() == name)
        .unwrap()
}

/// `activex` hosts three OCX controls and `ocx` its own `Knob`: each
/// component entry names the control's CLSID, events interface, the events
/// IIDs its hosted instances use (single and in a control array), the
/// number of events it declares and its licence key.
#[test]
fn components_describe_hosted_controls() {
    let activex = project("activex");
    let components: BTreeMap<String, _> = activex
        .components()
        .unwrap()
        .map(|c| (c.class_name().into_owned(), c))
        .collect();
    let winsock = &components["Winsock"];
    assert_eq!(winsock.ocx_filename(), "MSWINSCK.OCX");
    assert_eq!(
        winsock.clsid().unwrap().to_string(),
        "{248DD896-BB45-11CF-9ABC-0080C7E7B78D}"
    );
    assert_eq!(
        winsock.events_iid().unwrap().to_string(),
        "{248DD893-BB45-11CF-9ABC-0080C7E7B78D}"
    );
    assert_eq!(winsock.declared_event_count(), 7);
    assert_eq!(
        winsock.license_key().as_deref(),
        Some("2c49f800-c2dd-11cf-9ad6-0080c7e7b78d")
    );
    // Sink slot 9 + k is the event with DISPID event_dispids()[k]: Winsock's
    // events interface lists Error (6), DataArrival (0), Connect (1), ...
    assert_eq!(winsock.event_dispids(), [6, 0, 1, 2, 5, 3, 4]);
    assert!(winsock.bindable_properties().is_empty());
    assert_eq!(components["Inet"].declared_event_count(), 1);
    assert_eq!(components["Inet"].event_dispids(), [32]);
    assert_eq!(components["CommonDialog"].declared_event_count(), 0);
    // MaskEdBox's data-bindable properties: Text (DISPID 22, a BSTR, bound
    // by default), BackColor (-501, OLE_COLOR), ...
    let bindable = components["MaskEdBox"].bindable_properties();
    let names: Vec<_> = bindable.iter().map(|p| p.name).collect();
    assert_eq!(
        names,
        ["Text", "BackColor", "ForeColor", "Enabled", "BorderStyle"]
    );
    assert_eq!(
        (bindable[0].dispid, bindable[0].vartype, bindable[0].flags),
        (22, 8, 6)
    );
    assert_eq!(
        (bindable[1].dispid, bindable[1].vartype, bindable[1].flags),
        (-501, 0x1D, 0)
    );
    // The Common Controls host too. ProgressBar's mouse events (its first
    // three) convert x (parameter 2, OLE_XPOS_PIXELS, kind 6) and y (3,
    // kind 7) to the container's scale; its other events convert nothing.
    assert!(components.contains_key("ListView") && components.contains_key("TreeView"));
    let conversions = components["ProgressBar"].event_conversions();
    assert_eq!(conversions.len(), 10);
    for event in &conversions[..3] {
        let found: Vec<_> = event.iter().map(|c| (c.param, c.by_ref, c.kind)).collect();
        assert_eq!(
            found,
            [
                (2, false, ConversionKind::XPosPixels),
                (3, false, ConversionKind::YPosPixels)
            ]
        );
    }
    assert!(conversions[3..].iter().all(Vec::is_empty));
    assert!(winsock.event_conversions().is_empty());
    assert_eq!(winsock.license_key_length(), Some(0x48));
    // Extender capabilities: invisible at run time (Winsock), visible
    // (ListView), visible with bindable properties (MaskEdBox), alignable and
    // acting like a label (ProgressBar).
    assert_eq!(winsock.extender_flags(), 0x0B60);
    assert_eq!(components["ListView"].extender_flags(), 0x1766);
    assert_eq!(components["MaskEdBox"].extender_flags(), 0x1776);
    assert_eq!(components["ProgressBar"].extender_flags(), 0x53E6);
    // The form's Winsock1 and the Peers control array name the two
    // instance events IIDs.
    let client = object(activex, "Client");
    let guid_of = |name: &str| {
        *client
            .controls()
            .unwrap()
            .map(Result::unwrap)
            .find(|c| c.name() == name)
            .unwrap()
            .guid()
            .unwrap()
    };
    assert_eq!(winsock.instance_events_iid(), Some(guid_of("Winsock1")));
    assert_eq!(winsock.array_events_iid(), Some(guid_of("Peers")));

    // A project's own UserControl: its CLSID and events IID are the
    // object's; it has no licence key.
    let ocx = project("ocx");
    let knob = ocx.components().unwrap().next().unwrap();
    let object = object(ocx, "Knob");
    assert_eq!(knob.clsid(), object.object_clsid());
    assert_eq!(
        knob.events_iid(),
        object.events_iids().next().map(|(_, guid)| guid)
    );
    assert_eq!(knob.declared_event_count(), 2);
    assert_eq!(knob.license_key(), None);
    assert_eq!(knob.license_key_length(), None);
}

/// A hosted control's sink slots 0-8 are its extender's events: `ocx`
/// `Panel.Knob1_GotFocus` is slot 0. Its own events follow from slot 9
/// (`Turned` 9, `Reset` 10) and are named only in its type library.
#[test]
fn hosted_controls_name_their_extender_events() {
    let ocx = project("ocx");
    let panel = object(ocx, "Panel");
    let bindings = panel.events(None).unwrap();
    let knob1: Vec<_> = bindings
        .iter()
        .filter(|b| b.control_name == "Knob1")
        .map(|b| (b.event_slot, b.event_name))
        .collect();
    assert_eq!(knob1, [(0, Some("GotFocus")), (9, None), (10, None)]);
}

/// The designer of each object, from its own ControlInfo: an MDIForm and
/// an MDI child form (`mdi`), a UserDocument (`docs`, a DLL), a
/// UserControl and a PropertyPage (`ocx`, an OCX).
#[test]
fn designers_of_every_kind() {
    use visualbasic::vb::designer::Designer;
    for (fixture, name, designer) in [
        ("mdi", "Frame", Designer::MdiForm),
        ("mdi", "Child", Designer::Form),
        ("docs", "Page", Designer::UserDocument),
        ("ocx", "Knob", Designer::UserControl),
        ("ocx", "KnobPage", Designer::PropertyPage),
    ] {
        assert_eq!(
            object(project(fixture), name).designer(),
            Some(designer),
            "{name}"
        );
    }
    assert_eq!(object(project("docs"), "Ledger").designer(), None);
}

/// The method link table's order (`members`): plain public variables'
/// accessors, three null entries per `Implements`, public `WithEvents`
/// accessors, public procedures, the other methods, private `WithEvents`
/// accessors.
#[test]
fn method_links_in_vtable_order() {
    let members = project("members");
    let kinds = |name: &str| -> Vec<String> {
        let object = object(members, name);
        object
            .method_links()
            .unwrap()
            .map(|link| match link.unwrap().kind {
                MethodLinkKind::Empty => "null".to_string(),
                MethodLinkKind::Variable { offset, .. } => format!("var{offset:X}"),
                MethodLinkKind::Procedure { .. } => "proc".to_string(),
                other => format!("{other:?}"),
            })
            .collect()
    };
    let nulls = |name: &str| kinds(name).iter().filter(|k| *k == "null").count();
    assert_eq!(nulls("Two"), 6);
    assert_eq!(nulls("Three"), 9);
    assert_eq!(
        kinds("Mixed"),
        [
            "var34", "var34", "var38", "var38", "null", "null", "null", "var50", "var50", "var50",
            "proc", "proc", "proc", "proc", "proc", "proc", "proc", "var3C", "var3C", "var3C"
        ]
    );
}

/// A designer object's own form record: control id 0, its name, its
/// cType, then its properties, each decoded with its designer's table and
/// matching the source (`StartUpPosition = 3`, `HScrollSmallChange = 225`,
/// `PaletteMode = 0`, `ScaleMode = 1` with the default drawing flags).
#[test]
fn designer_records_decode_with_their_own_table() {
    use visualbasic::vb::{formdata::FormControlType, property::PropertyValue};
    for (fixture, name, form_type, property, value) in [
        (
            "forms",
            "Board",
            FormControlType::Form,
            "StartUpPosition",
            "3",
        ),
        (
            "mdi",
            "Frame",
            FormControlType::MDIForm,
            "StartUpPosition",
            "3",
        ),
        (
            "mdi",
            "Child",
            FormControlType::Form,
            "Caption",
            "\"Child\"",
        ),
        (
            "docs",
            "Page",
            FormControlType::UserDocument,
            "HScrollSmallChange",
            "225",
        ),
        (
            "ocx",
            "KnobPage",
            FormControlType::PropertyPage,
            "PaletteMode",
            "0",
        ),
        (
            "ocx",
            "Knob",
            FormControlType::UserControl,
            "ScaleMode",
            "1 flags=0x42",
        ),
    ] {
        let project = project(fixture);
        let object = object(project, name);
        let form_data = project
            .gui_entries()
            .unwrap()
            .find_map(|entry| {
                object
                    .form_data_from_gui_entry(&entry)
                    .filter(|data| data.form_name() == name)
            })
            .unwrap_or_else(|| panic!("{name}: no form data"));
        assert_eq!(form_data.form_type(), form_type, "{name}");
        let decoded: Vec<_> = form_data.form_properties_decoded().collect();
        assert!(
            !decoded
                .iter()
                .any(|p| matches!(p.value, PropertyValue::Flag) && p.name == "_"),
            "{name}"
        );
        let found = decoded
            .iter()
            .find(|p| p.name == property)
            .unwrap_or_else(|| panic!("{name}: no {property}"));
        assert_eq!(found.value.to_string(), value, "{name}");
    }
}

/// A control's bounds serialize as four `i16` twips values, or `0x8000`
/// and four `i32` when one does not fit (`geometry`); a Line's coordinates
/// and a Timer's position are 4-byte integers (the source's 2400.5 is
/// stored as 2400).
#[test]
fn control_bounds_and_coordinates() {
    use visualbasic::vb::property::{ControlBounds, PropertyValue};
    let project = project("geometry");
    let layout = object(project, "Layout");
    let form_data = project
        .gui_entries()
        .unwrap()
        .find_map(|entry| layout.form_data_from_gui_entry(&entry))
        .unwrap();
    let value = |control: &str, property: &str| {
        form_data
            .controls()
            .iter()
            .find(|c| c.name() == control)
            .unwrap()
            .properties()
            .find(|p| p.name == property)
            .unwrap_or_else(|| panic!("{control}.{property}"))
            .value
    };
    let bounds = |control: &str| match value(control, "Bounds") {
        PropertyValue::Bounds(bounds) => bounds,
        other => panic!("{control}: {other:?}"),
    };
    let rect = |left, top, width, height| ControlBounds {
        left,
        top,
        width,
        height,
    };
    assert_eq!(bounds("Hidden"), rect(-1200, -600, 1215, 495));
    assert_eq!(bounds("Far"), rect(40000, 36000, 33000, 495));
    assert_eq!(bounds("Inner"), rect(60, 60, 1935, 285));
    // The properties after an 18-byte bounds still decode.
    assert!(matches!(value("Far", "TabIndex"), PropertyValue::Int16(1)));
    assert!(matches!(value("Diagonal", "X2"), PropertyValue::Long(2400)));
    assert!(matches!(value("Diagonal", "Y1"), PropertyValue::Long(240)));
    assert!(matches!(value("Ticker", "Top"), PropertyValue::Long(3480)));
}

/// A UserControl's extender flags (component info +0x84) follow its
/// `MiscStatus`, one designer property at a time (`extender`, base 0x1766).
#[test]
fn extender_flags_follow_misc_status() {
    let project = project("extender");
    let flags: BTreeMap<String, u16> = project
        .components()
        .unwrap()
        .map(|c| (c.class_name().into_owned(), c.extender_flags()))
        .collect();
    for (class, expected) in [
        ("Plain", 0x1766),
        ("AlignBox", 0x17E6),
        ("NoFocus", 0x1362),
        ("ButtonLike", 0x3766),
        ("Ghost", 0x0B60),
        ("Holder", 0x9766),
        ("Forwarder", 0x5366),
        ("Bound", 0x1766),
    ] {
        assert_eq!(flags[class], expected, "{class}");
    }
}

/// The event parameters a hosted control's runtime converts, one event per
/// stdole type (`extender` `Coords`): pixels, HIMETRIC, container units
/// (none), by reference (none), the other stdole aliases (none), and a
/// by-value Variant.
#[test]
fn event_conversions_by_parameter_type() {
    use ConversionKind::*;
    let project = project("extender");
    let coords = project
        .components()
        .unwrap()
        .find(|c| c.class_name() == "Coords")
        .unwrap();
    let kinds: Vec<Vec<_>> = coords
        .event_conversions()
        .iter()
        .map(|event| event.iter().map(|c| (c.param, c.by_ref, c.kind)).collect())
        .collect();
    assert_eq!(
        kinds,
        [
            vec![
                (0, false, XPosPixels),
                (1, false, YPosPixels),
                (2, false, XSizePixels),
                (3, false, YSizePixels)
            ],
            vec![
                (0, false, XPosHimetric),
                (1, false, YPosHimetric),
                (2, false, XSizeHimetric),
                (3, false, YSizeHimetric)
            ],
            vec![],
            vec![],
            vec![(0, false, Variant)],
            vec![],
        ]
    );
    assert_eq!(XSizeHimetric.stdole_type(), Some("OLE_XSIZE_HIMETRIC"));
    assert_eq!(coords.control_flags(), 0x2057);
}

/// Every property `props`' source sets, on the form and on each intrinsic
/// control, decodes from the compiled form record under the same name and
/// with the same value, except where the designer stores another value:
/// `RightToLeft` is not in the records this build writes, a TextBox with a
/// `PasswordChar` stores `IMEMode` 3, and a FileListBox's height is fitted
/// to whole items. `ScaleMode` carries the user scale and `AutoRedraw` and
/// `FontTransparent`; a menu's items follow it in the menu section.
#[test]
fn form_records_match_their_source() {
    use visualbasic::vb::{formdata::FormControlType, property::PropertyValue};

    // `(control, property, value)` from the `.frm`: `Begin VB.<Class>
    // <Name>` blocks, `BeginProperty` blocks skipped, comments dropped.
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/props/Props.frm"),
    )
    .unwrap();
    let mut expected: Vec<(String, String, String)> = Vec::new();
    let mut names: Vec<String> = Vec::new();
    let mut in_property = 0;
    for line in source.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("Begin VB.") {
            names.push(rest.split_whitespace().nth(1).unwrap().to_string());
        } else if line.starts_with("BeginProperty") {
            in_property += 1;
        } else if line.starts_with("EndProperty") {
            in_property -= 1;
        } else if line == "End" {
            names.pop();
        } else if in_property == 0
            && let Some(control) = names.last()
            && let Some((key, value)) = line.split_once('=')
        {
            let value = value.split('\'').next().unwrap().trim();
            expected.push((control.clone(), key.trim().to_string(), value.to_string()));
        }
    }

    let project = project("props");
    let form = object(project, "Props");
    let form_data = project
        .gui_entries()
        .unwrap()
        .find_map(|entry| form.form_data_from_gui_entry(&entry))
        .unwrap();
    let mut decoded: BTreeMap<String, Vec<(&'static str, PropertyValue)>> = BTreeMap::new();
    decoded.insert(
        "Props".to_string(),
        form_data
            .form_properties_decoded()
            .map(|p| (p.name, p.value))
            .collect(),
    );
    for control in form_data.controls() {
        decoded.insert(
            control.name().into_owned(),
            control.properties().map(|p| (p.name, p.value)).collect(),
        );
    }

    // A number in the source: `&H..&` hex, `^A` a menu shortcut's code.
    let number = |text: &str| -> Option<f64> {
        if let Some(hex) = text.strip_prefix("&H") {
            return i64::from_str_radix(hex.trim_end_matches('&'), 16)
                .ok()
                .map(|n| n as f64);
        }
        if let Some(&[letter]) = text.strip_prefix('^').map(str::as_bytes) {
            return Some(f64::from(letter - b'A' + 1));
        }
        text.parse().ok()
    };
    let mut mismatches = Vec::new();
    for (control, key, value) in &expected {
        let properties = &decoded[control];
        let found = properties
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, v)| v);
        let bounds = properties.iter().find_map(|(_, v)| match v {
            PropertyValue::Bounds(b) => Some(*b),
            _ => None,
        });
        let scale = properties.iter().find_map(|(_, v)| match v {
            PropertyValue::Scale(s) => Some(*s),
            _ => None,
        });
        let client = properties.iter().find_map(|(_, v)| match v {
            PropertyValue::ClientRect(r) => Some(*r),
            _ => None,
        });
        let flag = |set: bool| if set { "-1" } else { "0" }.to_string();
        let user = scale.and_then(|s| s.user_scale);
        let actual: Option<String> = match (key.as_str(), found, bounds) {
            ("ScaleMode", _, _) => scale.map(|s| s.mode.to_string()),
            ("AutoRedraw", _, _) => scale.map(|s| flag(s.auto_redraw())),
            ("FontTransparent", _, _) => scale.map(|s| flag(s.font_transparent())),
            ("ScaleLeft", _, _) => user.map(|u| u.left.to_string()),
            ("ScaleTop", _, _) => user.map(|u| u.top.to_string()),
            // Stored as twips per unit: checked below.
            ("ScaleWidth" | "ScaleHeight", _, _) => Some(value.clone()),
            ("ClientLeft", _, _) => client.map(|r| r.left.to_string()),
            ("ClientTop", _, _) => client.map(|r| r.top.to_string()),
            ("ClientWidth", _, _) => client.map(|r| r.width.to_string()),
            ("ClientHeight", _, _) => client.map(|r| r.height.to_string()),
            ("Left", None, Some(b)) => Some(b.left.to_string()),
            ("Top", None, Some(b)) => Some(b.top.to_string()),
            ("Width", None, Some(b)) => Some(b.width.to_string()),
            ("Height", None, Some(b)) => Some(b.height.to_string()),
            (_, Some(PropertyValue::Color(c)), _) => Some(c.to_string()),
            (_, Some(PropertyValue::Str(s) | PropertyValue::TagStr(s)), _) => {
                Some(format!("\"{s}\""))
            }
            (_, Some(v), _) => Some(v.to_string()),
            _ => None,
        };
        let same = match (&actual, number(value)) {
            (Some(actual), _) if actual == value => true,
            // A Boolean `True` (-1) is the byte 255.
            (Some(actual), Some(n)) => {
                actual.parse::<f64>().ok() == Some(n) || (n == -1.0 && actual == "255")
            }
            _ => false,
        };
        if !same {
            mismatches.push(format!("{control}.{key}={value} {actual:?}"));
        }
    }
    let mut stored_otherwise = vec![
        "PFileListBox.Height=540 Some(\"420\")".to_string(),
        "PTextBox.IMEMode=1 Some(\"3\")".to_string(),
    ];
    stored_otherwise.extend(
        [
            "Feed",
            "PCheckBox",
            "PComboBox",
            "PCommandButton",
            "PFrame",
            "PLabel",
            "PListBox",
            "POptionButton",
            "PPictureBox",
            "PTextBox",
            "Props",
        ]
        .map(|control| format!("{control}.RightToLeft=-1 None")),
    );
    mismatches.sort();
    stored_otherwise.sort();
    assert_eq!(mismatches, stored_otherwise);

    // `ScaleWidth = 30` and `ScaleHeight = 40` across the same 540 twips.
    let user = match decoded["PUser"]
        .iter()
        .find(|(name, _)| *name == "ScaleMode")
    {
        Some((_, PropertyValue::Scale(s))) => s.user_scale.unwrap(),
        other => panic!("{other:?}"),
    };
    assert_eq!(user.twips_per_unit_x * 30.0, 540.0);
    assert_eq!(user.twips_per_unit_y * 40.0, 540.0);

    // The menu tree, by depth and parent.
    let menus: Vec<(String, u16, Option<String>)> = form_data
        .controls()
        .iter()
        .filter(|c| c.control_type() == FormControlType::Menu)
        .map(|c| {
            let parent = c
                .parent_index()
                .map(|i| form_data.controls()[i].name().into_owned());
            (c.name().into_owned(), c.depth(), parent)
        })
        .collect();
    let item = |name: &str, depth: u16, parent: Option<&str>| {
        (name.to_string(), depth, parent.map(str::to_string))
    };
    assert_eq!(
        menus,
        vec![
            item("PMenu", 0, None),
            item("PItem", 1, Some("PMenu")),
            item("PSub", 1, Some("PMenu")),
            item("PDeep1", 2, Some("PSub")),
            item("PDeep2", 2, Some("PSub")),
            item("PLast", 1, Some("PMenu")),
            item("PTop2", 0, None),
            item("PC1", 1, Some("PTop2")),
            item("PTop3", 0, None),
        ]
    );
}

/// A class's `Instancing`: exposed or not (object type 0x800), global
/// (0x20000), and how its registration record creates it (`server`, an
/// ActiveX EXE with one class per value); a Standard EXE's classes are
/// private, and a module or a designer object has none.
#[test]
fn classes_report_their_instancing() {
    use visualbasic::project::Instancing;

    let server = project("server");
    for (class, instancing) in [
        ("Hidden", Instancing::Private),
        ("Handed", Instancing::PublicNotCreatable),
        ("Single", Instancing::SingleUse),
        ("GlobalSingle", Instancing::GlobalSingleUse),
        ("Multi", Instancing::MultiUse),
        ("GlobalMulti", Instancing::GlobalMultiUse),
    ] {
        assert_eq!(
            object(server, class).instancing(),
            Some(instancing),
            "{class}"
        );
    }
    assert_eq!(object(server, "Program").instancing(), None);
    assert_eq!(
        object(project("docs"), "Ledger").instancing(),
        Some(Instancing::MultiUse)
    );
    assert_eq!(object(project("docs"), "Page").instancing(), None);
    assert_eq!(
        object(project("calls"), "Counter").instancing(),
        Some(Instancing::Private)
    );
    assert_eq!(object(project("ocx"), "Knob").instancing(), None);
}
