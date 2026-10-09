//! Constant pool reader.
//!
//! Each VB6 object (module, class, form) has one constant pool, shared by all
//! its procedures: an array of 4-byte entries, each the VA of
//!
//! - a **BSTR** string literal (`LitStr`, `LitVarStr`, a late-bound member
//!   name);
//! - a **GUID**: the interface a `VCallHresult` reports a failure against;
//! - an **ObjectInfo**: the class `New` creates;
//! - a P-Code **procedure thunk** (`ImpAdCall*` to a module procedure or a
//!   `Friend` method);
//! - a **`Declare` stub** (`ImpAdCall*` to a declared DLL function);
//! - an **import thunk** `jmp [IAT]` (`ImpAdCall*` to a runtime function
//!   such as `rtcIsMissing`);
//! - any other address: a global another module holds (`ImpAdLd*`), the
//!   descriptor of an array `For Each` walks.
//!
//! # Addressing
//!
//! The pool's base comes from `ObjectInfo.lpConstants` (offset 0x34); the
//! runtime keeps it at `[ebp-0x54]` while a procedure runs. A `%s` / `%c`
//! operand is an entry **index**, which the handlers scale by 4:
//!
//! ```text
//! entry = *(u32 *)(lpConstants + 4 * index)
//! ```
//!
//! The entry is a pointer, not the data itself. Which kind an entry is
//! follows from the opcode that names it (`LitStr` reads a string,
//! `VCallHresult` a GUID); [`ConstantPool::entry_at`] recognizes the kinds
//! whose bytes say what they are, and the typed accessors
//! ([`string_at`](ConstantPool::string_at),
//! [`guid_at`](ConstantPool::guid_at), [`api_stub_at`](ConstantPool::api_stub_at))
//! read an entry as the kind its opcode implies.
//!
//! # BSTR Format
//!
//! VB6 stores its string constants as COM BSTRs:
//! - 4 bytes **before** the string pointer: length in bytes (not characters)
//! - Followed by the UTF-16LE string data
//! - Followed by a null terminator (2 bytes, `\0\0`)
//!
//! The BSTR pointer points to the **first character**, not the length prefix.

use crate::{
    addressmap::AddressMap,
    error::Error,
    util::read_u32_le,
    vb::{
        bstr::BStr,
        control::Guid,
        external::{CallApiStub, resolve_api_stub},
        object::ObjectInfo,
    },
};

/// A constant pool entry, by what its bytes say it is.
///
/// Returned by [`ConstantPool::entry_at`]. Entries whose bytes do not
/// identify them - a GUID, a global's address - are
/// [`Address`](PoolEntry::Address); the opcode naming the entry says which.
#[derive(Debug)]
pub enum PoolEntry<'a> {
    /// The entry is 0.
    Null,
    /// A non-empty BSTR string literal.
    String(BStr<'a>),
    /// A P-Code procedure's thunk (`ImpAdCall*` to a module procedure or a
    /// `Friend` method): the VA of the procedure's ProcDscInfo.
    Procedure {
        /// VA of the procedure's ProcDscInfo.
        proc_dsc_va: u32,
    },
    /// A `Declare` function's call stub.
    Declare(CallApiStub<'a>),
    /// An import thunk, `jmp [iat_va]` (`FF 25 <iat_va>`): a function the
    /// executable imports, usually from the VB runtime.
    Import {
        /// VA of the import address table slot.
        iat_va: u32,
    },
    /// An object's ObjectInfo, the class `New` creates: its
    /// `lpPublicObject` descriptor points back to it.
    ObjectInfo {
        /// VA of the ObjectInfo.
        va: u32,
        /// The object's index in the project's object table.
        object_index: u16,
    },
    /// Any other address.
    Address(u32),
}

/// Reader for a VB6 constant pool.
///
/// Every accessor takes an entry **index**, the value of a `%s` / `%c`
/// operand ([`Operand::ConstPoolIndex`](crate::pcode::operand::Operand::ConstPoolIndex)).
///
/// # Lifetime
///
/// The `'a` lifetime ties the reader to the file buffer through the
/// [`AddressMap`].
#[derive(Debug, Clone)]
pub struct ConstantPool<'a> {
    /// Address map used for VA-to-file-offset resolution.
    map: &'a AddressMap<'a>,
    /// Base VA of the constant pool (from `ObjectInfo.lpConstants`).
    data_const_va: u32,
}

impl<'a> ConstantPool<'a> {
    /// The longest member name [`name_at`](Self::name_at) reads, in UTF-16
    /// code units.
    pub const MAX_NAME_CHARS: usize = 1024;

    /// Creates a new constant pool reader.
    ///
    /// # Arguments
    ///
    /// * `map` - Address map for VA resolution.
    /// * `data_const_va` - Base VA of the constant pool
    ///   (from [`ObjectInfo::constants_va`](super::object::ObjectInfo::constants_va)).
    pub fn new(map: &'a AddressMap<'a>, data_const_va: u32) -> Self {
        Self { map, data_const_va }
    }

    /// Returns the base VA of the constant pool.
    #[inline]
    pub fn data_const_va(&self) -> u32 {
        self.data_const_va
    }

    /// Returns the VA entry `index` holds.
    ///
    /// # Errors
    ///
    /// Returns an address-translation error if the entry cannot be read.
    pub fn va_at(&self, index: u16) -> Result<u32, Error> {
        let va = self
            .data_const_va
            .wrapping_add(u32::from(index).wrapping_mul(4));
        read_u32_le(self.map.slice_from_va(va, 4)?, 0)
    }

    /// Returns the BSTR entry `index` points to (`LitStr`, `LitVarStr`, a
    /// late-bound member name).
    ///
    /// Returns `Ok(None)` for a null entry and for an entry that is no BSTR:
    /// its length prefix is odd or 64 KiB or more, or the two bytes after the
    /// characters are not the null terminator.
    ///
    /// # Errors
    ///
    /// Returns an address-translation error if the entry cannot be read.
    pub fn string_at(&self, index: u16) -> Result<Option<BStr<'a>>, Error> {
        let va = self.va_at(index)?;
        if va == 0 {
            return Ok(None);
        }
        Ok(self.bstr(va))
    }

    /// Returns the member name entry `index` points to: a null-terminated
    /// UTF-16 string with no length prefix, the form the late-bound opcodes
    /// (`LateMem*`, and the names of a named call's arguments) pass to
    /// `IDispatch::GetIDsOfNames`.
    ///
    /// Returns `Ok(None)` for a null entry and for one with no terminator in
    /// the first [`MAX_NAME_CHARS`](Self::MAX_NAME_CHARS) characters.
    ///
    /// # Errors
    ///
    /// Returns an address-translation error if the entry cannot be read.
    pub fn name_at(&self, index: u16) -> Result<Option<String>, Error> {
        let va = self.va_at(index)?;
        if va == 0 {
            return Ok(None);
        }
        let Ok(data) = self.map.slice_from_va(va, 2) else {
            return Ok(None);
        };
        let units: Vec<u16> = data
            .as_chunks::<2>()
            .0
            .iter()
            .take(Self::MAX_NAME_CHARS)
            .map(|&pair| u16::from_le_bytes(pair))
            .take_while(|&unit| unit != 0)
            .collect();
        if units.len() >= Self::MAX_NAME_CHARS || units.len().saturating_mul(2) >= data.len() {
            return Ok(None);
        }
        Ok(Some(String::from_utf16_lossy(&units)))
    }

    /// Returns the `Declare` stub entry `index` points to (`ImpAdCall*`).
    ///
    /// Returns `Ok(None)` for a null entry and for an entry that is no stub
    /// (see [`resolve_api_stub`] for the forms recognized).
    ///
    /// # Errors
    ///
    /// Returns an address-translation error if the entry cannot be read.
    pub fn api_stub_at(&self, index: u16) -> Result<Option<CallApiStub<'a>>, Error> {
        let va = self.va_at(index)?;
        if va == 0 {
            return Ok(None);
        }
        Ok(resolve_api_stub(self.map, va).ok())
    }

    /// Returns the GUID entry `index` points to: the interface of a
    /// `VCallHresult` (`%v`'s second half).
    ///
    /// Returns `Ok(None)` for a null entry.
    ///
    /// # Errors
    ///
    /// Returns an address-translation error if the entry or the 16 bytes it
    /// points to cannot be read.
    pub fn guid_at(&self, index: u16) -> Result<Option<Guid>, Error> {
        let va = self.va_at(index)?;
        if va == 0 {
            return Ok(None);
        }
        let data = self.map.slice_from_va(va, 16)?;
        Ok(Guid::from_bytes(data))
    }

    /// Classifies entry `index` by the bytes it points to.
    ///
    /// Checked in this order:
    ///
    /// 1. a procedure thunk: `BA <ProcDscInfo> B9 <engine> FF E1`, or a
    ///    `Friend` method's `B8 00000000 66 3D 33 C0 BA <ProcDscInfo> 68
    ///    <engine> C3`;
    /// 2. an import thunk `FF 25 <IAT slot>`;
    /// 3. a `Declare` stub;
    /// 4. an ObjectInfo whose descriptor (`+0x18`) points back to it;
    /// 5. a non-empty BSTR;
    /// 6. otherwise an [`Address`](PoolEntry::Address).
    ///
    /// # Errors
    ///
    /// Returns an address-translation error if the entry itself cannot be
    /// read.
    pub fn entry_at(&self, index: u16) -> Result<PoolEntry<'a>, Error> {
        /// The `Friend` thunk's bytes before the ProcDscInfo VA.
        const FRIEND_HEAD: [u8; 10] = [0xB8, 0, 0, 0, 0, 0x66, 0x3D, 0x33, 0xC0, 0xBA];

        let va = self.va_at(index)?;
        if va == 0 {
            return Ok(PoolEntry::Null);
        }
        if let Ok(code) = self.map.slice_from_va(va, 6) {
            let at = |offset: usize| read_u32_le(code, offset).ok();
            if code.first() == Some(&0xBA)
                && code.get(5) == Some(&0xB9)
                && code.get(10..12) == Some(&[0xFF, 0xE1])
                && let Some(proc_dsc_va) = at(1)
            {
                return Ok(PoolEntry::Procedure { proc_dsc_va });
            }
            if code.get(..10) == Some(&FRIEND_HEAD[..])
                && let Some(proc_dsc_va) = at(10)
            {
                return Ok(PoolEntry::Procedure { proc_dsc_va });
            }
            if code.get(..2) == Some(&[0xFF, 0x25])
                && let Some(iat_va) = at(2)
            {
                return Ok(PoolEntry::Import { iat_va });
            }
        }
        if let Ok(stub) = resolve_api_stub(self.map, va) {
            return Ok(PoolEntry::Declare(stub));
        }
        if let Some(object_index) = self.object_info(va) {
            return Ok(PoolEntry::ObjectInfo { va, object_index });
        }
        if let Some(bstr) = self.bstr(va)
            && !bstr.is_empty()
        {
            return Ok(PoolEntry::String(bstr));
        }
        Ok(PoolEntry::Address(va))
    }

    /// Returns an iterator over the first `count` entries, classified by
    /// [`entry_at`](Self::entry_at), with their indices.
    ///
    /// `ObjectInfo.wConstantsCount` gives an object's entry count.
    pub fn entries(&self, count: u16) -> ConstPoolIter<'a> {
        ConstPoolIter {
            pool: self.clone(),
            index: 0,
            count,
        }
    }

    /// Returns the index of the object whose ObjectInfo is at `va`, if `va`
    /// holds one: its public object descriptor's first field points back.
    fn object_info(&self, va: u32) -> Option<u16> {
        let info = ObjectInfo::parse(self.map.slice_from_va(va, ObjectInfo::SIZE).ok()?).ok()?;
        let descriptor = self
            .map
            .slice_from_va(info.public_object_va().ok()?, 4)
            .ok()?;
        (read_u32_le(descriptor, 0).ok()? == va)
            .then(|| info.object_index().ok())
            .flatten()
    }

    /// Reads the BSTR whose characters start at `va`, if the bytes there
    /// are one: an even length prefix under 64 KiB, then the characters and
    /// the null terminator.
    fn bstr(&self, va: u32) -> Option<BStr<'a>> {
        let byte_len = read_u32_le(self.map.slice_from_va(va.wrapping_sub(4), 4).ok()?, 0).ok()?;
        if byte_len >= 0x10000 || byte_len % 2 != 0 {
            return None;
        }
        let len = usize::try_from(byte_len).ok()?;
        let data = self.map.slice_from_va(va, len.checked_add(2)?).ok()?;
        if data.get(len..len.checked_add(2)?) != Some(&[0, 0]) {
            return None;
        }
        Some(BStr::new(va, byte_len, data.get(..len)?))
    }
}

/// Iterator over a constant pool's entries.
///
/// Yields `(index, Result<PoolEntry>)` pairs. Created by
/// [`ConstantPool::entries`].
#[must_use = "iterators are lazy and do nothing unless consumed"]
pub struct ConstPoolIter<'a> {
    pool: ConstantPool<'a>,
    index: u16,
    count: u16,
}

impl<'a> Iterator for ConstPoolIter<'a> {
    type Item = (u16, Result<PoolEntry<'a>, Error>);

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.count {
            return None;
        }
        let index = self.index;
        self.index = self.index.saturating_add(1);
        Some((index, self.pool.entry_at(index)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::addressmap::SectionEntry;

    /// One section: VA 0x401000..0x403000 at file 0x200; the pool at
    /// 0x401000 (file 0x200).
    fn make_test_map(file: &[u8]) -> AddressMap<'_> {
        AddressMap::from_parts(
            file,
            0x00400000,
            vec![SectionEntry {
                virtual_address: 0x1000,
                virtual_size: 0x2000,
                raw_data_offset: 0x200,
                raw_data_size: 0x2000,
            }],
        )
    }

    /// Writes `value` at the file offset of `va`.
    fn put(file: &mut [u8], va: u32, value: &[u8]) {
        let at = (va - 0x401000 + 0x200) as usize;
        file[at..at + value.len()].copy_from_slice(value);
    }

    /// Writes the BSTR `text` with its characters at `va`.
    fn put_bstr(file: &mut [u8], va: u32, text: &str) {
        let utf16: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
        put(file, va - 4, &(utf16.len() as u32).to_le_bytes());
        put(file, va, &utf16);
        put(file, va + utf16.len() as u32, &[0, 0]);
    }

    #[test]
    fn test_va_at_scales_the_index() {
        let mut file = vec![0u8; 0x3000];
        put(&mut file, 0x401008, &0x12345678u32.to_le_bytes());
        let map = make_test_map(&file);
        let pool = ConstantPool::new(&map, 0x00401000);
        assert_eq!(pool.data_const_va(), 0x00401000);
        assert_eq!(pool.va_at(2).unwrap(), 0x12345678);
        assert_eq!(pool.va_at(0).unwrap(), 0);
        // Past the section.
        assert!(pool.va_at(0x0800).is_err());
    }

    #[test]
    fn test_string_at() {
        let mut file = vec![0u8; 0x3000];
        put(&mut file, 0x401004, &0x00401104u32.to_le_bytes());
        put_bstr(&mut file, 0x401104, "Hello");
        // Entry 2: an odd length prefix is no BSTR.
        put(&mut file, 0x401008, &0x00401204u32.to_le_bytes());
        put(&mut file, 0x401200, &7u32.to_le_bytes());
        // Entry 3: no terminator after the characters.
        put(&mut file, 0x40100C, &0x00401304u32.to_le_bytes());
        put(&mut file, 0x401300, &2u32.to_le_bytes());
        put(&mut file, 0x401304, &[b'x', 0, b'y', 0]);
        // Entry 4: an empty BSTR.
        put(&mut file, 0x401010, &0x00401404u32.to_le_bytes());
        let map = make_test_map(&file);
        let pool = ConstantPool::new(&map, 0x00401000);

        assert!(pool.string_at(0).unwrap().is_none());
        let bstr = pool.string_at(1).unwrap().unwrap();
        assert_eq!(bstr.byte_length(), 10);
        assert_eq!(bstr.va(), 0x00401104);
        assert_eq!(bstr.to_string_lossy(), "Hello");
        assert!(pool.string_at(2).unwrap().is_none());
        assert!(pool.string_at(3).unwrap().is_none());
        assert!(pool.string_at(4).unwrap().unwrap().is_empty());

        assert!(matches!(pool.entry_at(1).unwrap(), PoolEntry::String(_)));
        // An empty "string" is no evidence of one.
        assert!(matches!(
            pool.entry_at(4).unwrap(),
            PoolEntry::Address(0x00401404)
        ));
    }

    #[test]
    fn test_name_at() {
        // The file ends with the section.
        let mut file = vec![0u8; 0x2200];
        // "Add" with no length prefix: the dword before it is a character.
        put(&mut file, 0x401000, &0x00401102u32.to_le_bytes());
        put(
            &mut file,
            0x401100,
            &[b'x', 0, b'A', 0, b'd', 0, b'd', 0, 0, 0],
        );
        // No terminator before the section's end.
        put(&mut file, 0x401004, &0x00402FFEu32.to_le_bytes());
        put(&mut file, 0x402FFE, &[b'z', 0]);
        let map = make_test_map(&file);
        let pool = ConstantPool::new(&map, 0x00401000);
        assert_eq!(pool.name_at(0).unwrap().as_deref(), Some("Add"));
        assert_eq!(pool.name_at(1).unwrap(), None);
        assert_eq!(pool.name_at(2).unwrap(), None);
    }

    #[test]
    fn test_guid_at() {
        let mut file = vec![0u8; 0x3000];
        put(&mut file, 0x401000, &0x00401100u32.to_le_bytes());
        let guid: [u8; 16] = core::array::from_fn(|i| i as u8 + 1);
        put(&mut file, 0x401100, &guid);
        let map = make_test_map(&file);
        let pool = ConstantPool::new(&map, 0x00401000);
        assert_eq!(pool.guid_at(0).unwrap(), Guid::from_bytes(&guid));
        assert!(pool.guid_at(1).unwrap().is_none());
    }

    #[test]
    fn test_entry_at_procedure_thunks() {
        let mut file = vec![0u8; 0x3000];
        // A module procedure: mov edx, 0x402000; mov ecx, 0x401050; jmp ecx.
        put(&mut file, 0x401000, &0x00401100u32.to_le_bytes());
        put(
            &mut file,
            0x401100,
            &[
                0xBA, 0x00, 0x20, 0x40, 0x00, 0xB9, 0x50, 0x10, 0x40, 0x00, 0xFF, 0xE1,
            ],
        );
        // A Friend method's thunk.
        put(&mut file, 0x401004, &0x00401120u32.to_le_bytes());
        put(
            &mut file,
            0x401120,
            &[
                0xB8, 0, 0, 0, 0, 0x66, 0x3D, 0x33, 0xC0, 0xBA, 0x40, 0x20, 0x40, 0x00, 0x68, 0x54,
                0x10, 0x40, 0x00, 0xC3,
            ],
        );
        // An import thunk: jmp [0x401004].
        put(&mut file, 0x401008, &0x00401140u32.to_le_bytes());
        put(&mut file, 0x401140, &[0xFF, 0x25, 0x04, 0x10, 0x40, 0x00]);
        let map = make_test_map(&file);
        let pool = ConstantPool::new(&map, 0x00401000);

        assert!(matches!(
            pool.entry_at(0).unwrap(),
            PoolEntry::Procedure {
                proc_dsc_va: 0x00402000
            }
        ));
        assert!(matches!(
            pool.entry_at(1).unwrap(),
            PoolEntry::Procedure {
                proc_dsc_va: 0x00402040
            }
        ));
        assert!(matches!(
            pool.entry_at(2).unwrap(),
            PoolEntry::Import { iat_va: 0x00401004 }
        ));
        assert!(matches!(pool.entry_at(3).unwrap(), PoolEntry::Null));
    }

    #[test]
    fn test_entry_at_object_info() {
        let mut file = vec![0u8; 0x3000];
        // ObjectInfo at 0x401100: object index 3, descriptor at 0x401200,
        // whose first field points back.
        put(&mut file, 0x401000, &0x00401100u32.to_le_bytes());
        put(&mut file, 0x401100, &[1, 0, 3, 0]);
        put(&mut file, 0x401118, &0x00401200u32.to_le_bytes());
        put(&mut file, 0x401200, &0x00401100u32.to_le_bytes());
        // The same without the back-pointer is an address.
        put(&mut file, 0x401004, &0x00401300u32.to_le_bytes());
        put(&mut file, 0x401300, &[1, 0, 3, 0]);
        put(&mut file, 0x401318, &0x00401200u32.to_le_bytes());
        let map = make_test_map(&file);
        let pool = ConstantPool::new(&map, 0x00401000);

        assert!(matches!(
            pool.entry_at(0).unwrap(),
            PoolEntry::ObjectInfo {
                va: 0x00401100,
                object_index: 3
            }
        ));
        assert!(matches!(
            pool.entry_at(1).unwrap(),
            PoolEntry::Address(0x00401300)
        ));
    }

    #[test]
    fn test_api_stub_at_classification() {
        let mut file = vec![0u8; 0x3000];
        // Entry 1: a stub, push 0x00401400.
        put(&mut file, 0x401004, &0x00401300u32.to_le_bytes());
        put(&mut file, 0x401300, &[0x68, 0x00, 0x14, 0x40, 0x00]);
        // CallApiStub at 0x401400: library_va, function_va.
        put(&mut file, 0x401400, &0x00401500u32.to_le_bytes());
        put(&mut file, 0x401404, &0x00401510u32.to_le_bytes());
        put(&mut file, 0x401500, b"kernel32\0");
        put(&mut file, 0x401510, b"GetLastError\0");
        // Entry 2: a BSTR, not a stub.
        put(&mut file, 0x401008, &0x00401204u32.to_le_bytes());
        put_bstr(&mut file, 0x401204, "ab");
        let map = make_test_map(&file);
        let pool = ConstantPool::new(&map, 0x00401000);

        assert!(pool.api_stub_at(0).unwrap().is_none());
        let stub = pool.api_stub_at(1).unwrap().unwrap();
        assert_eq!(stub.library_name_bytes(&map).unwrap(), b"kernel32");
        assert_eq!(stub.function_name_bytes(&map).unwrap(), b"GetLastError");
        assert!(pool.api_stub_at(2).unwrap().is_none());
        assert!(matches!(pool.entry_at(1).unwrap(), PoolEntry::Declare(_)));

        let kinds: Vec<_> = pool
            .entries(3)
            .map(|(index, entry)| (index, entry.unwrap()))
            .collect();
        assert!(matches!(kinds[0], (0, PoolEntry::Null)));
        assert!(matches!(kinds[1], (1, PoolEntry::Declare(_))));
        assert!(matches!(kinds[2], (2, PoolEntry::String(_))));
    }
}
