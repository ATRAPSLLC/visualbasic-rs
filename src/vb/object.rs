//! PublicObjectDescriptor, ObjectInfo, and OptionalObjectInfo structure parsers.
//!
//! These structures describe individual objects (forms, modules, classes)
//! within a VB6 project.

use std::fmt;

use crate::{
    addressmap::AddressMap,
    error::Error,
    util::{read_u16_le, read_u32_le},
    vb::control::Guid,
};

/// View over a PublicObjectDescriptor structure (0x30 bytes).
///
/// Each entry in the object array describes one VB6 object (module, class,
/// form, UserControl).
///
/// # Layout
///
/// | Offset | Size | Field |
/// |--------|------|-------|
/// | 0x00 | 4 | `lpObjectInfo` (VA of [`ObjectInfo`]) |
/// | 0x04 | 4 | Reserved (0xFFFFFFFF in every fixture) |
/// | 0x08 | 4 | `lpPublicBytes` (descriptor table of the object's module-level variables) |
/// | 0x0C | 4 | `lpStaticBytes` (descriptor table of the object's `Static` locals; 0 when it has none) |
/// | 0x10 | 4 | `lpModulePublic` (.data VA of a module's variables; 0 for other objects) |
/// | 0x14 | 4 | `lpModuleStatic` (.data VA of a module's `Static` block; 0 otherwise) |
/// | 0x18 | 4 | `lpszObjectName` (null-terminated ANSI string VA) |
/// | 0x1C | 4 | `dwMethodCount` |
/// | 0x20 | 4 | `lpMethodNames` (VA; non-module objects only; 0 for modules) |
/// | 0x24 | 4 | `oStaticVars` (offset of the `Static` block pointer; 0xFFFF when none) |
/// | 0x28 | 4 | `fObjectType` (type flags, see below) |
/// | 0x2C | 4 | Reserved (0 in every fixture) |
///
/// # fObjectType values
///
/// | Low byte | Type | Full value (in `tests/fixtures`) |
/// |----------|------|---------------------|
/// | `0x01` | Standard module (.bas) | `0x00018001` |
/// | `0x03` | Class module (.cls) | `0x00118003` |
/// | `0x03` | UserControl (.ctl) | `0x001DA003` (`dispid` Dial, `forms` Gauge) |
/// | `0x83` | Form / UserDocument | `0x00018083` |
#[derive(Clone, Copy, Debug)]
pub struct PublicObjectDescriptor<'a> {
    /// Raw backing bytes borrowed from the PE file buffer.
    bytes: &'a [u8],
}

impl<'a> PublicObjectDescriptor<'a> {
    /// Total size of the structure in bytes.
    pub const SIZE: usize = 0x30;

    /// Parses a PublicObjectDescriptor from the given byte slice.
    ///
    /// # Errors
    ///
    /// Returns [`Error::TooShort`] if `data.len() < 0x30`.
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        if data.len() < Self::SIZE {
            return Err(Error::TooShort {
                expected: Self::SIZE,
                actual: data.len(),
                context: "PublicObjectDescriptor",
            });
        }
        let bytes = data.get(..Self::SIZE).ok_or(Error::TooShort {
            expected: Self::SIZE,
            actual: data.len(),
            context: "PublicObjectDescriptor",
        })?;
        Ok(Self { bytes })
    }

    /// Virtual address of the [`ObjectInfo`] structure at offset 0x00.
    #[inline]
    pub fn object_info_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x00)
    }

    /// Reserved field at offset 0x04 (0xFFFFFFFF in every fixture).
    #[inline]
    pub fn reserved(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x04)
    }

    /// Variable descriptor table VA at offset 0x08.
    ///
    /// The same table format for every object type: a 0x0C-byte header
    /// whose `+0x02` is the size of the variable data block, then one
    /// variable-length entry per module-level variable that needs
    /// initialization or cleanup (`String`, `Variant`, `Object`, arrays,
    /// UDTs; a `Long` has none), Private as well as Public. See
    /// [`ClassFormPublicBytes`](super::publicbytes::ClassFormPublicBytes).
    /// For a module, the runtime zero-fills
    /// [`module_public_va`](Self::module_public_va) with the `+0x02` size
    /// and walks the entries (MSVBVM60 6.00.8176 `0x660276C1`).
    #[inline]
    pub fn public_bytes_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x08)
    }

    /// Static variable descriptor table VA at offset 0x0C.
    ///
    /// Same format as the table at [`public_bytes_va`](Self::public_bytes_va),
    /// describing the object's `Static` locals; its `+0x02` is the size of
    /// the static block. 0 when the object has no `Static` locals.
    /// `tests/fixtures/statics`: module `Program` 0x00401778 (block size
    /// 0x20), class `Holder` 0x004017B4 (0x08). For a module the runtime
    /// zero-fills [`module_static_va`](Self::module_static_va) with that
    /// size and walks the entries (6.00.8176 `0x6602773F`).
    #[inline]
    pub fn static_bytes_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x0C)
    }

    /// Module variable block .data section VA at offset 0x10.
    ///
    /// Non-zero only for standard modules (.bas); 0 for other objects.
    /// Always 8 bytes after [`ObjectInfo::object_data_va`] in the
    /// fixtures. `ProcCallEngine` pushes this VA as the `ebp+8` slot of a
    /// module procedure (6.00.8176 `0x66104A9C`), which is the base of the
    /// module's `FMem*` accesses.
    #[inline]
    pub fn module_public_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x10)
    }

    /// Module static block .data section VA at offset 0x14.
    ///
    /// Non-zero only for a module with `Static` locals
    /// (`tests/fixtures/statics` `Program`: 0x00402074); 0 otherwise,
    /// including classes with `Static` locals.
    #[inline]
    pub fn module_static_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x14)
    }

    /// Object name string VA at offset 0x18.
    ///
    /// Points to a null-terminated ANSI string (e.g., `"Form1"`,
    /// `"Program"`, `"Counter"`).
    #[inline]
    pub fn object_name_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x18)
    }

    /// Number of methods at offset 0x1C.
    ///
    /// One per `Sub`, `Function` and `Property` procedure (event handlers
    /// included) in source order, plus, in a module, one per `Declare`
    /// statement (`tests/fixtures/vtable`: 13 `Declare`s and 3 procedures,
    /// count 16). Public variables of a class are not counted. Equal to
    /// [`ObjectInfo::method_count`] in every P-Code fixture; in the native
    /// build (`flow-native`) this is 25 while the `ObjectInfo` count is 0.
    #[inline]
    pub fn method_count(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x1C)
    }

    /// Method names table VA at offset 0x20.
    ///
    /// Points to an array of [`method_count`](Self::method_count) VAs, each
    /// to a null-terminated ANSI name. Public members, `Implements`
    /// members and `WithEvents` handlers of a class are named; `Private`
    /// and `Friend` procedures and a form's event handlers have 0 or
    /// 0xFFFFFFFF instead of a VA (`tests/fixtures/types` `Kinds`:
    /// 0xFFFFFFFF for `Friend Rec` and `Private Hidden`). Non-zero for every
    /// non-module object; always 0 for standard modules. When
    /// `method_count() == 0`, this value is not an address (`data` `Item`:
    /// 0x020507A8).
    #[inline]
    pub fn method_names_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x20)
    }

    /// Offset of the `Static` block pointer at offset 0x24.
    ///
    /// The byte offset, within the module's data block (the `ebp+8` base)
    /// or the class instance (`Me`), of the pointer to the object's
    /// `Static` locals block. `0x0000FFFF` when the object has no `Static`
    /// locals. `tests/fixtures/statics`: module `Program` 0x3C, class
    /// `Holder` 0x40.
    #[inline]
    pub fn static_vars_offset(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x24)
    }

    /// Object type flags at offset 0x28.
    ///
    /// Low byte determines the object type:
    /// - `0x01` = standard module (.bas)
    /// - `0x03` = class module (.cls) or UserControl (.ctl)
    /// - `0x83` = form / UserDocument
    ///
    /// See [`ObjectTypeFlags`](super::flags::ObjectTypeFlags) for bit
    /// definitions.
    #[inline]
    pub fn object_type_raw(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x28)
    }

    /// Returns `true` if flag `0x01` is set.
    ///
    /// The flag does not say whether an [`OptionalObjectInfo`] exists: it is
    /// set on every object in the fixtures, standard modules included, and
    /// a module has none (its constants pool starts at `ObjectInfo + 0x38`).
    /// The presence of `OptionalObjectInfo` is determined spatially (gap
    /// between `ObjectInfo` and the constants pool).
    ///
    /// Returns `false` if the underlying type field cannot be read.
    #[inline]
    pub fn has_optional_info(&self) -> bool {
        self.object_type_raw().unwrap_or(0) & 0x01 != 0
    }

    /// Returns `true` if bit `0x02` is set and bit `0x80` is clear (low
    /// byte `0x03`): a class module or a UserControl.
    ///
    /// Returns `false` if the underlying type field cannot be read.
    #[inline]
    pub fn is_class(&self) -> bool {
        let raw = self.object_type_raw().unwrap_or(0);
        raw & 0x02 != 0 && raw & 0x80 == 0
    }

    /// Returns `true` if this is a form or UserDocument (low byte `0x83`).
    ///
    /// Returns `false` if the underlying type field cannot be read.
    #[inline]
    pub fn is_form(&self) -> bool {
        self.object_type_raw().unwrap_or(0) & 0x82 == 0x82
    }

    /// Returns `true` if this is a standard module (low byte `0x01`).
    ///
    /// Returns `false` if the underlying type field cannot be read.
    #[inline]
    pub fn is_module(&self) -> bool {
        self.object_type_raw().unwrap_or(0) & 0x82 == 0x00
    }

    /// Reserved field at offset 0x2C (always 0 after compilation).
    #[inline]
    pub fn null_2c(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x2C)
    }
}

/// View over an ObjectInfo structure (0x38 bytes).
///
/// Contains method table, constants pool, and links back to the parent
/// structures.
///
/// `ProcCallEngine` (MSVBVM60 6.00.8176 `0x66104A99`) reads
/// `lpPublicObject` (+0x18, `0x66104A9C`), `lpConstants` (+0x34,
/// `0x66104ADC`) and `lpObjectTable` (+0x04, `0x66104AE2`) from this
/// structure on every P-Code procedure entry.
///
/// # Layout
///
/// | Offset | Size | Field |
/// |--------|------|-------|
/// | 0x00 | 2 | `wRefCount` (1 in every fixture) |
/// | 0x02 | 2 | `wObjectIndex` (zero-based) |
/// | 0x04 | 4 | `lpObjectTable` (back-pointer) |
/// | 0x08 | 4 | `lpIdeData` (0 in every fixture) |
/// | 0x0C | 4 | `lpPrivateObject` (0xFFFFFFFF for modules) |
/// | 0x10 | 4 | Reserved (0xFFFFFFFF in every fixture) |
/// | 0x14 | 4 | Reserved (0 in every fixture) |
/// | 0x18 | 4 | `lpPublicObject` (back-pointer to descriptor) |
/// | 0x1C | 4 | `lpObjectData` (per-object .data section area) |
/// | 0x20 | 2 | `wMethodCount` |
/// | 0x22 | 2 | `wMethodCountIde` (0 in every fixture) |
/// | 0x24 | 4 | `lpMethods` (VA of the array of [`ProcDscInfo`](super::procedure::ProcDscInfo) VAs) |
/// | 0x28 | 2 | `wConstantsCount` |
/// | 0x2A | 2 | `wMaxConstants` (pool capacity) |
/// | 0x2C | 4 | Reserved (0 in every fixture) |
/// | 0x30 | 4 | Unknown: an address outside the image when the pool has entries, else 0 |
/// | 0x34 | 4 | `lpConstants` (constants pool VA) |
#[derive(Clone, Copy, Debug)]
pub struct ObjectInfo<'a> {
    /// Raw backing bytes borrowed from the PE file buffer.
    bytes: &'a [u8],
}

impl<'a> ObjectInfo<'a> {
    /// Total size of the structure in bytes.
    pub const SIZE: usize = 0x38;

    /// Parses an ObjectInfo from the given byte slice.
    ///
    /// # Errors
    ///
    /// Returns [`Error::TooShort`] if `data.len() < 0x38`.
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        if data.len() < Self::SIZE {
            return Err(Error::TooShort {
                expected: Self::SIZE,
                actual: data.len(),
                context: "ObjectInfo",
            });
        }
        let bytes = data.get(..Self::SIZE).ok_or(Error::TooShort {
            expected: Self::SIZE,
            actual: data.len(),
            context: "ObjectInfo",
        })?;
        Ok(Self { bytes })
    }

    /// Reference count at offset 0x00 (1 in every fixture).
    #[inline]
    pub fn ref_count(&self) -> Result<u16, Error> {
        read_u16_le(self.bytes, 0x00)
    }

    /// Object index at offset 0x02.
    #[inline]
    pub fn object_index(&self) -> Result<u16, Error> {
        read_u16_le(self.bytes, 0x02)
    }

    /// Back-pointer to the [`ObjectTable`](super::objecttable::ObjectTable) at offset 0x04.
    #[inline]
    pub fn object_table_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x04)
    }

    /// IDE data pointer at offset 0x08 (0 in every fixture).
    #[inline]
    pub fn ide_data(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x08)
    }

    /// Pointer to [`PrivateObjectDescriptor`](super::privateobj::PrivateObjectDescriptor) at offset 0x0C.
    ///
    /// `0xFFFFFFFF` for standard modules (which have no private descriptor).
    #[inline]
    pub fn private_object_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x0C)
    }

    /// Back-pointer to the [`PublicObjectDescriptor`] at offset 0x18.
    #[inline]
    pub fn public_object_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x18)
    }

    /// Per-object data area pointer at offset 0x1C.
    ///
    /// Every object has one, in the `.data` section. On load the runtime
    /// stores its per-object record in the first dword
    /// (MSVBVM60 6.00.9848 `0x6602F870`). For a module, the module's
    /// variable block ([`PublicObjectDescriptor::module_public_va`])
    /// starts 8 bytes after it.
    #[inline]
    pub fn object_data_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x1C)
    }

    /// Number of entries in the method table at offset 0x20.
    ///
    /// Equal to [`PublicObjectDescriptor::method_count`] in every P-Code
    /// fixture, `Declare` slots of a module included; 0 in the native
    /// build (`tests/fixtures/flow-native`), which has no method table.
    #[inline]
    pub fn method_count(&self) -> Result<u16, Error> {
        read_u16_le(self.bytes, 0x20)
    }

    /// IDE-only method count at offset 0x22 (0 in every fixture).
    #[inline]
    pub fn method_count_ide(&self) -> Result<u16, Error> {
        read_u16_le(self.bytes, 0x22)
    }

    /// Virtual address of the method table at offset 0x24.
    ///
    /// An array of [`method_count`](Self::method_count) dwords, one per
    /// method in source order, each the VA of the method's
    /// [`ProcDscInfo`](super::procedure::ProcDscInfo) (whose +0x00 points
    /// back to this `ObjectInfo`). In a module the slots of `Declare`
    /// statements hold no `ProcDscInfo` VA: the compiler reserves them and
    /// writes nothing, so they hold stale bytes (`tests/fixtures/exprs`: 0
    /// and 0xFFFFFFFF; `vtable`'s 13: small integers and the text
    /// `Vtable`). The table follows the constant pool. The runtime
    /// builds an object's vtable from the method link table, not from this
    /// one (see [`OptionalObjectInfo::basic_class_object_va`]). When
    /// `method_count() == 0`, this value is not an address (`flow-native`
    /// `Program`: 0x01FF8CA8).
    #[inline]
    pub fn methods_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x24)
    }

    /// Constants count at offset 0x28.
    #[inline]
    pub fn constants_count(&self) -> Result<u16, Error> {
        read_u16_le(self.bytes, 0x28)
    }

    /// Constants pool capacity at offset 0x2A.
    ///
    /// The smallest power of two not below
    /// [`constants_count`](Self::constants_count), at least 32, and 0 for an
    /// empty pool (fixtures: 5 -> 32, 51 -> 64, 142 -> 256, 0 -> 0).
    #[inline]
    pub fn max_constants(&self) -> Result<u16, Error> {
        read_u16_le(self.bytes, 0x2A)
    }

    /// Constants pool base pointer at offset 0x34.
    ///
    /// Use [`ConstantPool::new`](super::constantpool::ConstantPool::new) to create
    /// a reader for resolving string and API references from this base address.
    ///
    /// `ProcCallEngine` reads it on every P-Code procedure entry, through
    /// [`ProcDscInfo::object_info_va`](super::procedure::ProcDscInfo::object_info_va),
    /// and keeps it at `ebp-0x54` (6.00.8176 `0x66104ADC`). In a module the
    /// pool starts at this `ObjectInfo` + 0x38; in other objects it follows
    /// the [`OptionalObjectInfo`], and with no entries it is the VA of the
    /// method table.
    #[inline]
    pub fn constants_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x34)
    }

    /// Returns `true` if this object carries a real method dispatch table.
    ///
    /// Two layouts mean it does not. A table address equal to
    /// [`constants_va`](Self::constants_va) while the pool has entries is the
    /// constants/variable pool, not a table; with an empty pool
    /// ([`constants_count`](Self::constants_count) 0) the pool pointer simply
    /// lands on the method table, as in an interface class whose methods
    /// have no constants (`tests/fixtures/events` `Measure`,
    /// `tests/fixtures/calls` `Shape`). A zero
    /// [`method_count`](Self::method_count) leaves
    /// [`methods_va`](Self::methods_va) uninitialized, so whatever it holds is
    /// not an address worth reading.
    ///
    /// # Errors
    ///
    /// Returns an error if the method count, the constants count or either
    /// VA cannot be read.
    pub fn has_method_table(&self) -> Result<bool, Error> {
        let methods = self.methods_va()?;
        let overlaps_pool = methods == self.constants_va()? && self.constants_count()? > 0;
        Ok(methods != 0 && !overlaps_pool && self.method_count()? > 0)
    }
}

/// View over an OptionalObjectInfo structure (0x40 bytes).
///
/// Follows [`ObjectInfo`] in memory (at ObjectInfo + 0x38). Present for
/// classes, forms and UserControls (`fObjectType` bit `0x02`); a standard
/// module has none even though its `fObjectType` has bit `0x01`, and its
/// constants pool starts at ObjectInfo + 0x38 instead. Contains COM
/// interface GUIDs, the control array, the method link table and what the
/// runtime needs to build the object's vtable.
///
/// # Accessor fallibility
///
/// [`parse`](Self::parse) validates that the fixed 0x40-byte header is
/// present. After that, fixed-offset accessors on this type are only
/// fallible if the already-validated backing slice is unexpectedly too
/// short or arithmetic overflows while reading primitive fields. Resolvers
/// and iterators that follow VAs, such as [`resolve_clsid`](Self::resolve_clsid)
/// and [`typed_iids`](Self::typed_iids), are fail-soft and may return
/// `None` or skip entries when pointed-to data is absent, truncated, or
/// outside the PE image.
///
/// # Layout
///
/// | Offset | Size | Field |
/// |--------|------|-------|
/// | 0x00 | 4 | `gui_guids_count` - GUI GUID table entry count |
/// | 0x04 | 4 | `object_clsid_va` - VA of 16-byte object CLSID |
/// | 0x08 | 4 | `null_08` - 0 in every fixture (reserved) |
/// | 0x0C | 4 | `gui_guid_table_va` - VA of GUID VA-pointer array |
/// | 0x10 | 4 | `default_iid_count` - default IID table entry count |
/// | 0x14 | 4 | `events_iid_table_va` - VA of event source IID table |
/// | 0x18 | 4 | `events_iid_count` - event source IID count |
/// | 0x1C | 4 | `default_iid_table_va` - VA of default IID VA-pointer array |
/// | 0x20 | 4 | `control_count` - number of ControlInfo entries |
/// | 0x24 | 4 | `controls_va` - VA of ControlInfo array |
/// | 0x28 | 2 | `method_link_count` - method link entries (the object's own vtable slots) |
/// | 0x2A | 2 | `inherited_vtable_slots` - vtable slots between `IDispatch` and the method links (0 class, 439 form, 482 UserControl) |
/// | 0x2C | 2 | `initialize_event_offset` - 0x0C for classes, 0x68 for forms and UserControls |
/// | 0x2E | 2 | `terminate_event_offset` - `initialize_event_offset + 4` |
/// | 0x30 | 4 | `method_link_table_va` - VA of method link table |
/// | 0x34 | 4 | `basic_class_object_va` - VA of the runtime-built vtable block (.data) |
/// | 0x38 | 4 | `null_38` - 0 in every fixture (reserved) |
/// | 0x3C | 4 | `field_3c` - non-zero, an address outside the image |
///
/// # GUID Tables
///
/// The GUI GUID table at +0x0C is an array of `gui_guids_count` VA pointers,
/// each pointing to a 16-byte GUID. For a form or UserControl the GUID is
/// the one of its [`GuiTable`](super::guitable) entry; every class carries
/// `{FCFB3D2A-A0FA-1068-A738-08002B3371B5}`. The default IID table at
/// +0x1C uses the same format with `default_iid_count` entries. In every
/// fixture the GUI GUID table directly follows the method table
/// (`methods_va + 4 * method_count`), the default IID table follows it at
/// +4, the event IID table at +8, and the ControlInfo array follows the
/// event IID table.
///
/// # Vtable
///
/// At load the runtime fills the block at
/// [`basic_class_object_va`](Self::basic_class_object_va) (MSVBVM60
/// 6.00.8176 `0x66013971`): `+0x08` gets the `ObjectInfo` VA, the vtable
/// starts at `+0x0C` with the 3 `IUnknown` and 4 `IDispatch` slots, then
/// [`inherited_vtable_slots`](Self::inherited_vtable_slots) slots pointing at
/// runtime stubs, then the `method_link_count` dwords copied from
/// [`method_link_table_va`](Self::method_link_table_va). An object's own
/// method at index `k` of the method link table is therefore at vtable
/// offset `0x1C + 4 * (inherited_vtable_slots + k)`: `tests/fixtures/forms`
/// `Board.Reset` (form, 439, link 0) is called at 0x6F8, `Gauge`'s link 2
/// (UserControl, 482) at 0x7AC. The block is
/// [`basic_class_object_size`](Self::basic_class_object_size) bytes long.
///
/// Nothing in this structure says whether the object is P-Code or native
/// code: that is a property of its method table entries (see
/// [`MethodEntry`](crate::project::MethodEntry)).
///
/// # Initialize / Terminate Offsets
///
/// The offsets at +0x2C and +0x2E are 0x0C/0x10 for every class and
/// 0x68/0x6C for every form and UserControl in the fixtures, whether or not
/// the object has a `Class_Initialize`/`Form_Initialize` handler. They are
/// not offsets into the object's own method link slots (`events` `Source`
/// has 3 method links, and +0x28 + 0x0C from its block is the next
/// object's block); what they index is unconfirmed.
#[derive(Clone, Copy, Debug)]
pub struct OptionalObjectInfo<'a> {
    /// Raw backing bytes borrowed from the PE file buffer.
    bytes: &'a [u8],
}

impl<'a> OptionalObjectInfo<'a> {
    /// Total size of the structure in bytes.
    pub const SIZE: usize = 0x40;

    /// Parses an OptionalObjectInfo from the given byte slice.
    ///
    /// # Errors
    ///
    /// Returns [`Error::TooShort`] if `data.len() < 0x40`.
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        if data.len() < Self::SIZE {
            return Err(Error::TooShort {
                expected: Self::SIZE,
                actual: data.len(),
                context: "OptionalObjectInfo",
            });
        }
        let bytes = data.get(..Self::SIZE).ok_or(Error::TooShort {
            expected: Self::SIZE,
            actual: data.len(),
            context: "OptionalObjectInfo",
        })?;
        Ok(Self { bytes })
    }

    /// GUI GUID table entry count at offset 0x00.
    ///
    /// Number of VA pointers in the table at [`gui_guid_table_va`](Self::gui_guid_table_va).
    /// Each entry is a VA pointing to a 16-byte GUID; for a form or
    /// UserControl it is the GUID of its
    /// [`GuiTableEntry`](super::guitable::GuiTableEntry). 1 in every fixture.
    #[inline]
    pub fn gui_guids_count(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x00)
    }

    /// VA of the object's 16-byte CLSID at offset 0x04.
    #[inline]
    pub fn object_clsid_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x04)
    }

    /// Reserved field at offset 0x08 (0 in every fixture).
    #[inline]
    pub fn null_08(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x08)
    }

    /// VA of the GUI GUID VA-pointer table at offset 0x0C.
    ///
    /// Array of [`gui_guids_count`](Self::gui_guids_count) dword VAs, each
    /// pointing to a 16-byte GUID. For a form or UserControl the GUID
    /// matches its [`GuiTable`](super::guitable) entry; every class has
    /// `{FCFB3D2A-A0FA-1068-A738-08002B3371B5}`.
    #[inline]
    pub fn gui_guid_table_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x0C)
    }

    /// Default IID table entry count at offset 0x10.
    ///
    /// Number of VA pointers in the table at [`default_iid_table_va`](Self::default_iid_table_va).
    /// 1 in every fixture (one default dispatch IID per object).
    #[inline]
    pub fn default_iid_count(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x10)
    }

    /// VA of the events source IID table at offset 0x14.
    ///
    /// The ControlInfo array follows the table directly, so when
    /// [`events_iid_count`](Self::events_iid_count) is 0 this equals
    /// [`controls_va`](Self::controls_va) (every such object in the fixtures).
    #[inline]
    pub fn events_iid_table_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x14)
    }

    /// Event source IID count at offset 0x18.
    ///
    /// Number of event source interfaces: 1 for an object that declares
    /// `Event`s (`calls` `Counter`, `events` `Source`, `types` `Kinds`,
    /// `forms` `Gauge`) and for the UserControl `dispid` `Dial`, 0 otherwise.
    #[inline]
    pub fn events_iid_count(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x18)
    }

    /// VA of the default IID VA-pointer table at offset 0x1C.
    ///
    /// Array of [`default_iid_count`](Self::default_iid_count) dword VAs,
    /// each pointing to a 16-byte IID. This is the default dispatch
    /// interface IID for COM QueryInterface resolution.
    #[inline]
    pub fn default_iid_table_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x1C)
    }

    /// Number of [`ControlInfo`](super::control::ControlInfo) entries at offset 0x20.
    ///
    /// For a form, its controls plus the form itself (`controls` `Form1`,
    /// two controls: 3). For a class, 1, plus 1 per `Implements` or
    /// `WithEvents` member (`calls` `Square`, `events` `Ring` and
    /// `Listener`: 2).
    #[inline]
    pub fn control_count(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x20)
    }

    /// VA of the [`ControlInfo`](super::control::ControlInfo) array at offset 0x24.
    ///
    /// Use [`ControlIterator`](super::control::ControlIterator) to iterate
    /// [`control_count`](Self::control_count) entries.
    #[inline]
    pub fn controls_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x24)
    }

    /// Method link count at offset 0x28.
    ///
    /// The number of the object's own vtable slots, which the runtime copies
    /// from [`method_link_table_va`](Self::method_link_table_va) into the
    /// vtable (see [`OptionalObjectInfo`]). Equal to
    /// [`ObjectInfo::method_count`] in the fixtures, plus 3 in a class with
    /// an `Implements` or `WithEvents` member (`calls` `Square`: 3 methods,
    /// 6 links; `events` `Ring` 25 and 28, `Listener` 5 and 8). `data`
    /// `Item`, whose only member is `Public Name As String`, has 0 methods
    /// and 2 links.
    #[inline]
    pub fn method_link_count(&self) -> Result<u16, Error> {
        read_u16_le(self.bytes, 0x28)
    }

    /// Number of inherited vtable slots at offset 0x2A.
    ///
    /// The vtable slots between `IDispatch`'s and the object's own method
    /// links: the runtime fills this many with pointers to its own stubs
    /// (`0x66102ABC + 8 * i`) after the 7 `IUnknown`/`IDispatch` slots and
    /// before the [`method_link_count`](Self::method_link_count) slots
    /// copied from the method link table (MSVBVM60 6.00.8176 `0x66013976`),
    /// and sizes the vtable copy with the sum of the two (`0x6603F516`).
    /// For a designer object they are its designer's built-in interface
    /// and its 256 control getters, `(cbSizeVft + 0x400 - 0x1C) / 4`: 439
    /// for every Form and MDIForm, 482 for every UserControl, 469 for a
    /// UserDocument and 445 for a PropertyPage in `tests/fixtures`; 0 for
    /// every class. It is not a P-Code method count.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying bytes cannot be read.
    #[inline]
    pub fn inherited_vtable_slots(&self) -> Result<u16, Error> {
        read_u16_le(self.bytes, 0x2A)
    }

    /// Size in bytes of the block at
    /// [`basic_class_object_va`](Self::basic_class_object_va):
    /// `0x28 + 4 * (inherited_vtable_slots + method_link_count)`.
    ///
    /// For all 30 pairs of adjacent blocks in `tests/fixtures` the next block
    /// starts there (`events` `Ring`, 28 links: 0x004044B4 + 0x98 =
    /// `Source`'s 0x0040454C; `dispid` `Host`, a form, 439 + 5 links:
    /// 0x00404418 + 0x718 = `Dial`'s 0x00404B30).
    ///
    /// # Errors
    ///
    /// Returns an error if either count cannot be read.
    pub fn basic_class_object_size(&self) -> Result<u32, Error> {
        // Two u16 counts: the sum times 4 plus 0x28 cannot overflow a u32.
        let slots = u32::from(self.inherited_vtable_slots()?)
            .saturating_add(u32::from(self.method_link_count()?));
        Ok(slots.saturating_mul(4).saturating_add(0x28))
    }

    /// Initialize event offset at 0x2C.
    ///
    /// - **Classes**: 0x0C
    /// - **Forms and UserControls**: 0x68
    ///
    /// The same in every object of a kind in the fixtures, with or
    /// without an `Initialize` handler. It does not index the object's own
    /// method link slots (see [`OptionalObjectInfo`]); what it indexes is
    /// unconfirmed.
    #[inline]
    pub fn initialize_event_offset(&self) -> Result<u16, Error> {
        read_u16_le(self.bytes, 0x2C)
    }

    /// Terminate event offset at 0x2E.
    ///
    /// `initialize_event_offset + 4` in every fixture.
    #[inline]
    pub fn terminate_event_offset(&self) -> Result<u16, Error> {
        read_u16_le(self.bytes, 0x2E)
    }

    /// VA of the method link table at offset 0x30.
    ///
    /// [`method_link_count`](Self::method_link_count) dwords that the
    /// runtime copies into the object's vtable (MSVBVM60 6.00.8176
    /// `0x660139D5`).
    #[inline]
    pub fn method_link_table_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x30)
    }

    /// Per-object vtable block VA at offset 0x34.
    ///
    /// Points to compiler-allocated .data section space (zeroed on disk).
    /// At load MSVBVM60 fills it (6.00.8176 `0x66013971`):
    ///
    /// - `+0x08`: the `ObjectInfo` VA
    /// - `+0x0C`: the vtable, 7 `IUnknown`/`IDispatch` slots, then
    ///   [`inherited_vtable_slots`](Self::inherited_vtable_slots) runtime
    ///   stub slots, then [`method_link_count`](Self::method_link_count)
    ///   slots copied from the method link table
    ///
    /// Its size is [`basic_class_object_size`](Self::basic_class_object_size).
    #[inline]
    pub fn basic_class_object_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x34)
    }

    /// Reserved field at offset 0x38 (0 in every fixture).
    #[inline]
    pub fn null_38(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x38)
    }

    /// Linker-internal field at offset 0x3C.
    ///
    /// Non-zero in every fixture, and an address outside the PE image
    /// (e.g. 0x0204DF10 in `calls`). [`ObjectInfo`] +0x30 holds values of
    /// the same kind.
    #[inline]
    pub fn field_3c(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x3C)
    }

    /// Resolves the object's CLSID from [`object_clsid_va`](Self::object_clsid_va).
    pub fn resolve_clsid(&self, map: &AddressMap<'_>) -> Option<Guid> {
        let va = self.object_clsid_va().ok()?;
        if va == 0 {
            return None;
        }
        let data = map.slice_from_va(va, 16).ok()?;
        Guid::from_bytes(data)
    }

    /// Returns an iterator over GUIDs from the GUI GUID table.
    ///
    /// Yields [`gui_guids_count`](Self::gui_guids_count) GUIDs. Each table
    /// entry is a VA pointer to a 16-byte GUID. These GUIDs correspond to
    /// the [`GuiTableEntry`](super::guitable::GuiTableEntry) entries for
    /// this object.
    pub fn gui_guids<'b>(&self, map: &'b AddressMap<'_>) -> GuidTableIter<'b> {
        GuidTableIter::new(
            map,
            self.gui_guid_table_va().unwrap_or(0),
            self.gui_guids_count().unwrap_or(0),
        )
    }

    /// Returns an iterator over default dispatch interface IIDs.
    ///
    /// Yields [`default_iid_count`](Self::default_iid_count) IIDs. These
    /// are the default COM dispatch interface GUIDs used for
    /// `QueryInterface` resolution.
    pub fn default_iids<'b>(&self, map: &'b AddressMap<'_>) -> GuidTableIter<'b> {
        GuidTableIter::new(
            map,
            self.default_iid_table_va().unwrap_or(0),
            self.default_iid_count().unwrap_or(0),
        )
    }

    /// Returns an iterator over event source interface IIDs.
    ///
    /// Yields [`events_iid_count`](Self::events_iid_count) IIDs. Empty
    /// for objects without custom event sources.
    pub fn events_iids<'b>(&self, map: &'b AddressMap<'_>) -> GuidTableIter<'b> {
        GuidTableIter::new(
            map,
            self.events_iid_table_va().unwrap_or(0),
            self.events_iid_count().unwrap_or(0),
        )
    }

    /// Returns one iterator over all typed IID tables on this object.
    ///
    /// Each yielded item carries an [`IidKind`] discriminator and the
    /// resolved GUID. Entries with unresolvable VAs are skipped with the
    /// same fail-soft behavior as [`gui_guids`](Self::gui_guids),
    /// [`default_iids`](Self::default_iids), and
    /// [`events_iids`](Self::events_iids).
    pub fn typed_iids(&self, map: &AddressMap<'_>) -> TypedIidIter {
        let mut items = Vec::new();
        items.extend(self.gui_guids(map).map(|(_, guid)| (IidKind::Gui, guid)));
        items.extend(
            self.default_iids(map)
                .map(|(_, guid)| (IidKind::Default, guid)),
        );
        items.extend(
            self.events_iids(map)
                .map(|(_, guid)| (IidKind::Events, guid)),
        );
        TypedIidIter {
            inner: items.into_iter(),
        }
    }
}

/// Discriminator for IID tables exposed by [`OptionalObjectInfo`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IidKind {
    /// GUI GUID table entry.
    Gui,
    /// Default dispatch interface IID.
    Default,
    /// Event source interface IID.
    Events,
}

impl IidKind {
    /// Returns the stable persistence string for this IID kind.
    ///
    /// These strings are part of the public API contract and are suitable
    /// for database storage: `"gui"`, `"default"`, and `"events"`.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Gui => "gui",
            Self::Default => "default",
            Self::Events => "events",
        }
    }
}

impl fmt::Display for IidKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Iterator over typed IIDs from all [`OptionalObjectInfo`] IID tables.
#[must_use = "iterators are lazy and do nothing unless consumed"]
pub struct TypedIidIter {
    /// Pre-collected typed IID entries.
    inner: std::vec::IntoIter<(IidKind, Guid)>,
}

impl Iterator for TypedIidIter {
    /// Yields `(kind, guid)` pairs.
    type Item = (IidKind, Guid);

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

/// Iterator over a VA-pointer GUID table.
///
/// Each table entry is a 4-byte VA pointing to a 16-byte GUID. The
/// iterator resolves each VA lazily, yielding `(guid_va, Guid)` pairs.
/// Entries with unresolvable VAs are silently skipped.
///
/// Created by [`OptionalObjectInfo::gui_guids`],
/// [`OptionalObjectInfo::default_iids`], and
/// [`OptionalObjectInfo::events_iids`].
#[must_use = "iterators are lazy and do nothing unless consumed"]
pub struct GuidTableIter<'a> {
    map: &'a AddressMap<'a>,
    /// Pre-read pointer table data (4 bytes per entry).
    ptr_data: &'a [u8],
    index: u32,
    count: u32,
}

impl<'a> GuidTableIter<'a> {
    /// Creates a new GUID table iterator.
    pub fn new(map: &'a AddressMap<'a>, table_va: u32, count: u32) -> Self {
        let ptr_data = if table_va != 0 && count > 0 {
            let ptr_size = (count as usize).saturating_mul(4);
            map.slice_from_va(table_va, ptr_size).unwrap_or(&[])
        } else {
            &[]
        };
        Self {
            map,
            ptr_data,
            index: 0,
            count,
        }
    }
}

impl<'a> Iterator for GuidTableIter<'a> {
    /// Yields `(guid_va, Guid)` pairs - the VA of the GUID data and
    /// the parsed 16-byte GUID.
    type Item = (u32, Guid);

    fn next(&mut self) -> Option<Self::Item> {
        while self.index < self.count {
            let i = self.index as usize;
            self.index = self.index.saturating_add(1);

            let offset = i.checked_mul(4)?;
            let end = offset.checked_add(4)?;
            let chunk = self.ptr_data.get(offset..end)?;
            let guid_va = u32::from_le_bytes(<[u8; 4]>::try_from(chunk).ok()?);
            if guid_va == 0 {
                continue;
            }

            if let Ok(guid_data) = self.map.slice_from_va(guid_va, 16)
                && let Some(guid) = Guid::from_bytes(guid_data)
            {
                return Some((guid_va, guid));
            }
        }
        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.count.saturating_sub(self.index) as usize;
        (0, Some(remaining))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_public_object_descriptor_parse() {
        let mut data = vec![0u8; PublicObjectDescriptor::SIZE];
        data[0x1C..0x20].copy_from_slice(&10u32.to_le_bytes()); // method_count
        // fObjectType = 0x00118003 (class module, as seen in real binaries)
        data[0x28..0x2C].copy_from_slice(&0x00118003u32.to_le_bytes());
        let desc = PublicObjectDescriptor::parse(&data).unwrap();
        assert_eq!(desc.method_count().unwrap(), 10);
        assert!(desc.has_optional_info());
        assert!(desc.is_class());
        assert!(!desc.is_form());
        assert!(!desc.is_module());
    }

    #[test]
    fn test_object_type_detection() {
        let mut data = vec![0u8; PublicObjectDescriptor::SIZE];

        // Module: low byte 0x01
        data[0x28..0x2C].copy_from_slice(&0x00018001u32.to_le_bytes());
        let desc = PublicObjectDescriptor::parse(&data).unwrap();
        assert!(desc.is_module());
        assert!(!desc.is_class());
        assert!(!desc.is_form());

        // Form: low byte 0x83
        data[0x28..0x2C].copy_from_slice(&0x00018083u32.to_le_bytes());
        let desc = PublicObjectDescriptor::parse(&data).unwrap();
        assert!(desc.is_form());
        assert!(!desc.is_class());
        assert!(!desc.is_module());

        // Class: low byte 0x03
        data[0x28..0x2C].copy_from_slice(&0x00118003u32.to_le_bytes());
        let desc = PublicObjectDescriptor::parse(&data).unwrap();
        assert!(desc.is_class());
        assert!(!desc.is_form());
        assert!(!desc.is_module());
    }

    #[test]
    fn test_public_object_descriptor_too_short() {
        let data = vec![0u8; PublicObjectDescriptor::SIZE - 1];
        assert!(matches!(
            PublicObjectDescriptor::parse(&data),
            Err(Error::TooShort { .. })
        ));
    }

    #[test]
    fn test_object_info_parse() {
        let mut data = vec![0u8; ObjectInfo::SIZE];
        data[0x20..0x22].copy_from_slice(&5u16.to_le_bytes()); // method_count
        data[0x24..0x28].copy_from_slice(&0x00404000u32.to_le_bytes()); // methods_va
        let info = ObjectInfo::parse(&data).unwrap();
        assert_eq!(info.method_count().unwrap(), 5);
        assert_eq!(info.methods_va().unwrap(), 0x00404000);
    }

    #[test]
    fn test_object_info_method_table_requires_a_count() {
        let mut data = vec![0u8; ObjectInfo::SIZE];
        data[0x24..0x28].copy_from_slice(&0x00404000u32.to_le_bytes()); // methods_va
        data[0x34..0x38].copy_from_slice(&0x00405000u32.to_le_bytes()); // constants_va

        // A zero count leaves the table pointer uninitialized.
        assert!(
            !ObjectInfo::parse(&data)
                .unwrap()
                .has_method_table()
                .unwrap()
        );

        data[0x20..0x22].copy_from_slice(&3u16.to_le_bytes()); // method_count
        assert!(
            ObjectInfo::parse(&data)
                .unwrap()
                .has_method_table()
                .unwrap()
        );

        // An empty pool's pointer lands on the method table (an interface
        // class whose methods have no constants: events fixture `Measure`).
        data[0x24..0x28].copy_from_slice(&0x00405000u32.to_le_bytes());
        assert!(
            ObjectInfo::parse(&data)
                .unwrap()
                .has_method_table()
                .unwrap()
        );

        // With entries in the pool, the "table" at the pool is the pool.
        data[0x28..0x2A].copy_from_slice(&2u16.to_le_bytes()); // constants_count
        assert!(
            !ObjectInfo::parse(&data)
                .unwrap()
                .has_method_table()
                .unwrap()
        );
    }

    #[test]
    fn test_object_info_too_short() {
        let data = vec![0u8; ObjectInfo::SIZE - 1];
        assert!(matches!(
            ObjectInfo::parse(&data),
            Err(Error::TooShort { .. })
        ));
    }

    #[test]
    fn test_optional_object_info_parse() {
        let mut data = vec![0u8; OptionalObjectInfo::SIZE];
        data[0x20..0x24].copy_from_slice(&7u32.to_le_bytes()); // control_count
        data[0x28..0x2A].copy_from_slice(&5u16.to_le_bytes()); // method_link_count
        data[0x2A..0x2C].copy_from_slice(&439u16.to_le_bytes()); // a form's inherited slots
        let opt = OptionalObjectInfo::parse(&data).unwrap();
        assert_eq!(opt.inherited_vtable_slots().unwrap(), 439);
        assert_eq!(opt.control_count().unwrap(), 7);
        // dispid's Host: 0x28 + 4 * (439 + 5).
        assert_eq!(opt.basic_class_object_size().unwrap(), 0x718);
    }

    #[test]
    fn test_optional_object_info_too_short() {
        let data = vec![0u8; OptionalObjectInfo::SIZE - 1];
        assert!(matches!(
            OptionalObjectInfo::parse(&data),
            Err(Error::TooShort { .. })
        ));
    }
}
