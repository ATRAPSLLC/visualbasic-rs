//! VB6 entry point detection.
//!
//! The PE entry point of a VB6 executable (EXE) is a two-instruction stub:
//!
//! ```x86asm
//! push    offset VBHeader     ; 0x68 <imm32>
//! call    ThunRTMain          ; 0xE8 <rel32>, to the import thunk
//!                             ; jmp dword ptr [__imp_ThunRTMain] (0xFF 0x25)
//! ```
//!
//! `ThunRTMain` is MSVBVM60.DLL ordinal 100, imported by ordinal. P-Code and
//! native-code builds have the same stub.
//!
//! The entry point of a VB6 ActiveX DLL or OCX does not contain the VBHeader
//! VA. Its COM exports do: each is a stub that slips the VBHeader VA and two
//! more values under the caller's arguments and jumps to the matching runtime
//! function (`VBDllGetClassObject`, `VBDllRegisterServer`,
//! `VBDllUnRegisterServer`):
//!
//! ```x86asm
//! pop     eax                 ; 0x58: the return address
//! push    offset VBHeader     ; 0x68 <imm32>
//! push    <imm32>             ; 0x68 <imm32>
//! push    <imm32>             ; 0x68 <imm32>
//! push    eax                 ; 0x50
//! jmp     VBDllGetClassObject ; 0xE9 <rel32>
//! ```
//!
//! The `DllCanUnloadNow` stub pushes one value (`58 68 <imm32> 50 E9 <rel32>`).
//!
//! [`extract_vb_header_va`] reads the EXE stub and
//! [`extract_vb_header_va_from_exports`] the DLL stubs, each accepting only
//! a pushed VA that holds the `"VB5!"` magic;
//! [`VbProject::from_goblin`](crate::project::VbProject::from_goblin) tries
//! the first, then the second. Nothing scans the file for the magic.

use crate::{addressmap::AddressMap, error::Error};

/// Minimum number of bytes needed at the entry point to extract the VBHeader VA.
const MIN_ENTRY_BYTES: usize = 5;

/// x86 opcode for `push imm32`.
const PUSH_IMM32: u8 = 0x68;

/// VBHeader magic signature.
const VB5_MAGIC: &[u8; 4] = b"VB5!";

/// Extracts the VBHeader virtual address from the PE entry point.
///
/// Returns the immediate of the `push imm32` (`0x68`) that begins the entry
/// point when the `"VB5!"` magic is there (every fixture executable). A pushed VA
/// that the file is too short to hold four bytes at is returned as well, so
/// that the caller reports a truncated VB6 file. A DLL, whose entry point
/// does not push the VBHeader VA, is read with
/// [`extract_vb_header_va_from_exports`] instead.
///
/// # Arguments
///
/// * `map` - The PE address map for RVA-to-file-offset translation.
/// * `entry_point_rva` - The PE entry point RVA (from the optional header).
///
/// # Returns
///
/// The virtual address of the `VBHeader` structure.
///
/// # Errors
///
/// - [`Error::EntryPointNotPush`] if the entry point does not begin with
///   `0x68` and four more bytes; `byte` is the first byte there, or 0 when
///   the entry point RVA is not file-backed.
/// - [`Error::BadMagic`] if the pushed VA holds four bytes other than
///   `"VB5!"` (a packer's `push imm32; ret` entry, say).
/// - The address error if the pushed VA is in no section, below the image
///   base, or in a section's zero-filled part.
pub fn extract_vb_header_va(map: &AddressMap<'_>, entry_point_rva: u32) -> Result<u32, Error> {
    let Some(&[PUSH_IMM32, b0, b1, b2, b3]) = map
        .slice_from_rva(entry_point_rva, MIN_ENTRY_BYTES)
        .ok()
        .and_then(|code| code.first_chunk::<5>())
    else {
        let byte = map
            .slice_from_rva(entry_point_rva, 1)
            .ok()
            .and_then(|c| c.first().copied())
            .unwrap_or(0);
        return Err(Error::EntryPointNotPush { byte });
    };
    let va = u32::from_le_bytes([b0, b1, b2, b3]);
    match map.slice_from_va(va, VB5_MAGIC.len()) {
        Ok(data) => match data.first_chunk::<4>() {
            Some(magic) if magic == VB5_MAGIC => Ok(va),
            Some(magic) => Err(Error::BadMagic {
                expected: "VB5!",
                got: *magic,
            }),
            None => Err(Error::TooShort {
                expected: VB5_MAGIC.len(),
                actual: data.len(),
                context: "VBHeader magic",
            }),
        },
        // The file ends before the header: a truncated VB6 file.
        Err(Error::TooShort { .. }) => Ok(va),
        Err(e) => Err(e),
    }
}

/// Extracts the VBHeader VA from a VB6 DLL by checking its exports.
///
/// The COM exports of a VB6 ActiveX DLL or OCX (`DllGetClassObject`,
/// `DllRegisterServer`, ...) begin with `pop eax; push VBHeader_VA`
/// (`0x58 0x68 <imm32>`). Exports are tried in the order given; the first
/// whose immediate points to the `"VB5!"` magic wins.
///
/// Returns `None` if no export matches.
pub fn extract_vb_header_va_from_exports(
    map: &AddressMap<'_>,
    exports: &[goblin::pe::export::Export<'_>],
) -> Option<u32> {
    for export in exports {
        let rva = u32::try_from(export.rva).ok()?;
        // Read 6 bytes: pop eax (0x58) + push imm32 (0x68 xx xx xx xx)
        let Ok(code) = map.slice_from_rva(rva, 6) else {
            continue;
        };
        let Some(&[0x58, PUSH_IMM32, b0, b1, b2, b3]) = code.first_chunk::<6>() else {
            continue;
        };
        let candidate = u32::from_le_bytes([b0, b1, b2, b3]);
        // Validate: should point to VB5! magic
        if let Ok(magic) = map.slice_from_va(candidate, 4)
            && magic.starts_with(VB5_MAGIC)
        {
            return Some(candidate);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::addressmap::SectionEntry;

    /// Build an AddressMap with a .text section for testing.
    fn make_test_map(file: &[u8]) -> AddressMap<'_> {
        // Direct construction for testing
        AddressMap::from_parts(
            file,
            0x00400000,
            vec![SectionEntry {
                virtual_address: 0x1000,
                virtual_size: 0x1000,
                raw_data_offset: 0x200,
                raw_data_size: 0x1000,
            }],
        )
    }

    #[test]
    fn test_extract_vb_header_va_valid() {
        let mut file = vec![0u8; 0x2000];
        // Place "push 0x00401234" at file offset 0x200 (RVA 0x1000)
        file[0x200] = PUSH_IMM32;
        file[0x201] = 0x34;
        file[0x202] = 0x12;
        file[0x203] = 0x40;
        file[0x204] = 0x00;
        // Followed by call (0xE8) - not checked, just for realism
        file[0x205] = 0xE8;
        // The VBHeader magic at VA 0x00401234 (file offset 0x434).
        file[0x434..0x438].copy_from_slice(VB5_MAGIC);

        let map = make_test_map(&file);
        let va = extract_vb_header_va(&map, 0x1000).unwrap();
        assert_eq!(va, 0x00401234);
    }

    #[test]
    fn test_extract_vb_header_va_requires_the_magic() {
        let mut file = vec![0u8; 0x2000];
        // push 0x00401000; ret: the push points back at the entry point.
        file[0x200..0x206].copy_from_slice(&[PUSH_IMM32, 0x00, 0x10, 0x40, 0x00, 0xC3]);
        let map = make_test_map(&file);
        assert_eq!(
            extract_vb_header_va(&map, 0x1000),
            Err(Error::BadMagic {
                expected: "VB5!",
                got: [PUSH_IMM32, 0x00, 0x10, 0x40],
            })
        );
        // A VA in no section.
        file[0x201..0x205].copy_from_slice(&0x12345678u32.to_le_bytes());
        let map = make_test_map(&file);
        assert!(matches!(
            extract_vb_header_va(&map, 0x1000),
            Err(Error::RvaNotMapped { .. })
        ));
    }

    #[test]
    fn test_extract_vb_header_va_not_push() {
        let mut file = vec![0u8; 0x2000];
        // Entry point starts with 0xCC (int3) instead of 0x68
        file[0x200] = 0xCC;

        let map = make_test_map(&file);
        assert_eq!(
            extract_vb_header_va(&map, 0x1000),
            Err(Error::EntryPointNotPush { byte: 0xCC })
        );
    }

    #[test]
    fn test_extract_vb_header_va_too_short() {
        // File is too small to contain the full push instruction
        let file = vec![0u8; 0x203]; // Only 3 bytes after offset 0x200

        let map = make_test_map(&file);
        // Falls through to EntryPointNotPush since slice_from_rva fails
        assert!(extract_vb_header_va(&map, 0x1000).is_err());
    }

    #[test]
    fn test_extract_vb_header_va_rva_not_mapped() {
        let file = vec![0u8; 0x2000];
        let map = make_test_map(&file);
        // RVA 0x5000 is outside the .text section
        assert!(extract_vb_header_va(&map, 0x5000).is_err());
    }
}
