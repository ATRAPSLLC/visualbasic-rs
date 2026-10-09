//! VB6 object (form, module, class) representation.
//!
//! In VB6, every source file compiles into an "object" registered in the
//! ObjectTable. Forms (`.frm`), standard modules (`.bas`), and class modules
//! (`.cls`) are all objects. Each object has a [`PublicObjectDescriptor`]
//! (the array entry), an [`ObjectInfo`] with method/constant table pointers,
//! an optional [`OptionalObjectInfo`] (controls, method links, vtable
//! layout), and an optional [`PrivateObjectDescriptor`] (function type
//! descriptors, parameter name tables).
//!
//! [`VbObject`] ties these structures together and provides iterators over
//! methods, controls, and method link thunks.

use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    str,
};

use crate::{
    addressmap::AddressMap,
    error::Error,
    project::{
        ControlEntryIterator, MethodEntry, MethodLinkIterator, MethodLinkKind, PCodeMethod,
        VbProject,
    },
    util::{read_u16_le, read_u32_le},
    vb::{
        constantpool::ConstantPool,
        control::Guid,
        designer::Designer,
        eventname,
        events::EventHandlerThunk,
        flags::ObjectTypeFlags,
        formdata::{FormControlType, FormDataParser},
        functype::FuncTypDesc,
        guitable::GuiTableEntry,
        object::{GuidTableIter, ObjectInfo, OptionalObjectInfo, PublicObjectDescriptor},
        privateobj::PrivateObjectDescriptor,
        publicbytes::ClassFormPublicBytes,
        varstub::VarStubIter,
    },
};

/// Result of looking up a method name from the method names table.
///
/// Distinguishes between "the object has no names table at all" (every
/// standard module) and "this particular method has no name" (a procedure
/// that is not a public member).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MethodNameResult<'a> {
    /// The object has no method names table (`method_names_va == 0`), as in
    /// all 29 standard modules of the fixtures.
    NoTable,
    /// This method has no name: its entry is 0. Procedures that are not
    /// public members (`Private`, `Friend`, `Class_Initialize`/`Terminate`,
    /// control event handlers) have none; a `Private` procedure that
    /// implements an interface member or handles a `WithEvents` event has
    /// one (`events`: `Ring.Measure_Area`, `Listener.m_Source_Started`).
    Unnamed,
    /// The resolved name bytes (null-terminated ASCII in the PE image).
    Name(&'a [u8]),
}

impl<'a> MethodNameResult<'a> {
    /// Returns the name bytes if available, or `None` for `NoTable`/`Unnamed`.
    pub fn as_bytes(&self) -> Option<&'a [u8]> {
        match self {
            Self::Name(n) => Some(n),
            _ => None,
        }
    }

    /// Returns the name as a lossy UTF-8 string if available, or `None`
    /// for `NoTable`/`Unnamed`.
    ///
    /// Borrows when the underlying bytes are already valid UTF-8 (the
    /// common case for ASCII identifier names).
    pub fn as_str(&self) -> Option<Cow<'a, str>> {
        self.as_bytes().map(String::from_utf8_lossy)
    }
}

/// Formats a VB6 function signature from a [`FuncTypDesc`] and method name.
///
/// Produces output like `Sub Reset(start As Long)`,
/// `Function GetValue(x As Long) As String`, or
/// `Property Get Name() As String`. `ByVal` is not written (only `ByRef`),
/// an `Optional` parameter's default is not shown, and a class-typed
/// parameter or return reads `As Class` (`types`: `Function K(x As Class) As Class`
/// for `K(ByVal x As Kinds) As Kinds`).
///
/// # Arguments
///
/// * `ftd` - The function type descriptor containing kind, args, return type.
/// * `name` - The method name string.
/// * `map` - Address map for resolving parameter name VAs.
pub fn format_signature(ftd: &FuncTypDesc<'_>, name: &str, map: &AddressMap<'_>) -> String {
    let kind = ftd.kind_keyword();
    let ret = ftd
        .return_type()
        .map(|t| format!(" As {}", t.value_type()))
        .unwrap_or_default();
    let param_names = ftd.param_names(map);
    let arg_types = ftd.arg_types();
    let args = if param_names.is_empty() && ftd.arg_count() == 0 {
        "()".into()
    } else if param_names.is_empty() && arg_types.is_empty() {
        format!("({} args)", ftd.arg_count())
    } else {
        let count = ftd.arg_count() as usize;
        let params: Vec<String> = (0..count)
            .map(|i| {
                let pname = param_names
                    .get(i)
                    .filter(|n| !n.is_empty())
                    .map(|n| String::from_utf8_lossy(n).into_owned());
                let name = pname.unwrap_or_else(|| format!("arg{i}"));
                match arg_types.get(i) {
                    Some(t) if ftd.has_param_array() && i.saturating_add(1) == count => {
                        format!("ParamArray {name}() As {}", t.type_name())
                    }
                    Some(t) => {
                        let optional = if t.is_optional() { "Optional " } else { "" };
                        let byref = if t.is_byref() { "ByRef " } else { "" };
                        let array = if t.is_array() { "()" } else { "" };
                        format!("{optional}{byref}{name}{array} As {}", t.type_name())
                    }
                    None => name,
                }
            })
            .collect();
        format!("({})", params.join(", "))
    };
    format!("{kind} {name}{args}{ret}")
}

/// How a class module can be created and seen outside its project: its
/// `Instancing` property.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Instancing {
    /// Visible only inside the project.
    Private,
    /// Public, but created only by the project.
    PublicNotCreatable,
    /// Creatable; each object in its own server instance.
    SingleUse,
    /// `SingleUse`, with its members in the global namespace.
    GlobalSingleUse,
    /// Creatable; one server instance for all objects.
    MultiUse,
    /// `MultiUse`, with its members in the global namespace.
    GlobalMultiUse,
}

/// A single VB6 object (form, module, class) within the project.
///
/// Provides access to the object's descriptor, info, optional info,
/// and private object descriptor (which contains function type
/// descriptors and parameter name tables).
///
/// Holds a reference to the parent [`VbProject`] so all accessor methods
/// can resolve VAs without requiring the project as a parameter.
#[derive(Debug)]
pub struct VbObject<'a, 'p> {
    /// Reference to the parent project for VA resolution.
    project: &'p VbProject<'a>,
    /// Public object descriptor (0x30-byte entry in the ObjectTable array).
    descriptor: PublicObjectDescriptor<'a>,
    /// Core object info with method table and constants VAs.
    info: ObjectInfo<'a>,
    /// Extended info (controls, method links, vtable layout); `None` for a
    /// standard module.
    optional_info: Option<OptionalObjectInfo<'a>>,
    /// Private descriptor with function types and param names; `None` for
    /// standard modules or when the VA is null/`0xFFFFFFFF`.
    private_object: Option<PrivateObjectDescriptor<'a>>,
}

impl<'a, 'p: 'a> VbObject<'a, 'p> {
    /// Returns the name of event sink slot `slot` of a hosted ActiveX
    /// control whose ControlInfo names events IID `guid`: the
    /// `VBControlExtenderEvents` member for slots 0-8 when `guid` is the
    /// instance or control-array events IID of one of the project's
    /// [`components`](VbProject::components). The control's own events, from
    /// slot 9 on, are named only in its type library.
    fn extender_event_name(&self, guid: Option<&Guid>, slot: u16) -> Option<&'static str> {
        let guid = guid?;
        let hosted = self.project.components().ok()?.any(|component| {
            component.instance_events_iid().as_ref() == Some(guid)
                || component.array_events_iid().as_ref() == Some(guid)
        });
        hosted
            .then(|| eventname::event_name_for_class("Extender", slot))
            .flatten()
    }

    /// Parses a VbObject by index from the object array.
    ///
    /// Resolves the `PublicObjectDescriptor` at position `index`, then
    /// follows pointers to `ObjectInfo`, `OptionalObjectInfo`, and
    /// `PrivateObjectDescriptor`.
    ///
    /// # Arguments
    ///
    /// * `project` - The parent VB6 project.
    /// * `index` - Zero-based index into the object array.
    ///
    /// # Errors
    ///
    /// Returns an error if `index >= total_objects` or if any VA in
    /// the descriptor chain cannot be resolved.
    pub fn parse(project: &'p VbProject<'a>, index: u16) -> Result<Self, Error> {
        let map = project.address_map();
        let ot = project.object_table();
        if index >= ot.total_objects()? {
            return Err(Error::ObjectIndexOutOfRange {
                index,
                total: ot.total_objects()?,
            });
        }

        // Each PublicObjectDescriptor is 0x30 bytes, starting at object_array_va
        let array_offset = u32::from(index).saturating_mul(PublicObjectDescriptor::SIZE as u32);
        let desc_data = map.slice_from_va(
            ot.object_array_va()?.wrapping_add(array_offset),
            PublicObjectDescriptor::SIZE,
        )?;
        let descriptor = PublicObjectDescriptor::parse(desc_data)?;

        // Follow descriptor -> ObjectInfo
        let info_data = map.slice_from_va(descriptor.object_info_va()?, ObjectInfo::SIZE)?;
        let info = ObjectInfo::parse(info_data)?;

        // OptionalObjectInfo (0x40 bytes) sits between ObjectInfo and the
        // constants table when there is room. For standard modules, the
        // constants table starts immediately after ObjectInfo (gap == 0)
        // and no OptionalObjectInfo exists - regardless of the flag bit.
        let optional_info = if descriptor.has_optional_info() {
            let opt_va = descriptor
                .object_info_va()?
                .wrapping_add(ObjectInfo::SIZE as u32);
            let constants_va = info.constants_va()?;
            // Only parse if there is a full 0x40-byte gap before the constants
            let has_room = constants_va == 0
                || constants_va >= opt_va.wrapping_add(OptionalObjectInfo::SIZE as u32);
            if has_room {
                map.slice_from_va(opt_va, OptionalObjectInfo::SIZE)
                    .ok()
                    .and_then(|d| OptionalObjectInfo::parse(d).ok())
            } else {
                None
            }
        } else {
            None
        };

        // PrivateObjectDescriptor at ObjectInfo.private_object_va
        let priv_va = info.private_object_va()?;
        let private_object = if priv_va != 0 && priv_va != 0xFFFFFFFF {
            map.slice_from_va(priv_va, PrivateObjectDescriptor::SIZE)
                .ok()
                .and_then(|d| PrivateObjectDescriptor::parse(d).ok())
        } else {
            None
        };

        Ok(Self {
            project,
            descriptor,
            info,
            optional_info,
            private_object,
        })
    }

    /// Returns a reference to the parent [`VbProject`].
    #[inline]
    pub fn project(&self) -> &'p VbProject<'a> {
        self.project
    }

    /// Returns the [`PublicObjectDescriptor`] for this object.
    #[inline]
    pub fn descriptor(&self) -> &PublicObjectDescriptor<'a> {
        &self.descriptor
    }

    /// Returns the [`ObjectInfo`] for this object.
    #[inline]
    pub fn info(&self) -> &ObjectInfo<'a> {
        &self.info
    }

    /// Returns the [`OptionalObjectInfo`] if present.
    #[inline]
    pub fn optional_info(&self) -> Option<&OptionalObjectInfo<'a>> {
        self.optional_info.as_ref()
    }

    /// Returns the [`PrivateObjectDescriptor`] if present.
    ///
    /// Contains function type descriptors and parameter name tables. Not
    /// available for standard modules (BAS files) - those have
    /// `private_object_va == 0xFFFFFFFF`.
    #[inline]
    pub fn private_object(&self) -> Option<&PrivateObjectDescriptor<'a>> {
        self.private_object.as_ref()
    }

    /// Number of methods with a prototype (a [`FuncTypDesc`]).
    ///
    /// Counts the non-null entries of the FuncTypDesc array. Public members
    /// have one, and so does a `Private` procedure that implements an
    /// interface member or handles a `WithEvents` event (`events`: `Ring`
    /// counts 11, its two `Radius` properties and nine `Measure_*`);
    /// `Friend` procedures, other `Private` procedures,
    /// `Class_Initialize`/`Terminate` and control event handlers have none.
    /// Returns 0 if no private object descriptor is available.
    ///
    /// # Errors
    ///
    /// Returns an error if the private object descriptor's fields or the
    /// method count cannot be read.
    pub fn public_func_count(&self) -> Result<u32, Error> {
        Ok(u32::try_from(self.func_type_descs()?.count()).unwrap_or(u32::MAX))
    }

    /// The private object descriptor's
    /// [`var_stub_count`](PrivateObjectDescriptor::var_stub_count), or 0 if
    /// no private object descriptor is available.
    ///
    /// It is 0 in every fixture, including `data`'s `Item`, which declares
    /// `Public Name As String`, so it does not count public variables; what
    /// it counts is unconfirmed.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying PrivateObjectDescriptor field
    /// cannot be read.
    #[inline]
    pub fn var_stub_count(&self) -> Result<u32, Error> {
        match self.private_object.as_ref() {
            Some(p) => Ok(u32::from(p.var_stub_count()?)),
            None => Ok(0),
        }
    }

    /// Reads the object name as a lossy UTF-8 string.
    ///
    /// Borrows when the underlying bytes are already valid UTF-8 (the
    /// common case for ASCII identifier names) and allocates only when
    /// invalid sequences need U+FFFD substitution. Use
    /// [`name_bytes`](Self::name_bytes) when you need the raw bytes
    /// (e.g., for byte-exact comparison or hex display of malformed
    /// names).
    ///
    /// # Errors
    ///
    /// Returns an error if the name VA cannot be resolved.
    pub fn name(&self) -> Result<Cow<'a, str>, Error> {
        Ok(String::from_utf8_lossy(self.name_bytes()?))
    }

    /// Reads the object name as raw bytes from the PE image.
    ///
    /// Returns the slice exactly as stored - no decoding, no fallback.
    /// Prefer [`name`](Self::name) for display.
    ///
    /// # Errors
    ///
    /// Returns an error if the name VA cannot be resolved.
    pub fn name_bytes(&self) -> Result<&'a [u8], Error> {
        self.project
            .read_string_at_va(self.descriptor.object_name_va()?)
    }

    /// Classifies the object kind: its [`designer`](Self::designer)'s name
    /// (`"Form"`, `"UserControl"`, `"MDIForm"`, `"UserDocument"`,
    /// `"PropertyPage"`), else what the type flags say
    /// ([`ObjectTypeFlags::kind_name`](crate::vb::flags::ObjectTypeFlags::kind_name):
    /// `"Class"` or `"Module"`). The flags alone cannot tell a UserControl
    /// (`0x001DA003` in `tests/fixtures/forms` and `dispid`) from a class
    /// (`0x00118003`).
    ///
    /// # Errors
    ///
    /// Returns an error if the descriptor's `object_type_raw` field cannot
    /// be read.
    pub fn object_kind(&self) -> Result<&'static str, Error> {
        match self.designer() {
            Some(designer) => Ok(designer.name()),
            None => Ok(ObjectTypeFlags(self.descriptor.object_type_raw()?).kind_name()),
        }
    }

    /// Returns a class module's `Instancing`: [`Instancing::Private`] for a
    /// class not exposed outside its project, otherwise from its COM
    /// registration record (the one with its CLSID,
    /// [`ComRegObject::reg_flag`](crate::vb::comreg::ComRegObject::reg_flag))
    /// and its object type's global-namespace bit. `None` for a module, a
    /// designer object (form, UserControl, ...) or an exposed class with no
    /// record.
    pub fn instancing(&self) -> Option<Instancing> {
        let flags = self.object_type_flags().ok()?;
        if !flags.is_class() || self.designer().is_some() {
            return None;
        }
        if !flags.is_exposed() {
            return Some(Instancing::Private);
        }
        let map = self.project.address_map();
        let clsid = self.optional_info.as_ref()?.resolve_clsid(map)?;
        let registration = self.project.com_registration()?;
        let record = registration
            .objects(map)
            .ok()?
            .find(|record| record.clsid() == Some(clsid))?;
        let global = flags.is_global_namespace();
        Some(match (record.reg_flag().ok()?, global) {
            (0, false) => Instancing::PublicNotCreatable,
            (1, false) => Instancing::SingleUse,
            (1, true) => Instancing::GlobalSingleUse,
            (2, false) => Instancing::MultiUse,
            (2, true) => Instancing::GlobalMultiUse,
            _ => return None,
        })
    }

    /// Returns the designer the object was built with, for a form,
    /// UserControl or other designer object: the events interface its own
    /// ControlInfo (control index 0xFFFF) names. A class names
    /// `IClassModuleEvt` `{FCFB3D21-...}` there and a module has no
    /// ControlInfo, so both give `None`.
    pub fn designer(&self) -> Option<Designer> {
        self.controls().ok()?.find_map(|control| {
            let control = control.ok()?;
            if control.index().ok()? != 0xFFFF {
                return None;
            }
            Designer::from_events_iid(control.guid()?)
        })
    }

    /// Reads the name of the method at `index` from the method names table.
    ///
    /// Returns a [`MethodNameResult`] that distinguishes:
    /// - `NoTable`: the object has no method names table (`method_names_va == 0`)
    /// - `Unnamed`: this specific method has no name: its entry is 0, or
    ///   `0xFFFFFFFF` (`types`: `Kinds.Rec`, a `Friend`, and `Kinds.Hidden`,
    ///   a `Private`)
    /// - `Name(&[u8])`: the resolved name bytes
    ///
    /// # Errors
    ///
    /// Returns an error if the names table entry or the name cannot be read.
    pub fn method_name(&self, index: u16) -> Result<MethodNameResult<'a>, Error> {
        let names_va = self.descriptor.method_names_va()?;
        if names_va == 0 {
            return Ok(MethodNameResult::NoTable);
        }
        let entry_va = names_va.wrapping_add(u32::from(index).saturating_mul(4));
        let entry_data = self.project.address_map().slice_from_va(entry_va, 4)?;
        let name_va = read_u32_le(entry_data, 0)?;
        if name_va == 0 || name_va == u32::MAX {
            return Ok(MethodNameResult::Unnamed);
        }
        self.project
            .read_string_at_va(name_va)
            .map(MethodNameResult::Name)
    }

    /// Returns the number of methods in this object.
    ///
    /// Uses the larger of `ObjectInfo.method_count()` and
    /// `PublicObjectDescriptor.method_count()`. The two are equal in every
    /// P-Code fixture object. In the natively compiled `flow-native` module
    /// the `ObjectInfo` count is 0 and the descriptor's 25 (its
    /// procedures), and there is no method table, so
    /// [`methods`](Self::methods) yields nothing.
    ///
    /// A standard module counts each `Declare` too: method_count is
    /// `Declare`s plus procedures (`vtable`: 13 + 3 = 16), and the
    /// `Declare`s take the first slots (see [`MethodEntry`]).
    ///
    /// # Errors
    ///
    /// Returns an error if either underlying method count cannot be read.
    #[inline]
    pub fn method_count(&self) -> Result<u16, Error> {
        let info_count = self.info.method_count()?;
        let desc_count = self.descriptor.method_count()? as u16;
        Ok(info_count.max(desc_count))
    }

    /// Returns the [`ObjectTypeFlags`] for this object.
    ///
    /// # Errors
    ///
    /// Returns an error if the descriptor's `object_type_raw` field cannot
    /// be read.
    #[inline]
    pub fn object_type_flags(&self) -> Result<ObjectTypeFlags, Error> {
        Ok(ObjectTypeFlags(self.descriptor.object_type_raw()?))
    }

    /// Returns `true` if any entry of the object's method table is a P-Code
    /// procedure ([`MethodEntry::PCode`]: the slot points at one of the
    /// object's `ProcDscInfo`s).
    ///
    /// `true` for every object with a procedure in the P-Code fixtures,
    /// `false` for an object without one (`data` `Item`, `statics` `Other`,
    /// a form with no code) and for every object of the natively compiled
    /// `flow-native` and `events-native`.
    ///
    /// # Errors
    ///
    /// Returns an error if the method table's VA or counts cannot be read.
    /// A slot that does not read counts as not P-Code.
    pub fn has_pcode(&self) -> Result<bool, Error> {
        Ok(self
            .methods()?
            .any(|entry| matches!(entry, Ok(MethodEntry::PCode(_)))))
    }

    /// Returns the number of entries in this object's control table.
    ///
    /// Forms and UserControls list their controls and themselves; a class
    /// lists itself (`"Class"`) plus one entry per implemented interface or
    /// `WithEvents` variable (`calls`: `Square` 2, `Counter` 1). Returns 0
    /// if no optional info is present (standard modules).
    ///
    /// # Errors
    ///
    /// Returns an error if the optional info's `control_count` cannot be read.
    #[inline]
    pub fn control_count(&self) -> Result<u32, Error> {
        match self.optional_info.as_ref() {
            Some(opt) => opt.control_count(),
            None => Ok(0),
        }
    }

    /// Returns an iterator over controls on this object.
    ///
    /// Controls are GUI elements (buttons, textboxes, etc.) on VB6 forms,
    /// plus an entry for the object itself; a class's table holds the class
    /// and its implemented interfaces and `WithEvents` variables (see
    /// [`control_count`](Self::control_count)). Returns an empty iterator if
    /// the object has no optional info or no controls.
    ///
    /// # Errors
    ///
    /// Returns an error if the optional info's control count or VA fields
    /// cannot be read.
    pub fn controls(&self) -> Result<ControlEntryIterator<'a, 'p>, Error> {
        let (controls_va, count) = match self.optional_info.as_ref() {
            Some(opt) => {
                let cc = opt.control_count()?;
                let cv = opt.controls_va()?;
                if cc > 0 && cv != 0 { (cv, cc) } else { (0, 0) }
            }
            None => (0, 0),
        };

        Ok(ControlEntryIterator::new(
            self.project.address_map(),
            controls_va,
            count,
        ))
    }

    /// Returns an iterator over controls enriched with form binary data types.
    ///
    /// Like [`controls`](Self::controls), but each yielded control has its
    /// [`form_control_type`](crate::project::VbControl::form_control_type) populated from the
    /// form binary data `cType` byte - the **authoritative** control type.
    ///
    /// Use this when form data is available (parsed from
    /// [`GuiTableEntry::form_data_va`](crate::vb::guitable::GuiTableEntry::form_data_va)).
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying control table fields cannot be read.
    pub fn controls_with_form_data(
        &self,
        form_data: &'p FormDataParser<'a>,
    ) -> Result<ControlEntryIterator<'a, 'p>, Error> {
        Ok(self.controls()?.with_form_data(form_data))
    }

    /// Returns an iterator over the method link table: the code addresses of
    /// the object's COM vtable, in vtable order.
    ///
    /// In the P-Code fixtures each method's entry is a
    /// `xor eax, eax; mov edx, <ProcDscInfo>; push <jmp [MethCallEngine]>; ret`
    /// stub; public and `WithEvents` variables add accessor thunks, and a
    /// class that implements interfaces has three null entries per
    /// interface, after its public variables' accessors.
    /// Entry `k` is not method `k` when the vtable order differs from the
    /// method table order; see [`MethodLink`](crate::project::MethodLink).
    ///
    /// Returns an empty iterator if no method link table exists.
    ///
    /// # Errors
    ///
    /// Returns an error if the method link count or table VA cannot be read.
    pub fn method_links(&self) -> Result<MethodLinkIterator<'a, 'p>, Error> {
        let (table_va, count) = match self.optional_info.as_ref() {
            Some(opt) => {
                let mlc = opt.method_link_count()?;
                let mlv = opt.method_link_table_va()?;
                if mlc > 0 && mlv != 0 {
                    (mlv, u32::from(mlc))
                } else {
                    (0, 0)
                }
            }
            None => (0, 0),
        };

        Ok(MethodLinkIterator::new(
            self.project.address_map(),
            table_va,
            count,
        ))
    }

    /// Returns `true` if this object has a real method dispatch table.
    ///
    /// Answered by [`ObjectInfo::has_method_table`]: the table is trusted only
    /// when it is not the constants pool and the object's own method count is
    /// non-zero. [`method_count`](Self::method_count) takes the larger of that
    /// count and the descriptor's, so without the gate a descriptor count would
    /// be iterated over a pointer the object never initialized.
    ///
    /// # Errors
    ///
    /// Returns an error if the method count or either VA cannot be read.
    pub fn has_method_table(&self) -> Result<bool, Error> {
        self.info.has_method_table()
    }

    /// Returns an iterator over all method table entries, classified by type.
    ///
    /// Each entry is classified as [`MethodEntry::Null`],
    /// [`MethodEntry::Declare`], [`MethodEntry::PCode`],
    /// [`MethodEntry::Native`], or [`MethodEntry::Runtime`], one per slot,
    /// so the position of an entry is its method index (the index of
    /// [`method_name`](Self::method_name) and
    /// [`func_type_descs`](Self::func_type_descs)). Use
    /// [`pcode_methods`](Self::pcode_methods) if you only want P-Code
    /// methods.
    ///
    /// In a standard module the slots before the first procedure are its
    /// `Declare` statements, which hold no pointer: they are
    /// [`MethodEntry::Declare`].
    ///
    /// Returns an empty iterator if the object has no method table: a
    /// module or class with no procedures (`statics` `Other`, `data`
    /// `Item`) or a natively compiled module (`flow-native`). The count is
    /// capped at the slots the file holds from the table on.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying method/constants VAs or method
    /// counts cannot be read.
    pub fn methods(&self) -> Result<MethodIterator<'a, 'p>, Error> {
        let map = self.project.address_map();
        let methods_va = self.info.methods_va()?;
        let object_info_va = self.descriptor.object_info_va()?;
        let total = if self.has_method_table()? {
            let available = map.slice_from_va(methods_va, 0).map_or(0, <[u8]>::len) / 4;
            self.method_count()?
                .min(u16::try_from(available).unwrap_or(u16::MAX))
        } else {
            0
        };
        let declares = if self.descriptor.is_module() {
            (0..total)
                .find(|&i| MethodEntry::is_procedure(map, methods_va, i, object_info_va))
                .unwrap_or(total)
        } else {
            0
        };
        Ok(MethodIterator {
            map,
            methods_va,
            object_info_va,
            declares,
            index: 0,
            total,
        })
    }

    /// Returns an iterator over P-Code methods in this object.
    ///
    /// The [`MethodEntry::PCode`] entries of [`methods`](Self::methods);
    /// the others (null, `Declare`, native, runtime) are skipped.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying method/constants VAs or method
    /// counts cannot be read.
    pub fn pcode_methods(&self) -> Result<PCodeMethodIterator<'a, 'p>, Error> {
        Ok(PCodeMethodIterator {
            inner: self.methods()?,
        })
    }

    /// Parses the object's module-level variable descriptor table
    /// ([`PublicObjectDescriptor::public_bytes_va`]).
    ///
    /// Modules, classes, forms and UserControls share the format (see
    /// [`ClassFormPublicBytes`]): one variable-length entry per variable
    /// that needs initialization or cleanup (`flow`: `Private m_Log As
    /// String` is a String entry at offset 0). Returns `None` when the VA is
    /// null or the table cannot be read.
    pub fn public_bytes(&self) -> Option<ClassFormPublicBytes<'a>> {
        self.variable_table(self.descriptor.public_bytes_va().ok()?)
    }

    /// Parses the descriptor table of the object's `Static` locals
    /// ([`PublicObjectDescriptor::static_bytes_va`]), in the same format as
    /// [`public_bytes`](Self::public_bytes).
    ///
    /// Returns `None` when the object has no `Static` locals (VA 0) or the
    /// table cannot be read.
    pub fn static_bytes(&self) -> Option<ClassFormPublicBytes<'a>> {
        self.variable_table(self.descriptor.static_bytes_va().ok()?)
    }

    /// Reads the variable descriptor table at `va`, `+0x00` bytes long.
    fn variable_table(&self, va: u32) -> Option<ClassFormPublicBytes<'a>> {
        if va == 0 {
            return None;
        }
        let map = self.project.address_map();
        let header = map.slice_from_va(va, ClassFormPublicBytes::MIN_SIZE).ok()?;
        let size = usize::from(read_u16_le(header, 0).ok()?).max(ClassFormPublicBytes::MIN_SIZE);
        ClassFormPublicBytes::parse(header.get(..size)?).ok()
    }

    /// Returns all code entry points in this object.
    ///
    /// Combines three sources into a single `Vec`, one entry per address:
    /// 1. **Method table** - P-Code and native methods from the dispatch
    ///    table. A P-Code entry's `va` is its first P-Code byte, its
    ///    `proc_dsc_va` its `ProcDscInfo` and its `stub_va` the method link
    ///    stub that enters it through `MethCallEngine`, when the object has
    ///    one (classes, forms, UserControls).
    /// 2. **Method link entries** - the [`method_links`](Self::method_links)
    ///    that are not a method's stub: member variable accessors
    ///    ([`MethodLinkKind::Variable`]) and, for a natively compiled object,
    ///    jumps to code not in the method table, as
    ///    [`CodeEntryKind::NativeThunk`]. A link that enters a listed P-Code
    ///    method is that method's `stub_va`, not a separate entry; null
    ///    links are skipped.
    /// 3. **Event handlers** - the event stubs in the controls' event sink
    ///    vtables, labelled `ControlName_EventName`, with the method index of
    ///    the procedure each enters.
    ///
    /// Null entries, `Declare` slots and slot values outside the image are
    /// excluded.
    ///
    /// # Name Resolution
    ///
    /// Method names are resolved using a three-tier fallback:
    /// 1. Method name table (`method_name()`) - from PublicObjectDescriptor
    /// 2. FuncTypDesc signature without a name, e.g. `Function (x As Long) As Long`
    /// 3. `None` (callers format their own, e.g. `method_NN`)
    ///
    /// When `form_data` is provided, event handler names use the exact
    /// `FormControlType` from the form binary (e.g., `Timer1_Timer` instead
    /// of `Timer1_Event00`). Without it, falls back to GUID-based class
    /// name lookup which is less reliable.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying method table, method link table,
    /// control table, or any control name VA cannot be read.
    pub fn code_entries(
        &self,
        form_data: Option<&FormDataParser<'a>>,
    ) -> Result<Vec<CodeEntry>, Error> {
        let mut entries = Vec::new();
        let mut seen = HashSet::new();
        let map = self.project.address_map();

        // Build FuncTypDesc map for name fallback
        let ftd_map = self.build_func_type_desc_map()?;

        // The method link table: the stub of each P-Code procedure it enters,
        // by ProcDscInfo. An unreadable link says nothing about the others.
        let links: Vec<_> = self
            .method_links()?
            .filter_map(|link| match link {
                Ok(link) => Some(link),
                Err(e) => {
                    crate::trace::warn_drop!("code_entries.method_links", error = ?e);
                    None
                }
            })
            .collect();
        let mut stub_by_dsc: HashMap<u32, u32> = HashMap::new();
        for link in &links {
            if let MethodLinkKind::Procedure { proc_dsc_va } = link.kind {
                stub_by_dsc.entry(proc_dsc_va).or_insert(link.thunk_va);
            }
        }

        // 1. Method table entries
        //    One unreadable slot is dropped on its own; it says nothing about the
        //    slots beside it.
        let mut method_by_dsc: HashMap<u32, u16> = HashMap::new();
        for (i, result) in self.methods()?.enumerate() {
            let entry = match result {
                Ok(entry) => entry,
                Err(e) => {
                    crate::trace::warn_drop!("code_entries.methods", error = ?e);
                    continue;
                }
            };
            let index = u16::try_from(i).unwrap_or(u16::MAX);
            match entry {
                MethodEntry::PCode(pm) => {
                    let pcode_size = match pm.proc_size() {
                        Ok(size) => size,
                        Err(e) => {
                            crate::trace::warn_drop!("code_entries.proc_size", error = ?e);
                            continue;
                        }
                    };
                    let proc_dsc_va = pm.proc_dsc_va();
                    method_by_dsc.entry(proc_dsc_va).or_insert(index);
                    if !seen.insert(pm.pcode_va()) {
                        continue;
                    }
                    entries.push(CodeEntry {
                        va: pm.pcode_va(),
                        kind: CodeEntryKind::PCode,
                        method_index: Some(index),
                        name: self.resolve_method_name(i, &ftd_map),
                        data_const_va: Some(pm.data_const_va()),
                        stub_va: stub_by_dsc.get(&proc_dsc_va).copied(),
                        proc_dsc_va: Some(proc_dsc_va),
                        pcode_size: Some(pcode_size),
                    });
                }
                MethodEntry::Native { va } => {
                    if !seen.insert(va) {
                        continue;
                    }
                    entries.push(CodeEntry {
                        va,
                        kind: CodeEntryKind::Native,
                        method_index: Some(index),
                        name: self.resolve_method_name(i, &ftd_map),
                        data_const_va: None,
                        stub_va: None,
                        proc_dsc_va: None,
                        pcode_size: None,
                    });
                }
                MethodEntry::Null | MethodEntry::Declare | MethodEntry::Runtime { .. } => {}
            }
        }

        // 2. Method link entries that are not a listed method's stub.
        for link in &links {
            let (va, stub_va, proc_dsc_va) = match link.kind {
                MethodLinkKind::Empty => continue,
                MethodLinkKind::Procedure { proc_dsc_va } => {
                    if method_by_dsc.contains_key(&proc_dsc_va) {
                        continue;
                    }
                    (link.thunk_va, None, Some(proc_dsc_va))
                }
                MethodLinkKind::Jump => (link.code_va, Some(link.thunk_va), None),
                MethodLinkKind::Variable { .. } | MethodLinkKind::Other => {
                    (link.thunk_va, None, None)
                }
            };
            if va == 0 || !seen.insert(va) {
                continue;
            }
            entries.push(CodeEntry {
                va,
                kind: CodeEntryKind::NativeThunk,
                method_index: None,
                name: None,
                data_const_va: None,
                stub_va,
                proc_dsc_va,
                pcode_size: None,
            });
        }

        // 3. Event handler VAs from control event sinks. Bad control rows
        //    are silently skipped (fail-soft); under the `tracing` feature
        //    each drop emits a `visualbasic::dropped` warn event.
        let controls: Vec<_> = if let Some(fd) = form_data {
            self.controls_with_form_data(fd)?
                .filter_map(|r| match r {
                    Ok(c) => Some(c),
                    Err(e) => {
                        crate::trace::warn_drop!("code_entries.controls_with_form_data", error = ?e);
                        None
                    }
                })
                .collect()
        } else {
            self.controls()?
                .filter_map(|r| match r {
                    Ok(c) => Some(c),
                    Err(e) => {
                        crate::trace::warn_drop!("code_entries.controls", error = ?e);
                        None
                    }
                })
                .collect()
        };

        for ctrl in &controls {
            let ctrl_name_cow = ctrl.name();
            let ctrl_name = ctrl_name_cow.as_ref();
            // Resolve FormControlType: prefer form binary data, fall back to GUID class name
            let ctrl_type = ctrl
                .form_control_type()
                .or_else(|| ctrl.class_name().and_then(FormControlType::from_class_name));
            for slot in 0..ctrl.event_count()? {
                let Some(handler_va) = ctrl.event_handler_va(slot) else {
                    continue;
                };
                if handler_va == 0 || map.va_to_offset(handler_va).is_err() {
                    continue;
                }
                // Resolve event name: typed table, then the GUID's class, then Event{NN}
                let event_name = ctrl_type
                    .and_then(|ct| eventname::event_name(slot, ct))
                    .or_else(|| {
                        ctrl.class_name()
                            .and_then(|class| eventname::event_name_for_class(class, slot))
                    })
                    .or_else(|| self.extender_event_name(ctrl.guid(), slot));
                let label = match event_name {
                    Some(en) => format!("{ctrl_name}_{en}"),
                    None => format!("{ctrl_name}_Event{slot:02}"),
                };
                if !seen.insert(handler_va) {
                    continue;
                }
                let proc_dsc_va = map
                    .slice_from_va(handler_va, EventHandlerThunk::SIZE)
                    .ok()
                    .and_then(|data| EventHandlerThunk::parse_from_event_entry(data, handler_va))
                    .map(|thunk| thunk.proc_dsc_info_va);
                entries.push(CodeEntry {
                    va: handler_va,
                    kind: CodeEntryKind::EventHandler,
                    method_index: proc_dsc_va.and_then(|dsc| method_by_dsc.get(&dsc).copied()),
                    name: Some(label),
                    data_const_va: None,
                    stub_va: None,
                    proc_dsc_va,
                    pcode_size: None,
                });
            }
        }

        Ok(entries)
    }

    /// Resolves a method name using three-tier fallback:
    /// 1. Method name table
    /// 2. FuncTypDesc function name
    /// 3. None (caller can format as `method_NN`)
    fn resolve_method_name(
        &self,
        index: usize,
        ftd_map: &HashMap<usize, FuncTypDesc<'a>>,
    ) -> Option<String> {
        // Tier 1: method name table
        if let Ok(result) = self.method_name(u16::try_from(index).ok()?)
            && let MethodNameResult::Name(n) = result
            && let Ok(s) = str::from_utf8(n)
        {
            return Some(s.to_string());
        }

        // Tier 2: FuncTypDesc signature
        if let Some(ftd) = ftd_map.get(&index) {
            let name = format_signature(ftd, "", self.project.address_map());
            // format_signature returns " ()" for empty name - strip prefix space
            let trimmed = name.trim();
            if !trimmed.is_empty() && trimmed != "()" {
                return Some(trimmed.to_string());
            }
        }

        None
    }

    /// Builds a map of method index to FuncTypDesc from the
    /// PrivateObjectDescriptor.
    fn build_func_type_desc_map(&self) -> Result<HashMap<usize, FuncTypDesc<'a>>, Error> {
        Ok(self
            .func_type_descs()?
            .map(|(i, ftd)| (i as usize, ftd))
            .collect())
    }

    /// Parses the form binary data for this object (forms with GUI data only).
    ///
    /// Requires a [`GuiTableEntry`](crate::vb::guitable::GuiTableEntry) that
    /// maps to this form. Returns `None` if the entry has no form data.
    pub fn form_data_from_gui_entry(
        &self,
        gui_entry: &GuiTableEntry<'a>,
    ) -> Option<FormDataParser<'a>> {
        let va = gui_entry.form_data_va().ok()?;
        let size = gui_entry.form_data_size().ok()? as usize;
        if va == 0 || size == 0 {
            return None;
        }
        let data = self.project.address_map().slice_from_va(va, size).ok()?;
        FormDataParser::parse(data).ok()
    }

    /// Returns all connected event-handler bindings on this object.
    ///
    /// Walks every control on the form, joins each event sink slot with
    /// the per-control-type event-name template, and yields one
    /// [`EventBinding`] per slot whose `handler_va` is non-zero.
    ///
    /// When `form_data` is provided, the authoritative `cType` byte from
    /// the form binary drives [`FormControlType`] resolution (e.g.,
    /// `Timer` → slot 0 = `"Timer"`, not the default `"Click"`).
    /// Without it, falls back to GUID-based class-name lookup.
    ///
    /// Returns an empty `Vec` for objects with no controls (modules,
    /// classes without GUI). The order of the returned bindings is:
    /// outer = control order from [`controls`](Self::controls), inner =
    /// ascending event slot index.
    ///
    /// # Errors
    ///
    /// Returns an error if the controls iterator or any control's
    /// [`event_count`](crate::project::VbControl::event_count) cannot
    /// be read.
    pub fn events(
        &self,
        form_data: Option<&'p FormDataParser<'a>>,
    ) -> Result<Vec<EventBinding<'a>>, Error> {
        self.events_inner(form_data, /* connected_only */ true)
    }

    /// Returns every event-handler slot on this object, including
    /// disconnected ones (`handler_va == 0`).
    ///
    /// Like [`events`](Self::events) but does not filter out empty slots -
    /// useful for completeness checks ("how many of this control's
    /// 24 events are wired up?") or for surfacing the full slot
    /// template per control type.
    ///
    /// # Errors
    ///
    /// Returns an error if the controls iterator or any control's
    /// [`event_count`](crate::project::VbControl::event_count) cannot
    /// be read.
    pub fn events_all_slots(
        &self,
        form_data: Option<&'p FormDataParser<'a>>,
    ) -> Result<Vec<EventBinding<'a>>, Error> {
        self.events_inner(form_data, /* connected_only */ false)
    }

    fn events_inner(
        &self,
        form_data: Option<&'p FormDataParser<'a>>,
        connected_only: bool,
    ) -> Result<Vec<EventBinding<'a>>, Error> {
        let mut bindings = Vec::new();
        let controls: Vec<_> = if let Some(fd) = form_data {
            self.controls_with_form_data(fd)?
                .filter_map(|r| match r {
                    Ok(c) => Some(c),
                    Err(e) => {
                        crate::trace::warn_drop!("events.controls_with_form_data", error = ?e);
                        None
                    }
                })
                .collect()
        } else {
            self.controls()?
                .filter_map(|r| match r {
                    Ok(c) => Some(c),
                    Err(e) => {
                        crate::trace::warn_drop!("events.controls", error = ?e);
                        None
                    }
                })
                .collect()
        };

        for ctrl in &controls {
            let ctrl_type = ctrl
                .form_control_type()
                .or_else(|| ctrl.class_name().and_then(FormControlType::from_class_name));
            let ctrl_index = ctrl.index()?;
            let event_count = ctrl.event_count()?;
            for slot in 0..event_count {
                let handler_va = ctrl.event_handler_va(slot).unwrap_or(0);
                if connected_only && handler_va == 0 {
                    continue;
                }
                let event_name = ctrl_type
                    .and_then(|ct| eventname::event_name(slot, ct))
                    .or_else(|| {
                        ctrl.class_name()
                            .and_then(|class| eventname::event_name_for_class(class, slot))
                    })
                    .or_else(|| self.extender_event_name(ctrl.guid(), slot));
                bindings.push(EventBinding {
                    control_index: ctrl_index,
                    control_name: ctrl.name(),
                    control_type: ctrl_type,
                    event_slot: slot,
                    event_name,
                    handler_va,
                });
            }
        }
        Ok(bindings)
    }

    /// Parses the form designer data for this object.
    ///
    /// Same as [`form_data_from_gui_entry`](Self::form_data_from_gui_entry):
    /// returns its [`FormDataParser`].
    #[inline]
    pub fn form_designer_data(&self, gui_entry: &GuiTableEntry<'a>) -> Option<FormDataParser<'a>> {
        self.form_data_from_gui_entry(gui_entry)
    }

    /// Returns a [`ConstantPool`] reader for this object's constants.
    ///
    /// The pool base VA comes from [`ObjectInfo::constants_va`] and
    /// contains BSTRs, API stubs, GUIDs, and code object references.
    ///
    /// # Errors
    ///
    /// Returns an error if `ObjectInfo::constants_va` cannot be read.
    pub fn constants_pool(&self) -> Result<ConstantPool<'a>, Error> {
        Ok(ConstantPool::new(
            self.project.address_map(),
            self.info.constants_va()?,
        ))
    }

    /// Returns the object's COM CLSID, if present.
    ///
    /// Resolves the CLSID from [`OptionalObjectInfo::object_clsid_va`].
    /// Returns `None` for standard modules or objects without a CLSID.
    pub fn object_clsid(&self) -> Option<Guid> {
        self.optional_info
            .as_ref()
            .and_then(|opt| opt.resolve_clsid(self.project.address_map()))
    }

    /// Returns an iterator over GUI GUIDs for this object.
    ///
    /// Delegates to [`OptionalObjectInfo::gui_guids`]. Returns an empty
    /// iterator if no optional info is present.
    pub fn gui_guids(&self) -> GuidTableIter<'_> {
        match self.optional_info.as_ref() {
            Some(opt) => opt.gui_guids(self.project.address_map()),
            None => GuidTableIter::new(self.project.address_map(), 0, 0),
        }
    }

    /// Returns an iterator over default dispatch interface IIDs.
    ///
    /// Delegates to [`OptionalObjectInfo::default_iids`]. Returns an empty
    /// iterator if no optional info is present.
    pub fn default_iids(&self) -> GuidTableIter<'_> {
        match self.optional_info.as_ref() {
            Some(opt) => opt.default_iids(self.project.address_map()),
            None => GuidTableIter::new(self.project.address_map(), 0, 0),
        }
    }

    /// Returns an iterator over event source interface IIDs.
    ///
    /// Delegates to [`OptionalObjectInfo::events_iids`]. Returns an empty
    /// iterator if no optional info is present.
    pub fn events_iids(&self) -> GuidTableIter<'_> {
        match self.optional_info.as_ref() {
            Some(opt) => opt.events_iids(self.project.address_map()),
            None => GuidTableIter::new(self.project.address_map(), 0, 0),
        }
    }

    /// Returns an iterator over [`FuncTypDesc`] entries.
    ///
    /// Walks the pointer array at [`PrivateObjectDescriptor::func_type_descs_va`],
    /// which runs parallel to the method table ([`method_count`](Self::method_count)
    /// entries), yielding `(method index, FuncTypDesc)` pairs and skipping
    /// the null entries of methods without a public prototype. Returns an
    /// empty iterator if no private object descriptor is present.
    ///
    /// # Errors
    ///
    /// Returns an error if the private object descriptor's func type descs VA
    /// or counts cannot be read.
    pub fn func_type_descs(&self) -> Result<FuncTypDescIter<'a, 'p>, Error> {
        let (ftd_va, total) = match self.private_object.as_ref() {
            Some(p) => {
                let va = p.func_type_descs_va()?;
                if va == 0 {
                    (0, 0)
                } else {
                    (va, u32::from(self.method_count()?))
                }
            }
            None => (0, 0),
        };
        Ok(FuncTypDescIter {
            map: self.project.address_map(),
            ftd_array_va: ftd_va,
            index: 0,
            total,
        })
    }

    /// Returns an iterator over [`VarStubDesc`](crate::vb::varstub::VarStubDesc) entries.
    ///
    /// Walks the pointer array at [`PrivateObjectDescriptor::var_stubs_va`].
    /// Returns an empty iterator if no private object descriptor is present
    /// or if there are no variable stubs.
    ///
    /// # Errors
    ///
    /// Returns an error if the private object descriptor's `var_stubs_va`
    /// or `var_stub_count` cannot be read.
    pub fn var_stubs(&self) -> Result<VarStubIter<'a>, Error> {
        let (stubs_va, count) = match self.private_object.as_ref() {
            Some(p) => {
                let va = p.var_stubs_va()?;
                let cnt = p.var_stub_count()?;
                if va != 0 && cnt > 0 {
                    (va, cnt)
                } else {
                    (0, 0)
                }
            }
            None => (0, 0),
        };
        Ok(VarStubIter::new(
            self.project.address_map(),
            stubs_va,
            count,
        ))
    }
}

/// Iterator over [`FuncTypDesc`] entries from a
/// [`PrivateObjectDescriptor`]'s pointer array.
///
/// Each entry in the array is a 4-byte VA pointing to a `FuncTypDesc`
/// structure. The iterator resolves each VA lazily, yielding
/// `(index, FuncTypDesc)` pairs. Null entries are silently skipped.
///
/// Created by [`VbObject::func_type_descs`].
#[must_use = "iterators are lazy and do nothing unless consumed"]
pub struct FuncTypDescIter<'a, 'p> {
    map: &'p AddressMap<'a>,
    ftd_array_va: u32,
    index: u32,
    total: u32,
}

impl<'a, 'p> Iterator for FuncTypDescIter<'a, 'p> {
    type Item = (u32, FuncTypDesc<'a>);

    fn next(&mut self) -> Option<Self::Item> {
        while self.index < self.total {
            let i = self.index;
            self.index = self.index.saturating_add(1);

            let ptr_va = self.ftd_array_va.wrapping_add(i.saturating_mul(4));
            let ptr_data = self.map.slice_from_va(ptr_va, 4).ok()?;
            let ptr_bytes: [u8; 4] = ptr_data.get(..4).and_then(|s| s.try_into().ok())?;
            let desc_va = u32::from_le_bytes(ptr_bytes);
            if desc_va == 0 {
                continue;
            }

            // Read extended data (0x40 bytes to cover arg types at +0x20)
            let desc_data = self.map.slice_from_va(desc_va, 0x40).ok()?;
            if let Ok(ftd) = FuncTypDesc::parse(desc_data) {
                return Some((i, ftd));
            }
        }
        None
    }
}

/// A code entry point discovered in a VB6 object.
///
/// Returned by [`VbObject::code_entries`], which combines method table entries,
/// method link thunks, and event handler VAs into a single list.
#[derive(Debug, Clone)]
pub struct CodeEntry {
    /// Virtual address of the code entry point.
    pub va: u32,
    /// What kind of code entry this is.
    pub kind: CodeEntryKind,
    /// Method table index: of the method for a method table entry, of the
    /// procedure the stub enters for an event handler; `None` for a
    /// [`CodeEntryKind::NativeThunk`] and for an event handler whose stub
    /// does not decode.
    pub method_index: Option<u16>,
    /// Human-readable name (method name or "ControlName_EventName").
    pub name: Option<String>,
    /// Constant pool base VA (`ObjectInfo.lpConstants`).
    /// Present for [`CodeEntryKind::PCode`] entries.
    pub data_const_va: Option<u32>,
    /// The x86 stub that enters the code: for a [`CodeEntryKind::PCode`]
    /// method of a class, form or UserControl, its method link stub
    /// (`xor eax, eax; mov edx, <ProcDscInfo>; push <jmp [MethCallEngine]>; ret`);
    /// for a [`CodeEntryKind::NativeThunk`] that is a jump, the jump.
    /// `None` for a standard module's procedures, which have no method
    /// links, and for event handlers, whose `va` is the stub.
    pub stub_va: Option<u32>,
    /// VA of the procedure's `ProcDscInfo`: present for
    /// [`CodeEntryKind::PCode`] entries, for an event handler whose stub
    /// decodes, and for a method link stub of a procedure not in the method
    /// table.
    pub proc_dsc_va: Option<u32>,
    /// Size of the P-Code byte stream in bytes.
    /// Present for [`CodeEntryKind::PCode`] entries.
    pub pcode_size: Option<u16>,
}

/// Classification of a code entry point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeEntryKind {
    /// P-Code procedure (bytecode); the entry's `va` is its first P-Code byte.
    PCode,
    /// A method table slot taken to be native code. No fixture yields one.
    Native,
    /// A method link (vtable) entry that does not enter a listed method: in
    /// the P-Code fixtures a member variable accessor (`data` `Item.Name`,
    /// `events` `Listener.m_Source`), x86 code that jumps into the runtime.
    NativeThunk,
    /// The event stub in a control's event sink vtable, which enters the
    /// handler procedure through `MethCallEngine`.
    EventHandler,
}

/// A single event-handler binding on a VB6 form's control.
///
/// Yielded by [`VbObject::events`]. Joins the control's event sink vtable
/// with the per-control-type event-name template, surfacing the
/// (control, slot) → handler-VA → event-name mapping that's otherwise
/// scattered across [`controls`](VbObject::controls), the
/// [`EventSinkVtable`](crate::vb::events::EventSinkVtable) header, and the
/// [`eventname`](crate::vb::eventname) lookup tables.
///
/// Only **connected** handlers are yielded - slots whose
/// `event_handler_va == 0` (event not wired up by the user) are filtered
/// out. Use [`VbObject::events_all_slots`] for the full per-slot view
/// including disconnected events.
#[derive(Debug, Clone)]
pub struct EventBinding<'a> {
    /// Index of the control on the form (matches
    /// [`VbControl::index`](crate::project::VbControl::index)).
    pub control_index: u16,
    /// Control name as a lossy UTF-8 string (e.g., `"Command1"`,
    /// `"Timer1"`). Empty when the control has no name. Borrows the
    /// underlying bytes when they're already valid UTF-8.
    pub control_name: Cow<'a, str>,
    /// Authoritative control type, if it can be resolved.
    ///
    /// Resolution prefers form binary data (`cType` byte) over the class the
    /// control's GUID names. `None` when neither source resolves the type -
    /// in that case [`event_name`](Self::event_name) comes from the events
    /// of the class the GUID names, if any.
    pub control_type: Option<FormControlType>,
    /// Zero-based slot in the control's event sink vtable.
    pub event_slot: u16,
    /// Resolved event name from the per-control-type template
    /// (e.g., `"Click"`, `"KeyPress"`, `"Timer"`). `None` when the slot
    /// is past the end of every known template (e.g., custom OCX
    /// events with no static lookup).
    pub event_name: Option<&'static str>,
    /// Virtual address of the handler stub. Always non-zero for
    /// bindings yielded by [`VbObject::events`] (the connected-only
    /// walker); may be zero when iterating all slots via
    /// [`VbObject::events_all_slots`].
    pub handler_va: u32,
}

impl<'a> EventBinding<'a> {
    /// Returns `true` if the control has a wired-up handler at this slot.
    #[inline]
    pub fn is_connected(&self) -> bool {
        self.handler_va != 0
    }

    /// Returns a `"ControlName_EventName"` label, falling back to
    /// `"ControlName_Event{NN}"` when the event name is unknown and
    /// `"_Event{NN}"` when the control name is empty.
    pub fn label(&self) -> String {
        let ctrl = self.control_name.as_ref();
        match self.event_name {
            Some(name) => format!("{ctrl}_{name}"),
            None => format!("{ctrl}_Event{:02}", self.event_slot),
        }
    }
}

/// Iterator over all method table entries in a VB6 object, classified by type.
///
/// Created by [`VbObject::methods`]. Each yielded [`MethodEntry`] is
/// classified as null, `Declare`, P-Code, native, or runtime based on the
/// VA at that slot and the slot's position.
#[must_use = "iterators are lazy and do nothing unless consumed"]
pub struct MethodIterator<'a, 'p> {
    /// Address map for VA resolution.
    map: &'p AddressMap<'a>,
    /// Base VA of the method dispatch table.
    methods_va: u32,
    /// VA of the owning object's `ObjectInfo`.
    object_info_va: u32,
    /// Number of leading `Declare` slots (standard modules only).
    declares: u16,
    /// Current zero-based slot position.
    index: u16,
    /// Total number of slots in the method table.
    total: u16,
}

impl<'a, 'p> Iterator for MethodIterator<'a, 'p> {
    type Item = Result<MethodEntry<'a>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.total {
            return None;
        }
        let i = self.index;
        self.index = self.index.saturating_add(1);
        if i < self.declares {
            return Some(Ok(MethodEntry::Declare));
        }
        Some(MethodEntry::classify(
            self.map,
            self.methods_va,
            i,
            self.object_info_va,
        ))
    }
}

/// Iterator over P-Code methods in a VB6 object, skipping non-P-Code entries.
///
/// Created by [`VbObject::pcode_methods`]: the [`MethodEntry::PCode`]
/// entries of a [`MethodIterator`], and its errors.
#[must_use = "iterators are lazy and do nothing unless consumed"]
pub struct PCodeMethodIterator<'a, 'p> {
    /// The method table walk.
    inner: MethodIterator<'a, 'p>,
}

impl<'a, 'p> Iterator for PCodeMethodIterator<'a, 'p> {
    type Item = Result<PCodeMethod<'a>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.find_map(|entry| match entry {
            Ok(MethodEntry::PCode(method)) => Some(Ok(method)),
            Ok(_) => None,
            Err(e) => Some(Err(e)),
        })
    }
}
