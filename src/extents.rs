//! Where the structures this crate reads lie in the image.
//!
//! A VB6 executable keeps its project metadata in the section that holds
//! its code: the VB header, the project and object tables, each object's
//! descriptors, method tables, constant pools and control tables, and every
//! procedure's descriptor with the P-Code before it sit between the native
//! stubs that enter the runtime. A disassembler that does not know where
//! they are decodes them as instructions. [`VbProject::structure_extents`]
//! states each one's extent, measured by the walk that reads it.

use std::{collections::BTreeSet, mem, ops::Range};

use crate::{
    addressmap::AddressMap,
    pcode::decoder::{InstructionIterator, PoolEntryRole},
    project::{MethodEntry, MethodLinkKind, VbObject, VbProject},
    util::read_u32_le,
    vb::{
        constantpool::{ConstantPool, PoolEntry},
        control::ControlInfo,
        controlprop::{ControlPropertyEntry, RecordLayout},
        events::{EventHandlerThunk, EventSinkVtable},
        external::{ExternalComponentEntry, ExternalDeclareInfo},
        functype::{ArgType, FuncTypDesc},
        header::VbHeader,
        member::MemberDesc,
        object::{ObjectInfo, OptionalObjectInfo, PublicObjectDescriptor},
        objecttable::ObjectTable,
        pooldesc::{ArrayDescriptor, CreationDescriptor, IoItems, RecordIoDescriptor},
        privateobj::PrivateObjectDescriptor,
        procedure::ProcDscInfo,
        projectdata::ProjectData,
        projectinfo2::{ProjectInfo2, ProjectInfo2Item, ProjectInfo2Iter},
        publicbytes::ClassFormPublicBytes,
        typeref::{InterfaceRef, RecordRef, TypeLibRef},
    },
};

/// What a [`StructureExtent`] holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum StructureKind {
    /// The VB header ([`VbHeader`]).
    VbHeader,
    /// The project data ([`ProjectData`]).
    ProjectData,
    /// The object table ([`ObjectTable`]).
    ObjectTable,
    /// The [`ProjectInfo2`] header.
    ProjectInfo2,
    /// An event-source record of the ProjectInfo2 region.
    EventSource,
    /// A public object descriptor ([`PublicObjectDescriptor`]).
    ObjectDescriptor,
    /// An [`ObjectInfo`].
    ObjectInfo,
    /// An [`OptionalObjectInfo`].
    OptionalObjectInfo,
    /// A [`PrivateObjectDescriptor`].
    PrivateObjectDescriptor,
    /// An object's method table: a slot per method, a standard module's
    /// `Declare` slots, which the compiler leaves unwritten, first.
    MethodTable,
    /// An object's method link table, the code addresses of its vtable.
    MethodLinkTable,
    /// Another array of VAs: method names, prototypes, members, events,
    /// variable stubs, parameter names, GUIDs or private object descriptors.
    PointerTable,
    /// A null-terminated ANSI string.
    Name,
    /// A 16-byte GUID.
    Guid,
    /// A procedure's or event's prototype ([`FuncTypDesc`]).
    FunctionType,
    /// A prototype's optional-parameter defaults: their header and values.
    OptionalDefaults,
    /// A public variable's or `Implements`' descriptor ([`MemberDesc`]).
    Member,
    /// An object's module-level or `Static` variable table
    /// ([`ClassFormPublicBytes`]).
    VariableTable,
    /// An object's constant pool: its entries' VAs.
    ConstantPool,
    /// A string constant: a BSTR's length prefix, characters and terminator.
    StringConstant,
    /// The descriptor `Get #` and `Put #` of a record read
    /// ([`RecordIoDescriptor`]), with its nested ones.
    RecordIoDescriptor,
    /// The item list of a `Print`, `Write` or `Input` ([`IoItems`]), with
    /// the byte after it the compiler's length counts.
    IoItems,
    /// The `SAFEARRAY` header of a fixed array in a record
    /// ([`ArrayDescriptor`]).
    ArrayDescriptor,
    /// The u32 byte length the compiler writes before a blob: a `Declare`'s
    /// library and function names, a record I/O descriptor, an item list.
    LengthPrefix,
    /// What `New` creates when it is not a project class
    /// ([`CreationDescriptor`]).
    CreationDescriptor,
    /// A class's licence key: its byte length and UTF-16 characters.
    LicenseKey,
    /// A member name a late-bound call passes: UTF-16, NUL-terminated.
    MemberName,
    /// A record's layout
    /// ([`RecordLayout`]): the members
    /// to initialize and release.
    RecordLayout,
    /// A procedure descriptor ([`ProcDscInfo`]) with its cleanup and
    /// line-number tables.
    ProcedureDescriptor,
    /// A procedure's P-Code, the bytecode the runtime interprets.
    Pcode,
    /// A native procedure's unwind record
    /// ([`ProcUnwindInfo`](crate::vb::native::ProcUnwindInfo)).
    UnwindRecord,
    /// A native procedure's `On Error GoTo` labels
    /// ([`ErrorHandlerTable`](crate::vb::native::ErrorHandlerTable)).
    ErrorHandlerTable,
    /// A native procedure's statement addresses, for `Resume`
    /// ([`ResumeTable`](crate::vb::native::ResumeTable)).
    ResumeTable,
    /// A native procedure's statement line numbers, for `Erl`
    /// ([`LineNumberTable`](crate::vb::native::LineNumberTable)).
    LineNumberTable,
    /// A control table entry ([`ControlInfo`]).
    ControlInfo,
    /// A control's event sink vtable ([`EventSinkVtable`]).
    EventSinkVtable,
    /// A control table entry's DISPID map: 8-byte pairs.
    DispidMap,
    /// A GUI table entry.
    GuiTableEntry,
    /// A form's or UserControl's form data.
    FormData,
    /// An entry of the project's external table.
    ExternalTable,
    /// A `Declare`d function's descriptor, which `DllFunctionCall` reads,
    /// with the 8 unread bytes before its call stub.
    DeclareDescriptor,
    /// A type library reference: an external table entry's, or a
    /// [`TypeLibRef`].
    TypeLibReference,
    /// An interface reference ([`InterfaceRef`]).
    InterfaceReference,
    /// A record reference ([`RecordRef`]).
    RecordReference,
    /// An external table entry's global object reference: a CLSID's VA and
    /// the data slot the runtime places the object in.
    GlobalObjectReference,
    /// A marker that bounds the project's native code: 16 bytes at its
    /// start, 4 at its end. Never executed.
    CodeMarker,
    /// An ActiveX component entry
    /// ([`ExternalComponentEntry`]).
    Component,
    /// The COM registration data
    /// ([`ComRegData`](crate::vb::comreg::ComRegData)): its header, records,
    /// strings and GUID arrays.
    ComRegistration,
}

impl StructureKind {
    /// Returns a short human-readable name, for provenance labels.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::VbHeader => "VB header",
            Self::ProjectData => "project data",
            Self::ObjectTable => "object table",
            Self::ProjectInfo2 => "ProjectInfo2",
            Self::EventSource => "event source record",
            Self::ObjectDescriptor => "object descriptor",
            Self::ObjectInfo => "ObjectInfo",
            Self::OptionalObjectInfo => "OptionalObjectInfo",
            Self::PrivateObjectDescriptor => "private object descriptor",
            Self::MethodTable => "method table",
            Self::MethodLinkTable => "method link table",
            Self::PointerTable => "pointer table",
            Self::Name => "name",
            Self::Guid => "GUID",
            Self::FunctionType => "prototype",
            Self::OptionalDefaults => "optional defaults",
            Self::Member => "member descriptor",
            Self::VariableTable => "variable table",
            Self::ConstantPool => "constant pool",
            Self::StringConstant => "string constant",
            Self::RecordIoDescriptor => "record I/O descriptor",
            Self::IoItems => "Print item list",
            Self::ArrayDescriptor => "array descriptor",
            Self::LengthPrefix => "length prefix",
            Self::CreationDescriptor => "creation descriptor",
            Self::LicenseKey => "licence key",
            Self::MemberName => "member name",
            Self::RecordLayout => "record layout",
            Self::ProcedureDescriptor => "procedure descriptor",
            Self::Pcode => "P-Code",
            Self::UnwindRecord => "unwind record",
            Self::ErrorHandlerTable => "error handler table",
            Self::ResumeTable => "resume table",
            Self::LineNumberTable => "line number table",
            Self::ControlInfo => "control info",
            Self::EventSinkVtable => "event sink vtable",
            Self::DispidMap => "DISPID map",
            Self::GuiTableEntry => "GUI table entry",
            Self::FormData => "form data",
            Self::ExternalTable => "external table entry",
            Self::DeclareDescriptor => "Declare descriptor",
            Self::TypeLibReference => "type library reference",
            Self::InterfaceReference => "interface reference",
            Self::RecordReference => "record reference",
            Self::GlobalObjectReference => "global object reference",
            Self::CodeMarker => "code marker",
            Self::Component => "component entry",
            Self::ComRegistration => "COM registration data",
        }
    }
}

/// The virtual addresses one structure occupies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructureExtent {
    /// Its half-open virtual-address range.
    pub extent: Range<u32>,
    /// What it is.
    pub kind: StructureKind,
}

impl<'a> VbProject<'a> {
    /// Returns the extent of every structure this crate reads in the image,
    /// ascending by address.
    ///
    /// Each is measured by the walk that decodes it, so a structure whose
    /// walk does not read is left out rather than guessed at, and so is one
    /// not wholly backed by the file (the zero-filled `.data` the runtime
    /// writes). Structures several owners share, such as a name or a type
    /// library reference, appear once. Native code is never an extent: the
    /// method link and event stubs, the `Declare` and import thunks and a
    /// native build's procedures stay out.
    pub fn structure_extents(&self) -> Vec<StructureExtent> {
        let mut walk = ExtentWalk {
            project: self,
            out: Vec::new(),
            procedures: BTreeSet::new(),
            object_infos: BTreeSet::new(),
            layouts: BTreeSet::new(),
        };
        walk.project_level();
        walk.objects();
        walk.procedures();
        walk.unwind_records();
        let mut out = walk.out;
        out.sort_by_key(|structure| (structure.extent.start, structure.extent.end, structure.kind));
        out.dedup();
        out
    }
}

/// The walk behind [`VbProject::structure_extents`].
struct ExtentWalk<'p> {
    /// The project walked.
    project: &'p VbProject<'p>,
    /// The extents found so far.
    out: Vec<StructureExtent>,
    /// Every `ProcDscInfo` VA a structure names, measured last.
    procedures: BTreeSet<u32>,
    /// The VA of every object's `ObjectInfo`: a `ProcDscInfo` points back at
    /// its owner's.
    object_infos: BTreeSet<u32>,
    /// Every record layout walked so far.
    layouts: BTreeSet<u32>,
}

impl<'p> ExtentWalk<'p> {
    /// The project's address map.
    fn map(&self) -> &'p AddressMap<'p> {
        self.project.address_map()
    }

    /// Each native procedure's unwind record and the tables it names.
    fn unwind_records(&mut self) {
        let map = self.map();
        for procedure in self.project.unwind_records() {
            let info = procedure.info;
            self.push(
                procedure.record_va,
                info.size(),
                StructureKind::UnwindRecord,
            );
            if let (Some(va), Some(table)) = (info.resume_va(), info.resume(map)) {
                self.push(va, table.size(), StructureKind::ResumeTable);
            }
            if let (Some(va), Some(table)) = (info.handlers_va(), info.handlers(map)) {
                self.push(va, table.size(), StructureKind::ErrorHandlerTable);
            }
            if let (Some(va), Some(table)) = (info.lines_va(), info.lines(map)) {
                self.push(va, table.size(), StructureKind::LineNumberTable);
            }
        }
    }

    /// Records `len` bytes at `va` as a `kind`, when the file backs every
    /// one of them contiguously. Returns whether it was recorded.
    fn push(&mut self, va: u32, len: usize, kind: StructureKind) -> bool {
        let map = self.map();
        let Some(last) = len
            .checked_sub(1)
            .and_then(|last| u32::try_from(last).ok())
            .and_then(|last| va.checked_add(last))
        else {
            return false;
        };
        let contiguous = match (map.va_to_offset(va), map.va_to_offset(last)) {
            (Ok(start), Ok(end)) => end.checked_sub(start) == len.checked_sub(1),
            _ => false,
        };
        if va == 0 || !contiguous {
            return false;
        }
        self.out.push(StructureExtent {
            extent: va..last.saturating_add(1),
            kind,
        });
        true
    }

    /// Records the u32 the compiler writes before a blob at `va` as its
    /// length, when it equals `len`.
    fn length_prefix(&mut self, va: u32, len: usize) {
        let Some(prefix) = va.checked_sub(4) else {
            return;
        };
        let held = self
            .map()
            .slice_from_va(prefix, 4)
            .ok()
            .and_then(|data| read_u32_le(data, 0).ok());
        if held.and_then(|held| usize::try_from(held).ok()) == Some(len) {
            self.push(prefix, 4, StructureKind::LengthPrefix);
        }
    }

    /// Records the null-terminated string at `va`.
    fn name(&mut self, va: u32) {
        if va == 0 || va == u32::MAX {
            return;
        }
        if let Ok(name) = self.project.read_string_at_va(va) {
            self.push(va, name.len().saturating_add(1), StructureKind::Name);
        }
    }

    /// Records the 16-byte GUID at `va`.
    fn guid(&mut self, va: u32) {
        self.push(va, 16, StructureKind::Guid);
    }

    /// Records the array of `count` VAs at `va` as a `kind` and returns its
    /// entries; empty when it is not wholly in the file.
    fn pointers(&mut self, va: u32, count: usize, kind: StructureKind) -> Vec<u32> {
        if !self.push(va, count.saturating_mul(4), kind) {
            return Vec::new();
        }
        let Ok(data) = self.map().slice_from_va(va, count.saturating_mul(4)) else {
            return Vec::new();
        };
        (0..count)
            .filter_map(|index| read_u32_le(data, index.checked_mul(4)?).ok())
            .collect()
    }

    /// The VB header and its strings, the project data, the object table,
    /// ProjectInfo2, the external, component and GUI tables, the COM
    /// registration data and `Sub Main`'s procedure.
    fn project_level(&mut self) {
        let project = self.project;
        let header_va = project.vb_header_va();
        let header = project.vb_header();
        self.push(header_va, VbHeader::SIZE, StructureKind::VbHeader);
        for offset in [
            header.project_description_offset(),
            header.project_exe_name_offset(),
            header.project_help_file_offset(),
            header.project_name_offset(),
        ]
        .into_iter()
        .flatten()
        .filter(|&offset| offset != 0)
        {
            self.name(header_va.wrapping_add(offset));
        }
        if let Ok(va) = header.project_data_va() {
            self.push(va, ProjectData::SIZE, StructureKind::ProjectData);
        }
        self.code_markers();
        let table = project.object_table();
        if let Ok(va) = project.project_data().object_table_va() {
            self.push(va, ObjectTable::SIZE, StructureKind::ObjectTable);
        }
        if let Ok(va) = table.project_name_va() {
            self.name(va);
        }
        self.project_info2();
        self.externals();
        self.components();
        for entry in project.gui_entries().into_iter().flatten() {
            if let Ok(size) = entry.entry_size() {
                self.push(entry.va(), size as usize, StructureKind::GuiTableEntry);
            }
            if let (Ok(va), Ok(size)) = (entry.form_data_va(), entry.form_data_size()) {
                self.push(va, size as usize, StructureKind::FormData);
            }
        }
        if let Some(registration) = project.com_registration()
            && let Ok(size) = registration.total_size(self.map())
        {
            self.push(registration.base_va(), size, StructureKind::ComRegistration);
        }
        // `lpSubMain` of a P-Code build is a `mov edx, <ProcDscInfo>` stub.
        if let Ok(sub_main) = header.sub_main_va()
            && let Ok([0xBA, a, b, c, d, ..]) = self.map().slice_from_va(sub_main, 5)
        {
            self.procedures.insert(u32::from_le_bytes([*a, *b, *c, *d]));
        }
    }

    /// The markers at the start and the end of the project's native code
    /// ([`ProjectData::code_start_va`], [`ProjectData::code_end_va`]), each
    /// taken only where its bytes are the marker.
    fn code_markers(&mut self) {
        /// The marker at the start: four `E9`, then twelve `CC`.
        const START: [u8; 16] = [
            0xE9, 0xE9, 0xE9, 0xE9, 0xCC, 0xCC, 0xCC, 0xCC, 0xCC, 0xCC, 0xCC, 0xCC, 0xCC, 0xCC,
            0xCC, 0xCC,
        ];
        /// The marker at the end.
        const END: [u8; 4] = [0x9E; 4];
        let data = self.project.project_data();
        let map = self.map();
        for (va, marker) in [
            (data.code_start_va(), &START[..]),
            (data.code_end_va(), &END[..]),
        ] {
            let Ok(va) = va else {
                continue;
            };
            if map
                .slice_from_va(va, marker.len())
                .is_ok_and(|bytes| bytes.starts_with(marker))
            {
                self.push(va, marker.len(), StructureKind::CodeMarker);
            }
        }
    }

    /// The ProjectInfo2 header, its private object descriptor array and the
    /// event-source records and names after it.
    fn project_info2(&mut self) {
        let map = self.map();
        let Ok(va) = self.project.object_table().project_info2_va() else {
            return;
        };
        let Some(header) = map
            .slice_from_va(va, ProjectInfo2::HEADER_SIZE)
            .ok()
            .and_then(|data| ProjectInfo2::parse(data).ok())
        else {
            return;
        };
        self.push(va, ProjectInfo2::HEADER_SIZE, StructureKind::ProjectInfo2);
        if let (Ok(descs), Ok(count)) = (
            header.object_descs_va(),
            self.project.object_table().total_objects(),
        ) {
            self.pointers(descs, usize::from(count), StructureKind::PointerTable);
        }
        let mut items = ProjectInfo2Iter::new(map, va);
        loop {
            let start = items.position();
            let Some(item) = items.next() else {
                break;
            };
            match item {
                ProjectInfo2Item::Record(record) => {
                    self.push(start, ProjectInfo2::ENTRY_SIZE, StructureKind::EventSource);
                    self.guid(record.guid_data_va);
                    self.name(record.guid_data_va.wrapping_add(16));
                    self.type_library(record.typelib_va);
                }
                ProjectInfo2Item::Name(_) => self.name(start),
            }
        }
    }

    /// The external table: its entries, the `Declare` descriptors and their
    /// strings, and the type library references.
    fn externals(&mut self) {
        let map = self.map();
        let data = self.project.project_data();
        let (Ok(table_va), Ok(count)) = (data.external_table_va(), data.external_count()) else {
            return;
        };
        if table_va == 0 {
            return;
        }
        for entry in self.project.externals().into_iter().flatten().flatten() {
            let Ok(object_va) = entry.external_object_va() else {
                continue;
            };
            if let Some(declare) = entry.as_declare(map) {
                self.push(
                    object_va,
                    ExternalDeclareInfo::BLOCK_SIZE,
                    StructureKind::DeclareDescriptor,
                );
                for va in [declare.library_name_va(), declare.function_name_va()]
                    .into_iter()
                    .flatten()
                {
                    self.name(va);
                    if let Ok(name) = self.project.read_string_at_va(va) {
                        self.length_prefix(va, name.len().saturating_add(1));
                    }
                }
            } else if let Some(typelib) = entry.as_typelib(map) {
                self.push(object_va, 8, StructureKind::GlobalObjectReference);
                if let Ok(guid) = typelib.typelib_guid_va() {
                    self.guid(guid);
                }
            }
        }
        self.push(
            table_va,
            (count as usize).saturating_mul(8),
            StructureKind::ExternalTable,
        );
    }

    /// The ActiveX component entries of the VB header's external table.
    fn components(&mut self) {
        let header = self.project.vb_header();
        let (Ok(mut va), Ok(count)) = (header.external_table_va(), header.external_count()) else {
            return;
        };
        if va == 0 {
            return;
        }
        for _ in 0..count {
            let Some(size) = self
                .map()
                .slice_from_va(va, ExternalComponentEntry::HEADER_SIZE)
                .ok()
                .and_then(|data| ExternalComponentEntry::parse(data).ok())
                .and_then(|entry| entry.entry_size().ok())
                .filter(|&size| size != 0)
            else {
                break;
            };
            self.push(va, size as usize, StructureKind::Component);
            va = va.wrapping_add(size);
        }
    }

    /// Every object's structures, by its descriptor.
    fn objects(&mut self) {
        let project = self.project;
        let Ok(array_va) = project.object_table().object_array_va() else {
            return;
        };
        let Ok(objects) = project.objects() else {
            return;
        };
        let objects: Vec<VbObject<'p, 'p>> = objects.flatten().collect();
        self.object_infos = objects
            .iter()
            .filter_map(|object| object.descriptor().object_info_va().ok())
            .collect();
        for (index, object) in objects.iter().enumerate() {
            let descriptor_va = u32::try_from(index)
                .ok()
                .and_then(|index| index.checked_mul(PublicObjectDescriptor::SIZE as u32))
                .and_then(|offset| array_va.checked_add(offset));
            if let Some(va) = descriptor_va {
                self.push(
                    va,
                    PublicObjectDescriptor::SIZE,
                    StructureKind::ObjectDescriptor,
                );
            }
            self.object(object);
        }
    }

    /// One object's structures.
    fn object(&mut self, object: &VbObject<'p, 'p>) {
        let descriptor = object.descriptor();
        if let Ok(va) = descriptor.object_name_va() {
            self.name(va);
        }
        if let (Ok(va), Ok(count)) = (descriptor.method_names_va(), object.method_count()) {
            for name in self.pointers(va, usize::from(count), StructureKind::PointerTable) {
                self.name(name);
            }
        }
        let Ok(info_va) = descriptor.object_info_va() else {
            return;
        };
        self.push(info_va, ObjectInfo::SIZE, StructureKind::ObjectInfo);
        for (variables, va) in [
            (object.public_bytes(), descriptor.public_bytes_va()),
            (object.static_bytes(), descriptor.static_bytes_va()),
        ] {
            if let (Some(variables), Ok(va)) = (variables, va)
                && let Ok(size) = variables.data_size()
            {
                let size = usize::from(size).max(ClassFormPublicBytes::MIN_SIZE);
                self.push(va, size, StructureKind::VariableTable);
                self.entries(variables.control_entries());
            }
        }
        self.method_table(object);
        self.constant_pool(object);
        if let Some(optional) = object.optional_info() {
            self.push(
                info_va.wrapping_add(ObjectInfo::SIZE as u32),
                OptionalObjectInfo::SIZE,
                StructureKind::OptionalObjectInfo,
            );
            self.optional(object, optional);
        }
        if let Some(private) = object.private_object()
            && let Ok(va) = object.info().private_object_va()
        {
            self.push(
                va,
                PrivateObjectDescriptor::SIZE,
                StructureKind::PrivateObjectDescriptor,
            );
            self.private(object, private);
        }
    }

    /// The object's method table, a standard module's `Declare` slots
    /// included, and the procedures it names.
    fn method_table(&mut self, object: &VbObject<'p, 'p>) {
        let Ok(methods) = object.methods() else {
            return;
        };
        let entries: Vec<_> = methods.collect();
        for entry in entries.iter().flatten() {
            if let MethodEntry::PCode(method) = entry {
                self.procedures.insert(method.proc_dsc_va());
            }
        }
        if let Ok(methods_va) = object.info().methods_va() {
            self.push(
                methods_va,
                entries.len().saturating_mul(4),
                StructureKind::MethodTable,
            );
        }
    }

    /// The object's constant pool and the procedures its thunks enter. The
    /// entries the P-Code names are measured with its procedures.
    fn constant_pool(&mut self, object: &VbObject<'p, 'p>) {
        let info = object.info();
        let (Ok(va), Ok(count)) = (info.constants_va(), info.constants_count()) else {
            return;
        };
        if count == 0 {
            return;
        }
        self.push(
            va,
            usize::from(count).saturating_mul(4),
            StructureKind::ConstantPool,
        );
        let Ok(pool) = object.constants_pool() else {
            return;
        };
        for (_, entry) in pool.entries(count) {
            if let Ok(PoolEntry::Procedure { proc_dsc_va }) = entry {
                self.procedures.insert(proc_dsc_va);
            }
        }
    }

    /// The structures an [`OptionalObjectInfo`] points at: the CLSID, the
    /// GUID tables, the method link table and the control table.
    fn optional(&mut self, object: &VbObject<'p, 'p>, optional: &OptionalObjectInfo<'p>) {
        if let Ok(va) = optional.object_clsid_va() {
            self.guid(va);
        }
        if let Some(va) = object.interface_iid_va() {
            self.guid(va);
        }
        for (table, count) in [
            (optional.gui_guid_table_va(), optional.gui_guids_count()),
            (
                optional.default_iid_table_va(),
                optional.default_iid_count(),
            ),
            (optional.events_iid_table_va(), optional.events_iid_count()),
        ] {
            let (Ok(table), Ok(count)) = (table, count) else {
                continue;
            };
            for guid in self.pointers(table, count as usize, StructureKind::PointerTable) {
                self.guid(guid);
            }
        }
        if let (Ok(va), Ok(count)) = (
            optional.method_link_table_va(),
            optional.method_link_count(),
        ) {
            self.pointers(va, usize::from(count), StructureKind::MethodLinkTable);
        }
        let pool = object.info().constants_va().ok();
        for link in object.method_links().into_iter().flatten().flatten() {
            match link.kind {
                MethodLinkKind::Procedure { proc_dsc_va } => {
                    self.procedures.insert(proc_dsc_va);
                }
                // A `New` variable's accessor names what to create by its
                // index in the object's constant pool.
                MethodLinkKind::Variable {
                    pushed: Some([index, pushed_pool]),
                    ..
                } if Some(pushed_pool) == pool => {
                    if let Ok(entry) = object
                        .constants_pool()
                        .and_then(|pool| pool.va_at(u16::try_from(index).unwrap_or(u16::MAX)))
                    {
                        self.creation(entry);
                    }
                }
                _ => {}
            }
        }
        let (Ok(controls_va), Ok(count)) = (optional.controls_va(), optional.control_count())
        else {
            return;
        };
        for index in 0..count {
            let Some(va) = index
                .checked_mul(ControlInfo::MIN_SIZE as u32)
                .and_then(|offset| controls_va.checked_add(offset))
            else {
                break;
            };
            self.control(va);
        }
    }

    /// One control table entry, its name, GUID and event sink vtable, and
    /// the procedures its event stubs enter.
    fn control(&mut self, va: u32) {
        let map = self.map();
        let Some(info) = map
            .slice_from_va(va, ControlInfo::MIN_SIZE)
            .ok()
            .and_then(|data| ControlInfo::parse(data).ok())
        else {
            return;
        };
        self.push(va, ControlInfo::MIN_SIZE, StructureKind::ControlInfo);
        if let Ok(name) = info.name_va() {
            self.name(name);
        }
        if let Ok(guid) = info.guid_va() {
            self.guid(guid);
        }
        if let (Ok(count), Ok(table)) = (info.dispid_map_count(), info.dispid_map_va()) {
            self.push(
                table,
                usize::from(count).saturating_mul(8),
                StructureKind::DispidMap,
            );
        }
        let Ok(sink_va) = info.event_sink_vtable_va() else {
            return;
        };
        let Some(sink) = map
            .slice_from_va(sink_va, EventSinkVtable::HEADER_SIZE)
            .ok()
            .and_then(|data| EventSinkVtable::parse(data, &info).ok())
        else {
            return;
        };
        self.push(sink_va, sink.size(), StructureKind::EventSinkVtable);
        for (_, handler) in sink.connected_handlers() {
            if let Some(thunk) = map
                .slice_from_va(handler, EventHandlerThunk::SIZE)
                .ok()
                .and_then(|data| EventHandlerThunk::parse_from_event_entry(data, handler))
            {
                self.procedures.insert(thunk.proc_dsc_info_va);
            }
        }
    }

    /// The structures a [`PrivateObjectDescriptor`] points at: the
    /// prototypes of the procedures and events, the members and the
    /// record layouts of the object's `Type`s.
    fn private(&mut self, object: &VbObject<'p, 'p>, private: &PrivateObjectDescriptor<'p>) {
        if let (Ok(va), Ok(count)) = (private.func_type_descs_va(), object.method_count()) {
            self.prototypes(va, usize::from(count));
        }
        if let (Ok(va), Ok(count)) = (private.event_descs_va(), private.event_count()) {
            self.prototypes(va, usize::from(count));
        }
        if let (Ok(va), Ok(count)) = (private.member_descs_va(), private.member_count()) {
            for member in self.pointers(va, usize::from(count), StructureKind::PointerTable) {
                self.member(member);
            }
        }
        if let (Ok(va), Ok(count)) = (private.record_layouts_va(), private.record_layout_count()) {
            for layout in self.pointers(va, usize::from(count), StructureKind::PointerTable) {
                self.record_layout(layout);
            }
        }
    }

    /// The record layout at `va`, measured by its own size field, and what
    /// its entries name. Each layout is walked once.
    fn record_layout(&mut self, va: u32) {
        if !self.layouts.insert(va) {
            return;
        }
        let Ok(layout) = RecordLayout::at(self.map(), va) else {
            return;
        };
        if let Ok(size) = layout.size() {
            self.push(va, usize::from(size), StructureKind::RecordLayout);
        }
        self.entries(layout.entries());
    }

    /// What variable, cleanup or record layout entries name: the layouts of
    /// records and of record arrays' elements, and the IID of a fixed
    /// array of objects.
    fn entries(&mut self, entries: impl Iterator<Item = ControlPropertyEntry<'p>>) {
        let entries: Vec<ControlPropertyEntry<'p>> = entries.collect();
        for entry in entries {
            for layout in [entry.record_layout_va(), entry.element_layout_va()]
                .into_iter()
                .flatten()
            {
                self.record_layout(layout);
            }
            if let Some(iid) = entry.safearray_iid_va() {
                self.guid(iid);
            }
        }
    }

    /// The array of `count` prototypes at `va`, each prototype with its
    /// parameter names, optional defaults and type references.
    fn prototypes(&mut self, va: u32, count: usize) {
        for desc_va in self.pointers(va, count, StructureKind::PointerTable) {
            let Some(desc) = self
                .map()
                .slice_from_va(desc_va, FuncTypDesc::MIN_SIZE)
                .ok()
                .and_then(|data| FuncTypDesc::parse(data).ok())
            else {
                continue;
            };
            if let Some(size) = desc.size() {
                self.push(desc_va, size, StructureKind::FunctionType);
            }
            if let Ok(names) = desc.param_names_va() {
                for name in self.pointers(
                    names,
                    usize::from(desc.entry_count()),
                    StructureKind::PointerTable,
                ) {
                    self.name(name);
                }
            }
            self.optional_defaults(&desc);
            for arg in desc.arg_types().into_iter().chain(desc.return_type()) {
                self.type_reference(arg);
            }
        }
    }

    /// A prototype's optional-parameter defaults: the 8-byte header (the
    /// values' size and VA) and the values.
    fn optional_defaults(&mut self, desc: &FuncTypDesc<'_>) {
        let Ok(header_va) = desc.optional_defaults_va() else {
            return;
        };
        if !self.push(header_va, 8, StructureKind::OptionalDefaults) {
            return;
        }
        let Ok(header) = self.map().slice_from_va(header_va, 8) else {
            return;
        };
        if let (Ok(size), Ok(values_va)) = (read_u32_le(header, 0), read_u32_le(header, 4)) {
            self.push(values_va, size as usize, StructureKind::OptionalDefaults);
        }
    }

    /// One member descriptor, its name and its type's reference.
    fn member(&mut self, va: u32) {
        let Some(member) = self
            .map()
            .slice_from_va(va, MemberDesc::MIN_SIZE)
            .ok()
            .and_then(|data| MemberDesc::parse(data).ok())
        else {
            return;
        };
        if let Ok(size) = member.size() {
            self.push(va, size, StructureKind::Member);
        }
        if let Ok(name) = member.name_va() {
            self.name(name);
        }
        if let Ok(arg) = member.var_type() {
            self.type_reference(arg);
        }
    }

    /// The reference an interface or record type carries, with its library.
    /// A class type's descriptor is the class's `ObjectInfo`, measured with
    /// its object.
    fn type_reference(&mut self, arg: ArgType) {
        let Some(va) = arg.descriptor_va() else {
            return;
        };
        let map = self.map();
        match arg.base_type() {
            ArgType::IUNKNOWN | ArgType::INTERFACE => {
                let Ok(reference) = InterfaceRef::at(map, va) else {
                    return;
                };
                self.push(va, InterfaceRef::SIZE, StructureKind::InterfaceReference);
                if let Ok(iid) = reference.iid_va() {
                    self.guid(iid);
                }
                if let Ok(typelib) = reference.typelib_va() {
                    self.type_library(typelib);
                }
            }
            ArgType::RECORD => {
                let Ok(reference) = RecordRef::at(map, va) else {
                    return;
                };
                self.push(va, RecordRef::SIZE, StructureKind::RecordReference);
                for guid in [reference.libid_va(), reference.guid_va()]
                    .into_iter()
                    .flatten()
                {
                    self.guid(guid);
                }
            }
            _ => {}
        }
    }

    /// A [`TypeLibRef`], its GUID, path and name.
    fn type_library(&mut self, va: u32) {
        if va == 0 {
            return;
        }
        let Ok(typelib) = TypeLibRef::at(self.map(), va) else {
            return;
        };
        self.push(va, TypeLibRef::SIZE, StructureKind::TypeLibReference);
        if let Ok(guid) = typelib.guid_va() {
            self.guid(guid);
        }
        for name in [typelib.path_va(), typelib.name_va()].into_iter().flatten() {
            self.name(name);
        }
    }

    /// Every procedure a structure named: its descriptor and the P-Code
    /// before it. A VA whose descriptor does not point back at one of the
    /// project's objects is not a procedure of the project.
    fn procedures(&mut self) {
        let map = self.map();
        let procedures = mem::take(&mut self.procedures);
        for va in procedures {
            let Some(desc) = map
                .slice_from_va(va, ProcDscInfo::MIN_SIZE)
                .ok()
                .and_then(|data| ProcDscInfo::parse(data).ok())
            else {
                continue;
            };
            let owned = desc
                .object_info_va()
                .is_ok_and(|owner| self.object_infos.contains(&owner));
            let (Ok(pcode), Ok(size)) = (desc.proc_size(), desc.extent_size()) else {
                continue;
            };
            if !owned || pcode == 0 {
                continue;
            }
            let pcode_va = va.wrapping_sub(u32::from(pcode));
            self.push(pcode_va, usize::from(pcode), StructureKind::Pcode);
            self.push(va, size, StructureKind::ProcedureDescriptor);
            self.entries(desc.cleanup_entries());
            if let Some(secondary) = desc.secondary_table() {
                self.entries(secondary.entries());
            }
            if let (Ok(owner), Ok(code)) = (
                desc.object_info_va(),
                map.slice_from_va(pcode_va, usize::from(pcode)),
            ) {
                self.pool_entries(owner, code, pcode);
            }
        }
    }

    /// What a `New` names when it is not a project class (whose
    /// `ObjectInfo` is measured with its object): the creation descriptor,
    /// the CLSID, the IID and the licence key.
    fn creation(&mut self, va: u32) {
        let Some(descriptor) = CreationDescriptor::at(self.map(), va) else {
            return;
        };
        self.push(
            va,
            CreationDescriptor::SIZE,
            StructureKind::CreationDescriptor,
        );
        for guid in [descriptor.clsid_va(), descriptor.iid_va()]
            .into_iter()
            .flatten()
        {
            self.guid(guid);
        }
        if let Ok(key) = descriptor.license_va()
            && key != 0
            && let Some(len) = self
                .map()
                .slice_from_va(key, 4)
                .ok()
                .and_then(|data| read_u32_le(data, 0).ok())
        {
            self.push(
                key,
                (len as usize).saturating_add(4),
                StructureKind::LicenseKey,
            );
        }
    }

    /// The constant pool entries a procedure's P-Code names, each measured
    /// as what its opcode says it holds. Entries no opcode types (an
    /// address, a `Print` item descriptor) are left out.
    fn pool_entries(&mut self, owner: u32, code: &'p [u8], size: u16) {
        let map = self.map();
        let Some(pool_va) = map
            .slice_from_va(owner, ObjectInfo::SIZE)
            .ok()
            .and_then(|data| ObjectInfo::parse(data).ok())
            .and_then(|info| info.constants_va().ok())
        else {
            return;
        };
        let pool = ConstantPool::new(map, pool_va);
        let references: BTreeSet<(u16, PoolEntryRole)> = InstructionIterator::new(code, size)
            .flatten()
            .flat_map(|instruction| instruction.pool_references(code))
            .map(|reference| (reference.index, reference.role))
            .collect();
        for (index, role) in references {
            let Ok(va) = pool.va_at(index) else {
                continue;
            };
            match role {
                PoolEntryRole::String => {
                    if let Ok(Some(string)) = pool.string_at(index) {
                        self.push(
                            string.length_prefix_va(),
                            string.total_binary_size(),
                            StructureKind::StringConstant,
                        );
                    }
                }
                PoolEntryRole::MemberName => {
                    let units = map.slice_from_va(va, 2).ok().and_then(|data| {
                        data.as_chunks::<2>()
                            .0
                            .iter()
                            .take(ConstantPool::MAX_NAME_CHARS)
                            .position(|&unit| unit == [0, 0])
                    });
                    if let Some(units) = units {
                        self.push(
                            va,
                            units.saturating_add(1).saturating_mul(2),
                            StructureKind::MemberName,
                        );
                    }
                }
                PoolEntryRole::Guid => self.guid(va),
                PoolEntryRole::RecordLayout => self.record_layout(va),
                PoolEntryRole::RecordIo => {
                    if let Some(size) = RecordIoDescriptor::at(map, va)
                        .ok()
                        .and_then(|descriptor| descriptor.extent_size())
                    {
                        self.push(va, size, StructureKind::RecordIoDescriptor);
                        self.length_prefix(va, size);
                    }
                }
                // The compiler's length counts a byte after the items, which
                // the runtime does not read.
                PoolEntryRole::IoItems => {
                    if let Ok(items) = IoItems::at(map, va) {
                        let size = items.size().saturating_add(1);
                        self.push(va, size, StructureKind::IoItems);
                        self.length_prefix(va, size);
                    }
                }
                PoolEntryRole::ArrayDescriptor => {
                    if let Ok(array) = ArrayDescriptor::at(map, va) {
                        self.push(va, array.size(), StructureKind::ArrayDescriptor);
                    }
                }
                PoolEntryRole::Creation => self.creation(va),
                PoolEntryRole::Address => {}
            }
        }
    }
}
