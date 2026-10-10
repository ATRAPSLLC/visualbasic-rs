//! The extents [`VbProject::structure_extents`] states, checked on the
//! compiled fixtures against what the structures' own fields say and against
//! every native code address the crate knows.

#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]

mod common;

use std::collections::BTreeSet;

use goblin::{
    options::ParseMode,
    pe::{PE, options::ParseOptions},
};
use visualbasic::{
    VbProject,
    extents::{StructureExtent, StructureKind},
    vb::{
        object::{ObjectInfo, OptionalObjectInfo, PublicObjectDescriptor},
        privateobj::PrivateObjectDescriptor,
    },
};

use crate::common::{fixture, projects};

/// Reads fixture `name`, leaked so its project outlives the test's borrows.
fn project(name: &str) -> (&'static [u8], &'static VbProject<'static>) {
    let bytes: &'static [u8] = Box::leak(std::fs::read(fixture(name)).unwrap().into_boxed_slice());
    (
        bytes,
        Box::leak(Box::new(VbProject::from_bytes(bytes).unwrap())),
    )
}

/// The bytes `extent` covers.
fn bytes(project: &VbProject<'static>, extent: &StructureExtent) -> &'static [u8] {
    let len = (extent.extent.end - extent.extent.start) as usize;
    &project
        .address_map()
        .slice_from_va(extent.extent.start, len)
        .unwrap()[..len]
}

/// The PE entry point goblin reads, which [`VbProject::native_entries`]
/// must hold.
fn pe_entry(file: &[u8], project: &VbProject<'static>) -> u32 {
    let options = ParseOptions::default()
        .with_parse_resources(false)
        .with_parse_mode(ParseMode::Permissive);
    let pe = PE::parse_with_opts(file, &options).unwrap();
    project.address_map().image_base()
        + pe.header
            .optional_header
            .unwrap()
            .standard_fields
            .address_of_entry_point
}

/// **No extent covers native code.** Every address of native code the crate
/// knows lies outside every extent, on every fixture: the entries
/// ([`VbProject::native_entries`]) and the code each unwind record names.
#[test]
fn no_extent_covers_code() {
    for name in projects() {
        let (file, project) = project(&name);
        let extents = project.structure_extents();
        let mut code = project.native_entries();
        assert!(
            code.contains(&pe_entry(file, project)),
            "{name}: entry point"
        );
        for procedure in project.unwind_records() {
            code.extend(procedure.info.code_vas(project.address_map()));
        }
        for va in &code {
            if let Some(structure) = extents.iter().find(|s| s.extent.contains(va)) {
                panic!("{name}: code at {va:#x} inside {structure:?}");
            }
        }
    }
}

/// **Extents do not overlap.** Two structures share bytes only when they
/// are the same structure, which the walk states once.
#[test]
fn extents_are_disjoint() {
    for name in projects() {
        let (_, project) = project(&name);
        let extents = project.structure_extents();
        for pair in extents.windows(2) {
            assert!(
                pair[0].extent.end <= pair[1].extent.start,
                "{name}: {:?} overlaps {:?}",
                pair[0],
                pair[1]
            );
        }
    }
}

/// Every fixed-size structure has its size, every name ends at its NUL,
/// every string constant's length prefix gives its length, and every
/// procedure's P-Code is the bytes its method reads.
#[test]
fn extents_match_their_structures() {
    for name in projects() {
        let (_, project) = project(&name);
        let extents = project.structure_extents();
        for structure in &extents {
            let held = bytes(project, structure);
            let fixed = match structure.kind {
                StructureKind::ObjectDescriptor => Some(PublicObjectDescriptor::SIZE),
                StructureKind::ObjectInfo => Some(ObjectInfo::SIZE),
                StructureKind::OptionalObjectInfo => Some(OptionalObjectInfo::SIZE),
                StructureKind::PrivateObjectDescriptor => Some(PrivateObjectDescriptor::SIZE),
                StructureKind::Guid => Some(16),
                _ => None,
            };
            if let Some(size) = fixed {
                assert_eq!(held.len(), size, "{name}: {structure:?}");
            }
            match structure.kind {
                StructureKind::Name => {
                    let (last, text) = held.split_last().unwrap();
                    assert_eq!(*last, 0, "{name}: {structure:?}");
                    assert!(!text.contains(&0), "{name}: {structure:?}");
                }
                StructureKind::StringConstant => {
                    let prefix = u32::from_le_bytes(held[..4].try_into().unwrap());
                    assert_eq!(prefix as usize, held.len() - 6, "{name}: {structure:?}");
                    assert_eq!(&held[held.len() - 2..], [0, 0], "{name}: {structure:?}");
                }
                StructureKind::MemberName => {
                    assert_eq!(&held[held.len() - 2..], [0, 0], "{name}: {structure:?}");
                }
                StructureKind::ResumeTable => {
                    assert_eq!(&held[held.len() - 4..], [0; 4], "{name}: {structure:?}");
                }
                _ => {}
            }
        }
        let pcode: BTreeSet<(u32, u32)> = extents
            .iter()
            .filter(|s| s.kind == StructureKind::Pcode)
            .map(|s| (s.extent.start, s.extent.end))
            .collect();
        for object in project.objects().unwrap().map(Result::unwrap) {
            for method in object.pcode_methods().unwrap().map(Result::unwrap) {
                let extent = (method.pcode_va(), method.proc_dsc_va());
                assert!(pcode.contains(&extent), "{name}: P-Code {extent:x?}");
                assert_eq!(
                    (extent.1 - extent.0) as usize,
                    method.pcode_bytes().len(),
                    "{name}"
                );
            }
        }
    }
}

/// The kinds a form project and an ActiveX project hold appear: the project
/// tables, the object structures, procedures, controls, forms and, for the
/// OCX, its components and COM registration.
#[test]
fn every_kind_is_found() {
    let expect = |name: &str, kinds: &[StructureKind]| {
        let (_, project) = project(name);
        let found: BTreeSet<StructureKind> =
            project.structure_extents().iter().map(|s| s.kind).collect();
        for kind in kinds {
            assert!(found.contains(kind), "{name}: no {kind:?}");
        }
    };
    use StructureKind::*;
    expect(
        "forms",
        &[
            VbHeader,
            ProjectData,
            ObjectTable,
            ProjectInfo2,
            EventSource,
            ObjectDescriptor,
            ObjectInfo,
            OptionalObjectInfo,
            PrivateObjectDescriptor,
            MethodTable,
            MethodLinkTable,
            PointerTable,
            Name,
            Guid,
            FunctionType,
            ConstantPool,
            StringConstant,
            ProcedureDescriptor,
            Pcode,
            ControlInfo,
            EventSinkVtable,
            GuiTableEntry,
            FormData,
            TypeLibReference,
        ],
    );
    expect("ocx", &[Component, ComRegistration]);
    expect("events", &[DispidMap]);
    expect(
        "types",
        &[ExternalTable, DeclareDescriptor, OptionalDefaults],
    );
    expect("members", &[Member, InterfaceReference]);
    expect("late", &[MemberName]);
    expect("coverage", &[RecordLayout]);
    expect("typerefs", &[RecordReference, RecordLayout]);
    expect(
        "errors-native",
        &[
            UnwindRecord,
            ErrorHandlerTable,
            ResumeTable,
            LineNumberTable,
        ],
    );
}
