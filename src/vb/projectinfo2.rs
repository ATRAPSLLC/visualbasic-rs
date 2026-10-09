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
//! 12-byte event-source records interleaved with null-terminated parameter
//! name strings (each padded to a 4-byte boundary). There is no count: in
//! `dispid` and `forms` records follow name strings. Projects with only
//! standard modules have nothing after the header. [`ProjectInfo2Iter`]
//! walks the region and tells the two apart (see [`ProjectInfo2Item`]).
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
//! | 0x00 | 4 | `lpInterfaceMetadata` (the type library reference; 0 for a control array or the project's own UserControl) |
//! | 0x04 | 4 | `lpGuidData` (16-byte event IID + source name string) |
//! | 0x08 | 4 | `lpDispatchSlot` (.data section slot, one per record) |

use std::str;

use crate::{
    addressmap::AddressMap,
    error::Error,
    util::{read_cstr, read_u32_le},
    vb::control::Guid,
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
    /// 0xFFFFFFFF for a standard module (every fixture).
    #[inline]
    pub fn object_descs_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x10)
    }
}

/// Type library reference (0x24 bytes) at each entry's `interface_metadata_va`.
///
/// Names the type library that declares the record's event interface.
/// Records of one library share one structure. In the fixtures it is
/// either `VB` (`VB6.OLB`, GUID `FCFB3D2E-A0FA-1068-A738-08002B3371B5`,
/// path `C:\VB98\VB6.OLB`) or `VBRUN` (GUID
/// `EA544A21-C82D-11D1-A3E4-00A0C90AEA82`, no path).
///
/// | Offset | Size | Field |
/// |--------|------|-------|
/// | 0x00 | 4 | `lpTypelibGuid` (VA of the library's 16-byte GUID) |
/// | 0x04 | 4 | Reserved (always 0) |
/// | 0x08 | 4 | Always 6 (both libraries' `MSFT` headers carry version 6) |
/// | 0x0C | 4 | Always 9 (both libraries' `MSFT` headers carry LCID 9) |
/// | 0x10 | 4 | `lpTypelibPath` (null-terminated path string, or 0) |
/// | 0x14 | 4 | `lpNameTable` (the library name, e.g. `"VB"`, `"VBRUN"`) |
/// | 0x18 | 4 | `lpDataSlot` (.data section VA, just below the slots of the records that use it) |
/// | 0x1C | 4 | Reserved (always 0) |
/// | 0x20 | 4 | Reserved (always 0) |
#[derive(Clone, Copy, Debug)]
pub struct InterfaceMetadata<'a> {
    bytes: &'a [u8],
}

impl<'a> InterfaceMetadata<'a> {
    /// Total size of the structure in bytes.
    pub const SIZE: usize = 0x24;

    /// Parses interface metadata from the given byte slice.
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        if data.len() < Self::SIZE {
            return Err(Error::TooShort {
                expected: Self::SIZE,
                actual: data.len(),
                context: "InterfaceMetadata",
            });
        }
        let bytes = data.get(..Self::SIZE).ok_or(Error::TooShort {
            expected: Self::SIZE,
            actual: data.len(),
            context: "InterfaceMetadata",
        })?;
        Ok(Self { bytes })
    }

    /// VA of the library name at offset 0x14.
    ///
    /// Points to the null-terminated library name (`"VB"`, `"VBRUN"`); in
    /// the fixtures this structure itself follows the name, so no method or
    /// property names follow it.
    #[inline]
    pub fn name_table_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x14)
    }

    /// VA of the typelib GUID at offset 0x00.
    #[inline]
    pub fn typelib_guid_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x00)
    }

    /// VA of the typelib path string at offset 0x10.
    ///
    /// Points to a null-terminated ANSI path of the type library file
    /// (`"C:\VB98\VB6.OLB"` in `controls` and `forms`). `0` for the
    /// runtime's own `VBRUN` library.
    ///
    /// Pair with [`typelib_guid_va`](Self::typelib_guid_va) to get the
    /// typelib's CLSID - together they identify which DLL/OCX provides
    /// the type information for this interface (a supply-chain signal
    /// useful for malware triage).
    ///
    /// # Errors
    ///
    /// Returns [`Error::Truncated`] if the backing buffer is shorter than expected.
    #[inline]
    pub fn typelib_path_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x10)
    }

    /// Raw `.data` section VA at offset 0x18.
    ///
    /// Points into the binary's `.data` section (zero on disk), 4 to 12
    /// bytes below the slots of the records that use this library. How the
    /// runtime uses it is not verified.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Truncated`] if the backing buffer is shorter than expected.
    #[inline]
    pub fn data_slot_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x18)
    }

    /// Reads the identifier strings that start at the name table.
    ///
    /// In the fixtures this is the library name alone (`["VB"]`,
    /// `["VBRUN"]`). The scan stops at the first byte sequence that is not
    /// an identifier.
    pub fn dispatch_names(&self, map: &AddressMap<'a>) -> Vec<&'a str> {
        let Ok(va) = self.name_table_va() else {
            return Vec::new();
        };
        if va == 0 {
            return Vec::new();
        }
        let Ok(data) = map.slice_from_va(va, 512) else {
            return Vec::new();
        };
        extract_name_block(data, 0).0
    }

    /// Scans up to 4096 bytes from the name table for runs of identifier
    /// strings.
    ///
    /// Heuristic: the first run is the library name; later runs are
    /// whatever identifier-like strings follow in the image (the scan does
    /// not know where the structure ends). Returns one vector per run.
    pub fn all_dispatch_names(&self, map: &AddressMap<'a>) -> Vec<Vec<&'a str>> {
        let Ok(va) = self.name_table_va() else {
            return Vec::new();
        };
        if va == 0 {
            return Vec::new();
        }
        let Ok(data) = map.slice_from_va(va, 4096) else {
            return Vec::new();
        };
        extract_all_name_blocks(data)
    }
}

/// A single event-source record (0x0C bytes).
#[derive(Debug, Clone, Copy)]
pub struct ControlTypeEntry {
    /// VA of the [`InterfaceMetadata`] type library reference; 0 for a
    /// control array and for an instance of the project's own UserControl.
    pub interface_metadata_va: u32,
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

    /// Parses the interface metadata for this entry.
    pub fn interface_metadata<'a>(&self, map: &'a AddressMap<'a>) -> Option<InterfaceMetadata<'a>> {
        let data = map
            .slice_from_va(self.interface_metadata_va, InterfaceMetadata::SIZE)
            .ok()?;
        InterfaceMetadata::parse(data).ok()
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
/// and the parameter name strings between them, in file order.
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

    /// Reads a record at the current position, if there is one.
    fn record(&self, data: &[u8]) -> Option<ControlTypeEntry> {
        let entry = ControlTypeEntry {
            interface_metadata_va: read_u32_le(data, 0).ok()?,
            guid_data_va: read_u32_le(data, 4).ok()?,
            dispatch_slot_va: read_u32_le(data, 8).ok()?,
        };
        let metadata_ok = entry.interface_metadata_va == 0
            || self.map.is_va_in_image(entry.interface_metadata_va);
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
        (metadata_ok && slot_ok && name.is_some()).then_some(entry)
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
/// The strings are parameter names of the objects' procedures, event
/// handlers included (`calls`: `v`, `o`, `a`, `b`, `factor`, ...; `forms`:
/// `NewValue` of `Gauge1_Changed`), not property names. The walk ends
/// before the P-Code that follows (`data` yields `Name` alone).
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

/// Checks if a byte sequence looks like a VB6 identifier.
fn is_vb_identifier(name: &[u8]) -> bool {
    name.len() >= 2
        && name
            .iter()
            .all(|&b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.')
}

/// Extracts one name block starting at `pos` in `data`.
///
/// A name block is a sequence of null-terminated VB6 identifier strings,
/// each null-padded to 4-byte alignment. The block ends at the first
/// non-identifier byte sequence or a 4+ byte null run.
///
/// Returns `(names, end_pos)` where `end_pos` is the byte offset
/// after the block (including the null terminator).
fn extract_name_block(data: &[u8], start: usize) -> (Vec<&str>, usize) {
    let mut names = Vec::new();
    let mut pos = start;

    while pos < data.len() {
        let Some(&b) = data.get(pos) else { break };
        // Skip null padding
        if b == 0 {
            let tail = data.get(pos..).unwrap_or(&[]);
            let nulls = tail.iter().take_while(|&&b| b == 0).count();
            if nulls >= 4 && !names.is_empty() {
                // End of name block
                return (names, pos.saturating_add(nulls));
            }
            pos = pos.saturating_add(nulls);
            continue;
        }
        let Ok(name) = read_cstr(data, pos) else {
            break;
        };
        if !is_vb_identifier(name) {
            break;
        }
        if let Ok(s) = str::from_utf8(name) {
            names.push(s);
        }
        pos = pos.saturating_add(name.len()).saturating_add(1);
    }
    (names, pos)
}

/// Extracts ALL name blocks from a name table, skipping binary metadata
/// between blocks.
///
/// The name table contains per-class blocks interleaved with binary
/// metadata (dispatch tables, GUIDs, paths). This function scans for
/// runs of VB6 identifier strings, collecting each run as a separate
/// block.
fn extract_all_name_blocks(data: &[u8]) -> Vec<Vec<&str>> {
    let mut blocks = Vec::new();
    let mut pos = 0usize;

    while pos < data.len() {
        let Some(&b) = data.get(pos) else { break };
        // Skip non-identifier bytes (binary metadata between blocks)
        if b == 0 {
            pos = pos.saturating_add(1);
            continue;
        }
        if !(0x20..=0x7E).contains(&b) {
            pos = pos.saturating_add(1);
            continue;
        }

        // Try to extract a name block starting here
        let Ok(name) = read_cstr(data, pos) else {
            pos = pos.saturating_add(1);
            continue;
        };
        if !is_vb_identifier(name) {
            pos = pos.saturating_add(1);
            continue;
        }

        // Found a valid identifier - extract the full block
        let (block, end) = extract_name_block(data, pos);
        if !block.is_empty() {
            blocks.push(block);
        }
        pos = end;
    }
    blocks
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
