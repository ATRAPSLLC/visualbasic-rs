//! The PE import table, by import address table slot.
//!
//! A P-Code procedure calls a function the executable imports - a runtime
//! function such as `rtcIsMissing` or `VarPtr` - through an `ImpAdCall*`
//! whose constant pool entry is an import thunk `jmp [slot]` (`FF 25
//! <slot>`, [`PoolEntry::Import`](crate::vb::constantpool::PoolEntry::Import)).
//! [`ImportTable`] maps such a slot's VA to the library and function.
//!
//! VB6 executables import the runtime's `rtc*` functions by ordinal;
//! [`Import::function`] names `MSVBVM60.DLL` ordinals from the runtime's
//! export table ([`lookup_export_by_ordinal`]).

use std::{borrow::Cow, collections::BTreeMap};

use crate::{
    addressmap::AddressMap,
    util::{read_cstr, read_u32_le},
    vb::exports::{ExportSignature, lookup_export, lookup_export_by_ordinal},
};

/// How an import names its function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportSymbol {
    /// By name.
    Name(String),
    /// By ordinal.
    Ordinal(u16),
}

/// One imported function: an import address table slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    /// The library, as the import descriptor names it (e.g. `MSVBVM60.DLL`).
    pub library: String,
    /// The function.
    pub symbol: ImportSymbol,
}

impl Import {
    /// Returns `true` if the import is from the VB runtime, `MSVBVM60.DLL`.
    pub fn is_runtime(&self) -> bool {
        self.library.eq_ignore_ascii_case("MSVBVM60.DLL")
    }

    /// Returns the function's name: the imported name, a runtime ordinal's
    /// export name, or `#<ordinal>`.
    pub fn function(&self) -> Cow<'_, str> {
        match &self.symbol {
            ImportSymbol::Name(name) => Cow::Borrowed(name),
            ImportSymbol::Ordinal(ordinal) => match self.runtime_export() {
                Some(export) => Cow::Borrowed(export.name),
                None => Cow::Owned(format!("#{ordinal}")),
            },
        }
    }

    /// Returns the runtime export's signature, for an import from
    /// `MSVBVM60.DLL` the export table describes.
    pub fn runtime_export(&self) -> Option<&'static ExportSignature> {
        if !self.is_runtime() {
            return None;
        }
        match &self.symbol {
            ImportSymbol::Ordinal(ordinal) => lookup_export_by_ordinal(*ordinal),
            ImportSymbol::Name(name) => lookup_export(name),
        }
    }
}

/// The executable's imports, keyed by the VA of their import address table
/// slot.
///
/// Read fail-soft: a malformed descriptor, thunk or name ends that
/// descriptor's walk, and the imports read so far remain.
#[derive(Debug, Clone, Default)]
pub struct ImportTable {
    by_slot: BTreeMap<u32, Import>,
}

impl ImportTable {
    /// The most descriptors, and thunks per descriptor, read.
    const LIMIT: u32 = 0x4000;

    /// Reads the import directory at `directory_rva`.
    ///
    /// Each `IMAGE_IMPORT_DESCRIPTOR` (20 bytes: `OriginalFirstThunk`,
    /// `TimeDateStamp`, `ForwarderChain`, `Name`, `FirstThunk`) is read
    /// until the all-zero terminator; each thunk of its lookup table
    /// (`OriginalFirstThunk`, else `FirstThunk`) is an ordinal (bit 31 set)
    /// or the RVA of a hint and a name, and names the slot at the same index
    /// of `FirstThunk`.
    pub fn parse(map: &AddressMap<'_>, directory_rva: u32) -> Self {
        let mut by_slot = BTreeMap::new();
        if directory_rva == 0 {
            return Self { by_slot };
        }
        let image_base = map.image_base();
        let file = map.file();
        let dword =
            |rva: u32| -> Option<u32> { read_u32_le(file, map.rva_to_offset(rva).ok()?).ok() };
        let name_at = |rva: u32| -> Option<String> {
            let bytes = read_cstr(file, map.rva_to_offset(rva).ok()?).ok()?;
            Some(String::from_utf8_lossy(bytes).into_owned())
        };
        for index in 0..Self::LIMIT {
            let Some(descriptor) = index
                .checked_mul(20)
                .and_then(|offset| directory_rva.checked_add(offset))
            else {
                break;
            };
            let field = |offset: u32| descriptor.checked_add(offset).and_then(dword);
            let (Some(lookup), Some(name), Some(first)) = (field(0), field(12), field(16)) else {
                break;
            };
            if lookup == 0 && name == 0 && first == 0 {
                break;
            }
            let Some(library) = name_at(name) else {
                continue;
            };
            let lookup = if lookup == 0 { first } else { lookup };
            for slot in 0..Self::LIMIT {
                let Some(step) = slot.checked_mul(4) else {
                    break;
                };
                let (Some(thunk), Some(slot_rva)) = (
                    lookup.checked_add(step).and_then(dword),
                    first.checked_add(step),
                ) else {
                    break;
                };
                if thunk == 0 {
                    break;
                }
                let symbol = if thunk & 0x8000_0000 != 0 {
                    ImportSymbol::Ordinal(u16::try_from(thunk & 0xFFFF).unwrap_or(0))
                } else {
                    match thunk.checked_add(2).and_then(name_at) {
                        Some(name) => ImportSymbol::Name(name),
                        None => break,
                    }
                };
                by_slot.insert(
                    image_base.wrapping_add(slot_rva),
                    Import {
                        library: library.clone(),
                        symbol,
                    },
                );
            }
        }
        Self { by_slot }
    }

    /// Returns the import whose address table slot is at `va`.
    pub fn by_slot(&self, va: u32) -> Option<&Import> {
        self.by_slot.get(&va)
    }

    /// Returns every import with its slot's VA, in slot order.
    pub fn iter(&self) -> impl Iterator<Item = (u32, &Import)> {
        self.by_slot.iter().map(|(&va, import)| (va, import))
    }

    /// Returns the number of imports.
    pub fn len(&self) -> usize {
        self.by_slot.len()
    }

    /// Returns `true` if there are no imports.
    pub fn is_empty(&self) -> bool {
        self.by_slot.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::addressmap::SectionEntry;

    #[test]
    fn test_parse_names_and_ordinals() {
        // One section: RVA 0x1000 at file 0x200.
        let mut file = vec![0u8; 0x1200];
        let put = |file: &mut Vec<u8>, rva: u32, bytes: &[u8]| {
            let at = (rva - 0x1000 + 0x200) as usize;
            file[at..at + bytes.len()].copy_from_slice(bytes);
        };
        // Descriptor at RVA 0x1100: lookup 0x1200, name 0x1300, IAT 0x1000.
        put(&mut file, 0x1100, &0x1200u32.to_le_bytes());
        put(&mut file, 0x110C, &0x1300u32.to_le_bytes());
        put(&mut file, 0x1110, &0x1000u32.to_le_bytes());
        put(&mut file, 0x1300, b"MSVBVM60.DLL\0");
        // Lookup: ordinal 592, then the name "VarPtr" (hint first), then end.
        put(&mut file, 0x1200, &0x8000_0250u32.to_le_bytes());
        put(&mut file, 0x1204, &0x1320u32.to_le_bytes());
        put(&mut file, 0x1322, b"__vbaFreeStr\0");
        let map = AddressMap::from_parts(
            &file,
            0x00400000,
            vec![SectionEntry {
                virtual_address: 0x1000,
                virtual_size: 0x1000,
                raw_data_offset: 0x200,
                raw_data_size: 0x1000,
            }],
        );
        let imports = ImportTable::parse(&map, 0x1100);
        assert_eq!(imports.len(), 2);

        let first = imports.by_slot(0x00401000).unwrap();
        assert!(first.is_runtime());
        assert_eq!(first.symbol, ImportSymbol::Ordinal(592));
        assert_eq!(first.function(), "rtcIsMissing");

        let second = imports.by_slot(0x00401004).unwrap();
        assert_eq!(second.function(), "__vbaFreeStr");
        assert!(second.runtime_export().is_some());

        assert!(imports.by_slot(0x00401008).is_none());
        assert!(ImportTable::parse(&map, 0).is_empty());
    }
}
