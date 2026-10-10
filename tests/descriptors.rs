//! The descriptors the constant pool holds for the P-Code that names them,
//! and the structures whose layout the extents walk measures, checked on
//! the compiled fixtures against their source.

#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]

mod common;

use visualbasic::{
    VbObject, VbProject,
    pcode::decoder::PoolEntryRole,
    project::MethodLinkKind,
    vb::{
        control::{ControlKind, DispidMapping, Guid},
        events::EventSinkVtable,
        pooldesc::{
            ArrayDescriptor, CreationDescriptor, IoItems, IoSeparator, RecordIoDescriptor,
            RecordIoKind,
        },
        projectinfo2::ControlTypeIter,
        typeref::TypeLibRef,
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

/// The VA of every constant pool entry an instruction `mnemonic` names in
/// `project`, with the role the opcode gives it.
fn pool_entries(project: &'static VbProject<'static>, mnemonic: &str) -> Vec<(u32, PoolEntryRole)> {
    let map = project.address_map();
    let mut out = Vec::new();
    for object in project.objects().unwrap().map(Result::unwrap) {
        for method in object.pcode_methods().unwrap().map(Result::unwrap) {
            let pool = method.constant_pool(map);
            let code = method.pcode_bytes();
            for instruction in method.instructions().unwrap().flatten() {
                if instruction.info.mnemonic != mnemonic {
                    continue;
                }
                for reference in instruction.pool_references(code) {
                    out.push((pool.va_at(reference.index).unwrap(), reference.role));
                }
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// `records`' `Get #f, 1, o` of an `Outer`: nine entries, three of them
/// `Inner` records (one, a fixed array of three and a dynamic array) whose
/// descriptors follow the entries; `coverage`'s `FileRec` has one, its
/// `String * 8` 0x14 bytes in.
#[test]
fn record_io_descriptors_describe_their_records() {
    let records = project("records");
    let [(va, role)] = pool_entries(records, "GetRecOwn4")[..] else {
        panic!("one GetRecOwn4 descriptor");
    };
    assert_eq!(role, PoolEntryRole::RecordIo);
    let outer = RecordIoDescriptor::at(records.address_map(), va).unwrap();
    let entries = outer.entries();
    assert_eq!(entries.len(), 9);
    let nested: Vec<usize> = entries
        .iter()
        .enumerate()
        .filter(|(_, e)| e.kind() == RecordIoKind::Record)
        .map(|(i, _)| i)
        .collect();
    assert_eq!(nested.len(), 3, "In1, Ins(2) and DynIn()");
    assert!(entries[nested[1]].is_fixed_array());
    assert_eq!(entries[nested[1]].count, 3);
    assert!(entries[nested[2]].is_dynamic_array());
    for index in nested {
        let inner = outer.nested(index).unwrap();
        let kinds: Vec<RecordIoKind> = inner.entries().iter().map(|e| e.kind()).collect();
        assert_eq!(kinds[0], RecordIoKind::String, "Inner.S");
    }
    assert_eq!(outer.extent_size(), Some(0xDC));

    let coverage = project("coverage");
    let [(va, _)] = pool_entries(coverage, "GetRecOwn4")[..] else {
        panic!("one GetRecOwn4 descriptor");
    };
    let file_rec = RecordIoDescriptor::at(coverage.address_map(), va).unwrap();
    assert_eq!(file_rec.record_size().unwrap(), 0x24);
    let [entry] = file_rec.entries()[..] else {
        panic!("one entry");
    };
    assert_eq!(
        (entry.gap, entry.kind(), entry.value),
        (0x14, RecordIoKind::FixedString, 8)
    );
    assert_eq!(file_rec.extent_size(), Some(22));
}

/// `forms`' `Print s`: one `String` item and a new line.
#[test]
fn io_items_read_their_items() {
    let forms = project("forms");
    let [(va, role)] = pool_entries(forms, "PrintObject")[..] else {
        panic!("one PrintObject list");
    };
    assert_eq!(role, PoolEntryRole::IoItems);
    let items = IoItems::at(forms.address_map(), va).unwrap();
    let [item] = items.items().collect::<Vec<_>>()[..] else {
        panic!("one item");
    };
    assert_eq!(
        (item.item_type(), item.separator()),
        (8, IoSeparator::NewLine)
    );
    assert_eq!(items.size(), 3);
}

/// `data`'s `Record.Flags(3) As Byte` and `records`' `Outer.Grid(1 To 2, 3
/// To 5) As Integer`, the fixed arrays `AryInRecLdPr` indexes.
#[test]
fn array_descriptors_give_their_bounds() {
    let data = project("data");
    let [(va, role)] = pool_entries(data, "AryInRecLdPr")[..] else {
        panic!("one array");
    };
    assert_eq!(role, PoolEntryRole::ArrayDescriptor);
    let flags = ArrayDescriptor::at(data.address_map(), va).unwrap();
    assert_eq!(
        (flags.dimensions().unwrap(), flags.element_size().unwrap()),
        (1, 1)
    );
    assert_eq!(flags.bounds(), [(4, 0)]);
    assert_eq!(flags.size(), 0x18);

    let records = project("records");
    let [(va, _)] = pool_entries(records, "AryInRecLdPr")[..] else {
        panic!("one array");
    };
    let grid = ArrayDescriptor::at(records.address_map(), va).unwrap();
    assert_eq!(grid.bounds(), [(3, 3), (2, 1)], "rightmost dimension first");
    assert_eq!(grid.size(), 0x20);
}

/// `New` of a class outside the project names a creation descriptor
/// (`forms`: VB's `Global` as `VBGlobal`); of a project class, its
/// `ObjectInfo`; `members`' `Holder.Auto As New Collection` reaches its
/// descriptor through its accessors' pushed pool index.
#[test]
fn creation_descriptors_name_their_class() {
    let forms = project("forms");
    let map = forms.address_map();
    let guid = |va: u32| Guid::from_bytes(map.slice_from_va(va, 16).unwrap()).unwrap();
    let created: Vec<(u32, PoolEntryRole)> = pool_entries(forms, "NewIfNullPr");
    assert!(
        created
            .iter()
            .all(|(_, role)| *role == PoolEntryRole::Creation)
    );
    let global = created
        .iter()
        .find_map(|&(va, _)| CreationDescriptor::at(map, va))
        .unwrap();
    assert_eq!(global.flags().unwrap(), 2);
    assert_eq!(
        guid(global.clsid_va().unwrap()).to_string(),
        "{FCFB3D23-A0FA-1068-A738-08002B3371B5}"
    );

    let members = project("members");
    let holder = object(members, "Holder");
    let pool_va = holder.info().constants_va().unwrap();
    let pushed: Vec<[u32; 2]> = holder
        .method_links()
        .unwrap()
        .filter_map(|link| match link.unwrap().kind {
            MethodLinkKind::Variable {
                pushed: Some(pushed),
                ..
            } => Some(pushed),
            _ => None,
        })
        .collect();
    assert!(!pushed.is_empty());
    assert!(
        pushed
            .iter()
            .all(|&[index, pool]| (index, pool) == (0, pool_va))
    );
    let entry = holder.constants_pool().unwrap().va_at(0).unwrap();
    let auto = CreationDescriptor::at(members.address_map(), entry).unwrap();
    assert_eq!(
        Guid::from_bytes(
            members
                .address_map()
                .slice_from_va(auto.clsid_va().unwrap(), 16)
                .unwrap()
        )
        .unwrap()
        .to_string(),
        "{A4C4671C-499F-101B-BB78-00AA00383CBB}",
        "VBA's Collection"
    );
}

/// The private object descriptor lists the layouts of a class's `Type`s in
/// declaration order, one layout per shape: `udts` `Shapes` declares five,
/// `Second.Pair` has `Shapes.PrivT`'s members and shares its layout.
#[test]
fn record_layouts_follow_the_type_declarations() {
    let udts = project("udts");
    let sizes: Vec<u16> = object(udts, "Shapes")
        .record_layouts()
        .unwrap()
        .iter()
        .map(|(_, layout)| layout.record_size().unwrap())
        .collect();
    // Point, Named, Third, PrivT, Holder.
    assert_eq!(sizes, [0x08, 0x20, 0x0C, 0x08, 0x08]);
    let shapes = object(udts, "Shapes").record_layouts().unwrap();
    let second = object(udts, "Second").record_layouts().unwrap();
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].0, shapes[3].0, "Pair shares PrivT's layout");
    for name in projects() {
        let project = project(&name);
        for object in project.objects().unwrap().map(Result::unwrap) {
            assert!(object.record_layouts().is_ok(), "{name}");
        }
    }
}

/// A `WithEvents` variable's entry maps each event of its source to the
/// procedure that handles it: `events` `Listener.m_Source` handles
/// `Started`, `Ticked` and `Named`.
#[test]
fn dispid_maps_name_the_handlers() {
    let events = project("events");
    let listener = object(events, "Listener");
    let map = events.address_map();
    let control = listener
        .controls()
        .unwrap()
        .map(Result::unwrap)
        .find(|c| c.info().kind().unwrap() == ControlKind::WithEvents)
        .unwrap();
    let pairs = control.info().dispid_map(map);
    assert_eq!(
        pairs.iter().map(|p| p.source).collect::<Vec<_>>(),
        [1, 2, 3],
        "event numbers"
    );
    assert!(
        pairs
            .iter()
            .all(|DispidMapping { handler, .. }| handler >> 16 == 0x6003)
    );
    // The sink vtable follows the map.
    let sink = control.info().event_sink_vtable_va().unwrap();
    assert_eq!(
        sink,
        control.info().dispid_map_va().unwrap() + 8 * u32::try_from(pairs.len()).unwrap()
    );
}

/// An `Implements`' sink vtable has four zero slots after the interface's
/// members: `calls` `Square` implements `Shape`.
#[test]
fn implements_sinks_end_in_four_zero_slots() {
    let calls = project("calls");
    let map = calls.address_map();
    let square = object(calls, "Square");
    let control = square
        .controls()
        .unwrap()
        .map(Result::unwrap)
        .find(|c| c.info().kind().unwrap() == ControlKind::Implements)
        .unwrap();
    let info = control.info();
    let va = info.event_sink_vtable_va().unwrap();
    let sink = EventSinkVtable::parse(map.slice_from_va(va, 0x18).unwrap(), info).unwrap();
    let slots = usize::from(info.event_handler_slots().unwrap());
    assert_eq!(sink.size(), 0x18 + 4 * (slots + 4));
    let tail = &map.slice_from_va(va, sink.size()).unwrap()[sink.size() - 16..sink.size()];
    assert_eq!(tail, [0; 16]);
}

/// The VB header is 0x78 bytes, its last 16 `GUID_NULL`; every type library
/// reference ends in one address the binary repeats.
#[test]
fn header_and_type_library_references_have_their_size() {
    for name in projects() {
        let project = project(&name);
        assert_eq!(
            project.vb_header().reserved_guid().unwrap().to_string(),
            "{00000000-0000-0000-0000-000000000000}",
            "{name}"
        );
        assert_eq!(project.vb_header().as_bytes().len(), 0x78);
    }
    let typerefs = project("typerefs");
    let map = typerefs.address_map();
    let mut tails = Vec::new();
    for entry in ControlTypeIter::new(map, typerefs.object_table().project_info2_va().unwrap()) {
        if let Ok(typelib) = TypeLibRef::at(map, entry.typelib_va) {
            let bytes = map
                .slice_from_va(entry.typelib_va, TypeLibRef::SIZE)
                .unwrap();
            tails.push(u32::from_le_bytes(bytes[0x24..0x28].try_into().unwrap()));
            assert_eq!(typelib.flags().unwrap(), 0);
        }
    }
    tails.dedup();
    assert_eq!(tails, [0x02085A90]);
}
