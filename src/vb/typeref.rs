//! References to types defined in type libraries: the libraries
//! themselves, their interfaces and their records (user-defined types).
//!
//! A type-list entry that names such a type ([`ArgType`](super::functype::ArgType)
//! codes 0x1C, 0x1D for an interface, 0x14 for a record) is followed by the
//! VA of an [`InterfaceRef`] or a [`RecordRef`]; an event-source record of
//! the ProjectInfo2 region ([`ControlTypeEntry`](super::projectinfo2::ControlTypeEntry))
//! points to the [`TypeLibRef`] of its event interface's library.
//!
//! In `tests/fixtures/typerefs`, `Shapes` declares `Public Items As
//! Collection` (an [`InterfaceRef`] to VBA's `_Collection`), `Public Font As
//! StdFont` (stdole's `Font`), `Public Unk As IUnknown` (code 0x1C, stdole's
//! `IUnknown`) and a parameter of its own `Public Type Point` (a
//! [`RecordRef`] to the project's library).

use std::str;

use crate::{
    addressmap::AddressMap,
    error::Error,
    util::{read_cstr, read_u16_le, read_u32_le},
    vb::control::Guid,
};

/// Reads the 16-byte GUID at `va`.
fn guid_at(map: &AddressMap<'_>, va: u32) -> Option<Guid> {
    Guid::from_bytes(map.slice_from_va(va, 16).ok()?)
}

/// Reads the null-terminated ANSI string at `va`, `None` for VA 0.
fn str_at<'a>(map: &AddressMap<'a>, va: u32) -> Option<&'a str> {
    if va == 0 {
        return None;
    }
    let off = map.va_to_offset(va).ok()?;
    str::from_utf8(read_cstr(map.file(), off).ok()?).ok()
}

/// A reference to a type library (0x28 bytes).
///
/// One per library the project uses a type of; every reference to the
/// library's types points to it. In the fixtures: `VB` (`VB6.OLB`, GUID
/// `FCFB3D2E-A0FA-1068-A738-08002B3371B5`, version 6.0, LCID 9, path
/// `C:\VB98\VB6.OLB`), `VBRUN` (GUID `EA544A21-C82D-11D1-A3E4-00A0C90AEA82`,
/// no path), `VBA` (`VBA6.DLL`), `stdole` (version 2.0, LCID 0) and the
/// libraries of hosted controls (`MSComctlLib` 2.0, `MSMask` 1.1, LCID 0).
///
/// | Offset | Size | Field |
/// |--------|------|-------|
/// | 0x00 | 4 | `lpGuid` (VA of the library's 16-byte GUID) |
/// | 0x04 | 4 | Development-environment field, 0 on disk |
/// | 0x08 | 2 | Major version |
/// | 0x0A | 2 | Minor version |
/// | 0x0C | 4 | LCID |
/// | 0x10 | 4 | `lpPath` (null-terminated path of the library file, or 0) |
/// | 0x14 | 4 | `lpName` (the library name, e.g. `"VB"`, `"VBRUN"`) |
/// | 0x18 | 4 | `lpDataSlot` (.data section VA, just below the slots of the references that use it) |
/// | 0x1C | 4 | Development-environment field, 0 on disk |
/// | 0x20 | 4 | Flags ([`flags`](Self::flags)): 0 in every fixture |
/// | 0x24 | 4 | A compiler heap address copied unrelocated, the same in every reference of a binary (`typerefs`: 0x02085A90) |
///
/// The runtime loads the library by GUID, version and LCID, else from the
/// path, and checks the loaded library's version and LCID, and its GUID
/// unless flag bits 0-1 are set (MSVBVM60 6.00.8176 `0x660F291E`); it
/// stores the `ITypeLib` at the data slot. It reads none of +0x04, +0x1C
/// or +0x24.
#[derive(Clone, Copy, Debug)]
pub struct TypeLibRef<'a> {
    bytes: &'a [u8],
}

impl<'a> TypeLibRef<'a> {
    /// Total size of the structure in bytes.
    pub const SIZE: usize = 0x28;

    /// Parses a type library reference from the given byte slice.
    ///
    /// # Errors
    ///
    /// Returns [`Error::TooShort`] if `data.len() < 0x28`.
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        let bytes = data.get(..Self::SIZE).ok_or(Error::TooShort {
            expected: Self::SIZE,
            actual: data.len(),
            context: "TypeLibRef",
        })?;
        Ok(Self { bytes })
    }

    /// Reads the type library reference at `va`.
    ///
    /// # Errors
    ///
    /// Returns an error if `va` is not mapped or too few bytes follow it.
    pub fn at(map: &AddressMap<'a>, va: u32) -> Result<Self, Error> {
        Self::parse(map.slice_from_va(va, Self::SIZE)?)
    }

    /// VA of the library's GUID at offset 0x00.
    #[inline]
    pub fn guid_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x00)
    }

    /// Reads the library's GUID.
    pub fn guid(&self, map: &AddressMap<'_>) -> Option<Guid> {
        guid_at(map, self.guid_va().ok()?)
    }

    /// The library's version, `(major, minor)`, at offsets 0x08 and 0x0A.
    ///
    /// # Errors
    ///
    /// Returns an error if the fields cannot be read.
    pub fn version(&self) -> Result<(u16, u16), Error> {
        Ok((
            read_u16_le(self.bytes, 0x08)?,
            read_u16_le(self.bytes, 0x0A)?,
        ))
    }

    /// The library's LCID at offset 0x0C (9 for `VB`, `VBRUN` and `VBA`, 0
    /// for `stdole` and the controls' libraries).
    #[inline]
    pub fn lcid(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x0C)
    }

    /// VA of the library file's path at offset 0x10; 0 for `VBRUN`.
    #[inline]
    pub fn path_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x10)
    }

    /// Reads the library file's path (`"C:\VB98\VB6.OLB"`), `None` when
    /// there is none.
    pub fn path(&self, map: &AddressMap<'a>) -> Option<&'a str> {
        str_at(map, self.path_va().ok()?)
    }

    /// VA of the library name at offset 0x14.
    #[inline]
    pub fn name_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x14)
    }

    /// Reads the library name (`"VB"`, `"VBRUN"`, `"stdole"`).
    pub fn name(&self, map: &AddressMap<'a>) -> Option<&'a str> {
        str_at(map, self.name_va().ok()?)
    }

    /// The `.data` section VA at offset 0x18 (zero-filled on disk), 4 to
    /// 12 bytes below the slots of the references that use the library.
    #[inline]
    pub fn data_slot_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x18)
    }

    /// Flags at offset 0x20: with bit 0 or 1 set the runtime does not check
    /// the loaded library's GUID against [`guid`](Self::guid); the compiler
    /// reads bit 1 as an extended type library. 0 in every fixture.
    #[inline]
    pub fn flags(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x20)
    }
}

/// A reference to an interface of a type library (0x0C bytes): the
/// descriptor an [`ArgType`](super::functype::ArgType) of code 0x1C or 0x1D
/// names.
///
/// | Offset | Size | Field |
/// |--------|------|-------|
/// | 0x00 | 4 | `lpTypeLib` ([`TypeLibRef`] of the interface's library) |
/// | 0x04 | 4 | `lpIid` (VA of the interface's 16-byte IID) |
/// | 0x08 | 4 | `lpDataSlot` (.data section VA, zero-filled on disk) |
///
/// `typerefs`: `Collection` is VBA's `_Collection`
/// (`A4C46780-499F-101B-BB78-00AA00383CBB`), `StdFont` stdole's `Font`,
/// `IUnknown` stdole's `IUnknown` (code 0x1C), `VB.TextBox` VB's `TextBox`
/// (`33AD4EE1-6699-11CF-B70C-00AA0060D393`). One reference serves every use
/// of the interface in the project.
#[derive(Clone, Copy, Debug)]
pub struct InterfaceRef<'a> {
    bytes: &'a [u8],
}

impl<'a> InterfaceRef<'a> {
    /// Size of the structure in bytes.
    pub const SIZE: usize = 0x0C;

    /// Parses an interface reference from the given byte slice.
    ///
    /// # Errors
    ///
    /// Returns [`Error::TooShort`] if `data.len() < 0x0C`.
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        let bytes = data.get(..Self::SIZE).ok_or(Error::TooShort {
            expected: Self::SIZE,
            actual: data.len(),
            context: "InterfaceRef",
        })?;
        Ok(Self { bytes })
    }

    /// Reads the interface reference at `va`.
    ///
    /// # Errors
    ///
    /// Returns an error if `va` is not mapped or too few bytes follow it.
    pub fn at(map: &AddressMap<'a>, va: u32) -> Result<Self, Error> {
        Self::parse(map.slice_from_va(va, Self::SIZE)?)
    }

    /// VA of the interface's library ([`TypeLibRef`]) at offset 0x00.
    #[inline]
    pub fn typelib_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x00)
    }

    /// Reads the interface's library.
    pub fn typelib(&self, map: &AddressMap<'a>) -> Option<TypeLibRef<'a>> {
        TypeLibRef::at(map, self.typelib_va().ok()?).ok()
    }

    /// VA of the interface's IID at offset 0x04.
    #[inline]
    pub fn iid_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x04)
    }

    /// Reads the interface's IID.
    pub fn iid(&self, map: &AddressMap<'_>) -> Option<Guid> {
        guid_at(map, self.iid_va().ok()?)
    }

    /// The `.data` section VA at offset 0x08.
    #[inline]
    pub fn data_slot_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x08)
    }
}

/// A reference to a record (user-defined type) of a type library (0x14
/// bytes): the descriptor an [`ArgType`](super::functype::ArgType) of code
/// 0x14 names.
///
/// | Offset | Size | Field |
/// |--------|------|-------|
/// | 0x00 | 4 | `lpLibId` (VA of the defining library's 16-byte GUID) |
/// | 0x04 | 4 | `lpGuid` (VA of the record's 16-byte GUID) |
/// | 0x08 | 2 | The library's major version |
/// | 0x0A | 2 | The library's minor version |
/// | 0x0C | 4 | The library's LCID |
/// | 0x10 | 4 | `lpDataSlot` (.data section VA, zero-filled on disk) |
///
/// `typerefs`: `Shapes`' `Public Type Point` names the project's own library
/// (`ACCFB576-1C34-4F63-9142-D21358736A52`, the
/// [`ComRegData::project_guid`](super::comreg::ComRegData::project_guid)),
/// version 1.0 (`MajorVer`, `MinorVer`), LCID 0x409.
#[derive(Clone, Copy, Debug)]
pub struct RecordRef<'a> {
    bytes: &'a [u8],
}

impl<'a> RecordRef<'a> {
    /// Size of the structure in bytes.
    pub const SIZE: usize = 0x14;

    /// Parses a record reference from the given byte slice.
    ///
    /// # Errors
    ///
    /// Returns [`Error::TooShort`] if `data.len() < 0x14`.
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        let bytes = data.get(..Self::SIZE).ok_or(Error::TooShort {
            expected: Self::SIZE,
            actual: data.len(),
            context: "RecordRef",
        })?;
        Ok(Self { bytes })
    }

    /// Reads the record reference at `va`.
    ///
    /// # Errors
    ///
    /// Returns an error if `va` is not mapped or too few bytes follow it.
    pub fn at(map: &AddressMap<'a>, va: u32) -> Result<Self, Error> {
        Self::parse(map.slice_from_va(va, Self::SIZE)?)
    }

    /// VA of the defining library's GUID at offset 0x00.
    #[inline]
    pub fn libid_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x00)
    }

    /// Reads the defining library's GUID.
    pub fn libid(&self, map: &AddressMap<'_>) -> Option<Guid> {
        guid_at(map, self.libid_va().ok()?)
    }

    /// VA of the record's GUID at offset 0x04.
    #[inline]
    pub fn guid_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x04)
    }

    /// Reads the record's GUID.
    pub fn guid(&self, map: &AddressMap<'_>) -> Option<Guid> {
        guid_at(map, self.guid_va().ok()?)
    }

    /// The defining library's version, `(major, minor)`, at offsets 0x08
    /// and 0x0A.
    ///
    /// # Errors
    ///
    /// Returns an error if the fields cannot be read.
    pub fn version(&self) -> Result<(u16, u16), Error> {
        Ok((
            read_u16_le(self.bytes, 0x08)?,
            read_u16_le(self.bytes, 0x0A)?,
        ))
    }

    /// The defining library's LCID at offset 0x0C.
    #[inline]
    pub fn lcid(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x0C)
    }

    /// The `.data` section VA at offset 0x10.
    #[inline]
    pub fn data_slot_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x10)
    }
}
