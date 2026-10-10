//! ProjectInfo2 structure parser, and the event-source records that follow it.
//!
//! The `ProjectInfo2` structure is pointed to by `ObjectTable.lpProjectInfo2`
//! (+0x08). Its 0x28-byte header points to the per-object
//! `PrivateObjectDescriptor` array.
//!
//! # Layout
//!
//! The compiler places the 0x28-byte header at the end of the data that
//! precedes the first procedure's P-Code. After it, up to the P-Code, come
//! 12-byte event-source records interleaved with null-terminated name
//! strings (each padded to a 4-byte boundary) and the descriptors some
//! member types carry. There is no count: in `dispid` and `forms` records
//! follow name strings. Projects with only standard modules have nothing
//! after the header. [`ProjectInfo2Iter`] walks the region and tells
//! records from names (see [`ProjectInfo2Item`]).
//!
//! The names are those of the objects' procedure parameters, event
//! parameters, public variables and implemented interfaces, not property
//! or method names. [`VbProject::name_references`](crate::VbProject::name_references)
//! attributes each one through the pointers that refer to it.
//!
//! ## Header (0x28 bytes)
//!
//! | Offset | Size | Field |
//! |--------|------|-------|
//! | 0x00 | 4 | Reserved (always 0) |
//! | 0x04 | 4 | `lpObjectTable` (back-pointer) |
//! | 0x08 | 4 | Reserved (always 0xFFFFFFFF) |
//! | 0x0C | 4 | Reserved (always 0) |
//! | 0x10 | 4 | `lpObjectDescs` (PrivateObjectDescriptor VA array) |
//! | 0x14 | 12 | Reserved (always 0) |
//! | 0x20 | 4 | Reserved (always 0xFFFFFFFF) |
//! | 0x24 | 4 | Reserved (always 0) |
//!
//! ## Event-source records (0x0C bytes each)
//!
//! One record per event source of the project's objects: the object's own
//! events (name `"Class"`, `"Form"` or `"UserControl"`) and its controls
//! (named by instance, e.g. `"Text1"`). The GUID is the source's event
//! interface IID, not a CLSID: in `VB6.OLB` the fixtures' GUIDs are
//! `TextBoxEvents`, `LabelEvents`, `CommandButtonEvents`, `FormEvents` and
//! `UserControlEvents`, and `"Class"` is `IClassModuleEvt` of the runtime's
//! `VBRUN` library.
//!
//! | Offset | Size | Field |
//! |--------|------|-------|
//! | 0x00 | 4 | `lpTypeLib` (the event interface's [`TypeLibRef`]; 0 for a control array or the project's own UserControl) |
//! | 0x04 | 4 | `lpGuidData` (16-byte event IID + source name string) |
//! | 0x08 | 4 | `lpDispatchSlot` (.data section slot, one per record) |

use std::str;

use crate::{
    addressmap::AddressMap,
    error::Error,
    util::{read_cstr, read_u32_le},
    vb::{control::Guid, typeref::TypeLibRef},
};

/// View over a ProjectInfo2 header (0x28 bytes).
#[derive(Clone, Copy, Debug)]
pub struct ProjectInfo2<'a> {
    bytes: &'a [u8],
}

impl<'a> ProjectInfo2<'a> {
    /// Header size in bytes.
    pub const HEADER_SIZE: usize = 0x28;

    /// Size of each control type entry.
    pub const ENTRY_SIZE: usize = 0x0C;

    /// Parses the ProjectInfo2 header from the given byte slice.
    ///
    /// # Errors
    ///
    /// Returns [`Error::TooShort`] if `data.len() < 0x28`.
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        if data.len() < Self::HEADER_SIZE {
            return Err(Error::TooShort {
                expected: Self::HEADER_SIZE,
                actual: data.len(),
                context: "ProjectInfo2",
            });
        }
        let bytes = data.get(..Self::HEADER_SIZE).ok_or(Error::TooShort {
            expected: Self::HEADER_SIZE,
            actual: data.len(),
            context: "ProjectInfo2",
        })?;
        Ok(Self { bytes })
    }

    /// ObjectTable back-pointer at offset 0x04.
    #[inline]
    pub fn object_table_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x04)
    }

    /// VA of the PrivateObjectDescriptor pointer array at offset 0x10.
    ///
    /// Contains one DWORD per object (total_objects entries), in object
    /// table order. Each entry equals that object's
    /// `ObjectInfo.lpPrivateObject`: a PrivateObjectDescriptor VA, or
    /// 0xFFFFFFFF for a standard module; except that a module of an
    /// ActiveX DLL declaring `Public Type`s (`udts` `Mod1`) holds an
    /// address outside the image, a compiler address left unrelocated.
    #[inline]
    pub fn object_descs_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x10)
    }

    /// Reads the [`object_descs_va`](Self::object_descs_va) array's `count`
    /// entries ([`ObjectTable::total_objects`](super::objecttable::ObjectTable::total_objects)):
    /// per object, its PrivateObjectDescriptor VA, or for a standard module
    /// 0xFFFFFFFF or an address outside the image.
    ///
    /// # Errors
    ///
    /// Returns an error if the array VA cannot be read or the array is not
    /// mapped.
    pub fn object_descs(&self, map: &AddressMap<'_>, count: u16) -> Result<Vec<u32>, Error> {
        let len = usize::from(count).saturating_mul(4);
        let data = map.slice_from_va(self.object_descs_va()?, len)?;
        (0..usize::from(count))
            .map(|i| read_u32_le(data, i.saturating_mul(4)))
            .collect()
    }
}

/// A single event-source record (0x0C bytes).
#[derive(Debug, Clone, Copy)]
pub struct ControlTypeEntry {
    /// VA of the [`TypeLibRef`] of the event interface's library; 0 for a
    /// control array and for an instance of the project's own UserControl.
    pub typelib_va: u32,
    /// VA of the GUID data: 16-byte event interface IID followed by the
    /// null-terminated source name (`"Class"`, `"Form"`, `"UserControl"` or
    /// a control instance name).
    pub guid_data_va: u32,
    /// VA of this record's slot in the .data section.
    pub dispatch_slot_va: u32,
}

impl ControlTypeEntry {
    /// Reads the 16-byte event interface IID from the GUID data.
    pub fn control_guid<'a>(&self, map: &AddressMap<'a>) -> Option<Guid> {
        let data = map.slice_from_va(self.guid_data_va, 16).ok()?;
        Guid::from_bytes(data)
    }

    /// Reads the source name string (after the 16-byte GUID).
    pub fn control_name<'a>(&self, map: &AddressMap<'a>) -> Option<&'a str> {
        let data = map
            .slice_from_va(self.guid_data_va.wrapping_add(16), 64)
            .ok()?;
        let name = read_cstr(data, 0).ok()?;
        if name.is_empty() {
            return None;
        }
        str::from_utf8(name).ok()
    }

    /// Reads the [`TypeLibRef`] of the event interface's library, `None`
    /// for a record with none.
    pub fn typelib<'a>(&self, map: &AddressMap<'a>) -> Option<TypeLibRef<'a>> {
        TypeLibRef::at(map, self.typelib_va).ok()
    }
}

/// One item of the region after a ProjectInfo2 header.
#[derive(Debug, Clone, Copy)]
pub enum ProjectInfo2Item<'a> {
    /// A 12-byte event-source record.
    Record(ControlTypeEntry),
    /// A null-terminated name string, padded to a 4-byte boundary.
    Name(&'a str),
}

/// Walks the region after a ProjectInfo2 header: the event-source records
/// and the name strings between them, in file order.
///
/// At each 4-byte-aligned position it reads either
///
/// - a record: its second dword is the VA of a 16-byte GUID followed by an
///   identifier (the source name), its first dword 0 or a VA in the image,
///   and its third a VA inside a section (the records' slots are in the
///   zero-filled `.data`), or
/// - a name: an identifier (ASCII letter, then letters, digits or `_`),
///   its terminating NUL and NUL padding to the next 4-byte boundary,
///
/// and stops at the first position that is neither: the P-Code of the
/// first procedure, or the end of the file. Every step advances at least 4
/// bytes. `forms` yields the records `Text1`, `Label1`, `Form`, `Command1`
/// (a control array: no type library), `Gauge1` (the project's own
/// UserControl), the names `start` and `s`, the record `UserControl` and
/// the names `v` and `NewValue`; `data` the record `Class` and the name
/// `Name`.
#[must_use = "iterators are lazy and do nothing unless consumed"]
pub struct ProjectInfo2Iter<'a> {
    map: &'a AddressMap<'a>,
    /// VA of the next item.
    va: u32,
    /// Set once the walk has met something that is neither item.
    done: bool,
}

impl<'a> ProjectInfo2Iter<'a> {
    /// Creates a walk of the region after the ProjectInfo2 header at
    /// `pi2_va`.
    pub fn new(map: &'a AddressMap<'a>, pi2_va: u32) -> Self {
        Self {
            map,
            va: pi2_va.wrapping_add(ProjectInfo2::HEADER_SIZE as u32),
            done: false,
        }
    }

    /// Returns the VA of the next item: read before an item is yielded, its
    /// start; after the walk ends, the end of the last item.
    #[inline]
    pub fn position(&self) -> u32 {
        self.va
    }

    /// Reads a record at the current position, if there is one.
    fn record(&self, data: &[u8]) -> Option<ControlTypeEntry> {
        let entry = ControlTypeEntry {
            typelib_va: read_u32_le(data, 0).ok()?,
            guid_data_va: read_u32_le(data, 4).ok()?,
            dispatch_slot_va: read_u32_le(data, 8).ok()?,
        };
        let typelib_ok = entry.typelib_va == 0 || self.map.is_va_in_image(entry.typelib_va);
        // The slot is in the zero-filled .data: inside a section, not
        // necessarily file-backed.
        let slot_ok = matches!(
            self.map.va_to_offset(entry.dispatch_slot_va),
            Ok(_) | Err(Error::RvaInBssRegion { .. })
        );
        let name = self
            .map
            .slice_from_va(entry.guid_data_va.checked_add(16)?, 1)
            .ok()
            .and_then(|tail| identifier(tail));
        (typelib_ok && slot_ok && name.is_some()).then_some(entry)
    }
}

impl<'a> Iterator for ProjectInfo2Iter<'a> {
    type Item = ProjectInfo2Item<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        let Ok(data) = self.map.slice_from_va(self.va, 4) else {
            self.done = true;
            return None;
        };
        if let Some(entry) = data
            .get(..ProjectInfo2::ENTRY_SIZE)
            .and_then(|d| self.record(d))
        {
            self.va = self.va.wrapping_add(ProjectInfo2::ENTRY_SIZE as u32);
            return Some(ProjectInfo2Item::Record(entry));
        }
        let Some(name) = identifier(data) else {
            self.done = true;
            return None;
        };
        // The name, its NUL, then NUL padding to a 4-byte boundary.
        let end = name.len().saturating_add(4) & !3;
        if data
            .get(name.len()..end)
            .is_none_or(|pad| pad.iter().any(|&b| b != 0))
        {
            self.done = true;
            return None;
        }
        self.va = self.va.wrapping_add(u32::try_from(end).unwrap_or(u32::MAX));
        Some(ProjectInfo2Item::Name(name))
    }
}

/// Iterates the event-source records that follow a ProjectInfo2 header.
///
/// The [`ProjectInfo2Item::Record`]s of a [`ProjectInfo2Iter`], skipping the
/// name strings between them: `forms` yields its 6 records and `dispid` its
/// 4 (`Class`, `Form`, `Dial1`, `UserControl`).
#[must_use = "iterators are lazy and do nothing unless consumed"]
pub struct ControlTypeIter<'a> {
    inner: ProjectInfo2Iter<'a>,
}

impl<'a> ControlTypeIter<'a> {
    /// Creates a new iterator over control type entries.
    ///
    /// `pi2_va` is the VA of the ProjectInfo2 header.
    pub fn new(map: &'a AddressMap<'a>, pi2_va: u32) -> Self {
        Self {
            inner: ProjectInfo2Iter::new(map, pi2_va),
        }
    }
}

impl<'a> Iterator for ControlTypeIter<'a> {
    type Item = ControlTypeEntry;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.find_map(|item| match item {
            ProjectInfo2Item::Record(entry) => Some(entry),
            ProjectInfo2Item::Name(_) => None,
        })
    }
}

/// Collects the name strings of the region after the ProjectInfo2 header at
/// `pi2_va` (the [`ProjectInfo2Item::Name`]s of a [`ProjectInfo2Iter`]).
///
/// The strings are the names of the objects' procedure parameters, event
/// parameters, public variables and implemented interfaces, one string per
/// spelling (`calls`: `v`, `o`, `factor`, ... parameters, `Value` the
/// parameter of `Counter`'s `Event Changed`, `Shape` `Square`'s
/// `Implements`; `members`: `Holder`'s variables `B` ... `Auto`). Property
/// and method names are not among them. The walk ends at the first data
/// that is neither a record nor a name: before the P-Code (`data` yields
/// `Name` alone), or at a member type's descriptor (`members` stops after
/// `Auto`, before `Count`, `IFirst`, `Value`, `Amount`, ...).
///
/// For every name with its owner, use
/// [`VbProject::name_references`](crate::VbProject::name_references).
pub fn read_name_strings<'a>(map: &'a AddressMap<'a>, pi2_va: u32) -> Vec<&'a str> {
    ProjectInfo2Iter::new(map, pi2_va)
        .filter_map(|item| match item {
            ProjectInfo2Item::Name(name) => Some(name),
            ProjectInfo2Item::Record(_) => None,
        })
        .collect()
}

/// The identifier `data` starts with, if it is one terminated by a NUL: an
/// ASCII letter, then letters, digits or `_`, at most 255 of them (VB's
/// limit), so the search for the NUL stays short.
fn identifier(data: &[u8]) -> Option<&str> {
    let window = data.get(..data.len().min(256))?;
    let len = window.iter().position(|&b| b == 0)?;
    let name = data.get(..len)?;
    let (first, rest) = name.split_first()?;
    (first.is_ascii_alphabetic() && rest.iter().all(|&b| b.is_ascii_alphanumeric() || b == b'_'))
        .then(|| str::from_utf8(name).ok())
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_header() {
        let mut data = vec![0u8; ProjectInfo2::HEADER_SIZE];
        data[0x04..0x08].copy_from_slice(&0x00402000u32.to_le_bytes());
        data[0x08..0x0C].copy_from_slice(&0xFFFFFFFFu32.to_le_bytes());
        data[0x10..0x14].copy_from_slice(&0x00405000u32.to_le_bytes());
        let pi2 = ProjectInfo2::parse(&data).unwrap();
        assert_eq!(pi2.object_table_va().unwrap(), 0x00402000);
        assert_eq!(pi2.object_descs_va().unwrap(), 0x00405000);
    }

    #[test]
    fn test_parse_too_short() {
        let data = vec![0u8; ProjectInfo2::HEADER_SIZE - 1];
        assert!(matches!(
            ProjectInfo2::parse(&data),
            Err(Error::TooShort { .. })
        ));
    }
}
