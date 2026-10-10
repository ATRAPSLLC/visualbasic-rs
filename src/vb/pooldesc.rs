//! Descriptors the constant pool holds for the P-Code that names them.
//!
//! Besides strings, GUIDs and addresses, a procedure's constant pool points
//! at descriptors the runtime reads when an instruction names them (see
//! [`PoolEntryRole`](crate::pcode::decoder::PoolEntryRole)):
//!
//! - [`RecordIoDescriptor`]: how `Get #` and `Put #` transfer a record;
//! - [`IoItems`]: the items of a `Print`, `Write` or `Input` statement;
//! - [`ArrayDescriptor`]: the `SAFEARRAY` header of a fixed array inside a
//!   record;
//! - [`CreationDescriptor`]: the class `New` creates when it is not one of
//!   the project's own.
//!
//! The layouts are the ones MSVBVM60 6.00.8176 reads; the fixtures `records`
//! and `udts` hold nested records, record arrays and record I/O.

use crate::{
    addressmap::AddressMap,
    error::Error,
    util::{read_i16_le, read_u16_le, read_u32_le},
};

/// How deep [`RecordIoDescriptor::extent_size`] follows nested records.
const MAX_RECORD_DEPTH: usize = 16;

/// What a [`RecordIoEntry`] transfers, its kind (the low nibble of its
/// flags).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecordIoKind {
    /// A `String` member (a BSTR).
    String,
    /// A `Variant` member.
    Variant,
    /// A nested record, whose own descriptor
    /// [`RecordIoDescriptor::nested`] reads.
    Record,
    /// A plain element of [`RecordIoEntry::value`] bytes.
    Raw,
    /// A plain scalar of the size in the flags' high byte. No fixture has
    /// one.
    Scalar,
    /// Bytes skipped without a transfer, the size in the flags' high byte.
    Skip,
    /// A `String * N` member, N in [`RecordIoEntry::value`] (2N bytes in
    /// memory).
    FixedString,
    /// Another kind value.
    Other(u8),
}

/// One entry of a [`RecordIoDescriptor`] (12 bytes).
///
/// | Offset | Size | Field |
/// |--------|------|-------|
/// | 0x00 | 4 | Plain bytes before the member, transferred as they are |
/// | 0x04 | 2 | Flags: kind in bits 0-3, 0x10 a fixed array, 0x20 a dynamic array (a `SAFEARRAY` pointer); the size of a [`Scalar`](RecordIoKind::Scalar) or [`Skip`](RecordIoKind::Skip) in the high byte |
/// | 0x06 | 4 | Element count of a fixed array; 1 for a single member, 0xFFFFFFFF for a dynamic array |
/// | 0x0A | 2 | A nested record's descriptor, as an offset from this entry; a [`Raw`](RecordIoKind::Raw) element's size; a [`FixedString`](RecordIoKind::FixedString)'s length |
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordIoEntry {
    /// Plain bytes before the member.
    pub gap: u32,
    /// The flags.
    pub flags: u16,
    /// The element count.
    pub count: u32,
    /// The kind-dependent value at 0x0A.
    pub value: u16,
}

impl RecordIoEntry {
    /// Size of one entry.
    pub const SIZE: usize = 12;

    /// What the entry transfers.
    pub fn kind(&self) -> RecordIoKind {
        match self.flags & 0x0F {
            0 => RecordIoKind::String,
            1 => RecordIoKind::Variant,
            2 => RecordIoKind::Record,
            3 => RecordIoKind::Raw,
            4 => RecordIoKind::Scalar,
            5 => RecordIoKind::Skip,
            6 => RecordIoKind::FixedString,
            other => RecordIoKind::Other(u8::try_from(other).unwrap_or(u8::MAX)),
        }
    }

    /// Returns `true` for a fixed array of [`count`](Self::count) elements.
    pub fn is_fixed_array(&self) -> bool {
        self.flags & 0x10 != 0
    }

    /// Returns `true` for a dynamic array, a `SAFEARRAY` pointer.
    pub fn is_dynamic_array(&self) -> bool {
        self.flags & 0x20 != 0
    }
}

/// The descriptor `Get #` and `Put #` of a record read (`GetRecOwn*`,
/// `PutRecOwn*`): a 10-byte header, its entries, then the descriptors of
/// its nested records.
///
/// | Offset | Size | Field |
/// |--------|------|-------|
/// | 0x00 | 4 | Bytes of the record the transfer covers |
/// | 0x04 | 4 | Number of [`RecordIoEntry`]s |
/// | 0x08 | 2 | Padding after the record: a nested record's array stride is its size plus this |
/// | 0x0A | 12 each | The entries |
///
/// The runtime walks the entries recursively (MSVBVM60 6.00.8176
/// `0x6601C660`, from `__vbaGetOwner3`, `__vbaPutOwner3` and their record
/// number forms), then transfers the bytes the entries left as they are.
/// Each nested record's descriptor follows the entry array, in entry order,
/// each followed by its own nested ones (`records`' `Outer`: 9 entries and
/// three copies of `Inner`'s). `coverage`'s `FileRec` (`n As Long`, `x As
/// Double`, `c As Currency`, `fx As String * 8`) has one entry, the
/// fixed-length string 0x14 bytes in.
#[derive(Clone, Copy, Debug)]
pub struct RecordIoDescriptor<'a> {
    bytes: &'a [u8],
}

impl<'a> RecordIoDescriptor<'a> {
    /// Size of the header before the entries.
    pub const HEADER_SIZE: usize = 10;

    /// Parses a descriptor from a slice that runs at least to its end (to
    /// the end of the file for [`nested`](Self::nested)).
    ///
    /// # Errors
    ///
    /// Returns [`Error::TooShort`] if `data` is shorter than the header.
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        if data.len() < Self::HEADER_SIZE {
            return Err(Error::TooShort {
                expected: Self::HEADER_SIZE,
                actual: data.len(),
                context: "RecordIoDescriptor",
            });
        }
        Ok(Self { bytes: data })
    }

    /// Reads the descriptor at `va`.
    ///
    /// # Errors
    ///
    /// Returns an error if `va` is not mapped or the header does not fit.
    pub fn at(map: &AddressMap<'a>, va: u32) -> Result<Self, Error> {
        Self::parse(map.slice_from_va(va, Self::HEADER_SIZE)?)
    }

    /// Bytes of the record the transfer covers, at offset 0x00.
    #[inline]
    pub fn record_size(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x00)
    }

    /// Number of entries, at offset 0x04.
    #[inline]
    pub fn entry_count(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x04)
    }

    /// Padding after the record, at offset 0x08.
    #[inline]
    pub fn tail_padding(&self) -> Result<i16, Error> {
        read_i16_le(self.bytes, 0x08)
    }

    /// Reads the entries; empty when they do not all read.
    pub fn entries(&self) -> Vec<RecordIoEntry> {
        let Some(count) = self
            .entry_count()
            .ok()
            .and_then(|count| usize::try_from(count).ok())
            .filter(|&count| {
                count
                    .checked_mul(RecordIoEntry::SIZE)
                    .and_then(|len| len.checked_add(Self::HEADER_SIZE))
                    .is_some_and(|end| end <= self.bytes.len())
            })
        else {
            return Vec::new();
        };
        (0..count)
            .map_while(|index| {
                let at = Self::HEADER_SIZE.checked_add(index.checked_mul(RecordIoEntry::SIZE)?)?;
                Some(RecordIoEntry {
                    gap: read_u32_le(self.bytes, at).ok()?,
                    flags: read_u16_le(self.bytes, at.checked_add(4)?).ok()?,
                    count: read_u32_le(self.bytes, at.checked_add(6)?).ok()?,
                    value: read_u16_le(self.bytes, at.checked_add(10)?).ok()?,
                })
            })
            .collect()
    }

    /// Returns the descriptor of entry `index`'s nested record: at the
    /// entry's offset plus its [`value`](RecordIoEntry::value). `None` for
    /// another kind or when it is outside the parsed slice.
    pub fn nested(&self, index: usize) -> Option<Self> {
        let entry = self.entries().get(index).copied()?;
        if entry.kind() != RecordIoKind::Record {
            return None;
        }
        let at = Self::HEADER_SIZE
            .checked_add(index.checked_mul(RecordIoEntry::SIZE)?)?
            .checked_add(usize::from(entry.value))?;
        Self::parse(self.bytes.get(at..)?).ok()
    }

    /// Returns the bytes the descriptor occupies with its nested
    /// descriptors: `0x0A + 12 * entries` and every nested descriptor's
    /// extent.
    ///
    /// `None` when the entries or a nested descriptor do not read, or
    /// nesting goes deeper than 16 records.
    pub fn extent_size(&self) -> Option<usize> {
        self.extent_size_at(0)
    }

    /// [`extent_size`](Self::extent_size), `depth` records deep.
    fn extent_size_at(&self, depth: usize) -> Option<usize> {
        if depth > MAX_RECORD_DEPTH {
            return None;
        }
        let entries = self.entries();
        if entries.len() != usize::try_from(self.entry_count().ok()?).ok()? {
            return None;
        }
        let mut end =
            Self::HEADER_SIZE.checked_add(entries.len().checked_mul(RecordIoEntry::SIZE)?)?;
        for (index, entry) in entries.iter().enumerate() {
            if entry.kind() != RecordIoKind::Record {
                continue;
            }
            let start = Self::HEADER_SIZE
                .checked_add(index.checked_mul(RecordIoEntry::SIZE)?)?
                .checked_add(usize::from(entry.value))?;
            let nested = self.nested(index)?.extent_size_at(depth.checked_add(1)?)?;
            end = end.max(start.checked_add(nested)?);
        }
        Some(end)
    }
}

/// What separates an [`IoItem`] from the next (bits 6-7 of its byte).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IoSeparator {
    /// `,`: `Print` moves to the next print zone; `Write` writes a comma.
    Comma,
    /// `;`: nothing between the items; `Write` writes a comma.
    Semicolon,
    /// The end of the line: a carriage return and line feed.
    NewLine,
    /// The fourth value, which no statement emits.
    Other,
}

/// One item of an [`IoItems`] list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IoItem(pub u8);

impl IoItem {
    /// The item's type (bits 0-5): 0 none (a separator alone), 2 `Integer`,
    /// 3 `Long`, 4 `Single`, 5 `Double`, 6 `Currency`, 7 `Date`, 8 `String`,
    /// 9 `Object`, 0x0A error, 0x0B `Boolean`, 0x0C a Variant's address,
    /// 0x11 `Byte`, 0x20 `Spc(n)`, 0x21 `Tab(n)`. It decides how many
    /// argument dwords the item takes: none for 0, two for a `Double`,
    /// `Currency` or `Date`, one otherwise.
    pub fn item_type(self) -> u8 {
        self.0 & 0x3F
    }

    /// What follows the item.
    pub fn separator(self) -> IoSeparator {
        match self.0 >> 6 {
            0 => IoSeparator::Comma,
            1 => IoSeparator::Semicolon,
            2 => IoSeparator::NewLine,
            _ => IoSeparator::Other,
        }
    }
}

/// The item list of a `Print`, `Write` or `Input` statement (`PrintObject`,
/// `PrintFile`, `WriteFile`, `InputFile`): a u16 count, then one
/// [`IoItem`] byte per item.
///
/// `Print`, `Print #` and `Write #` read the items with the arguments
/// pushed for them (MSVBVM60 6.00.8176 `0x6600D080`, from `__vbaPrintObj`,
/// `__vbaPrintFile` and `__vbaWriteFile`); `Input #` reads only the types,
/// one destination per item (`__vbaInputFile`). `forms`: `01 00 88`, one
/// `String` and a new line.
#[derive(Clone, Copy, Debug)]
pub struct IoItems<'a> {
    bytes: &'a [u8],
}

impl<'a> IoItems<'a> {
    /// Parses an item list from a slice that runs at least to its end.
    ///
    /// # Errors
    ///
    /// Returns [`Error::TooShort`] if `data` is shorter than its count says.
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        let size = usize::from(read_u16_le(data, 0)?).saturating_add(2);
        let bytes = data.get(..size).ok_or(Error::TooShort {
            expected: size,
            actual: data.len(),
            context: "IoItems",
        })?;
        Ok(Self { bytes })
    }

    /// Reads the item list at `va`.
    ///
    /// # Errors
    ///
    /// Returns an error if `va` is not mapped or the list does not fit.
    pub fn at(map: &AddressMap<'a>, va: u32) -> Result<Self, Error> {
        Self::parse(map.slice_from_va(va, 2)?)
    }

    /// Returns the bytes the list occupies: 2 plus one per item.
    #[inline]
    pub fn size(&self) -> usize {
        self.bytes.len()
    }

    /// The items.
    pub fn items(&self) -> impl Iterator<Item = IoItem> + '_ {
        self.bytes
            .get(2..)
            .unwrap_or(&[])
            .iter()
            .copied()
            .map(IoItem)
    }
}

/// The `SAFEARRAY` header of a fixed array inside a record, which
/// `AryInRecLdPr` and `AryInRecLdRf` index: 0x10 bytes, then a bound per
/// dimension.
///
/// | Offset | Size | Field |
/// |--------|------|-------|
/// | 0x00 | 2 | `cDims` |
/// | 0x02 | 2 | `fFeatures` |
/// | 0x04 | 4 | `cbElements` |
/// | 0x08 | 4 | `cLocks` |
/// | 0x0C | 4 | `pvData`: 0, the array's offset in the record |
/// | 0x10 | 8 each | `{cElements, lLbound}`, rightmost dimension first |
///
/// The runtime requires `cDims` to equal the instruction's dimension count
/// and adds the element's offset to the record's address (MSVBVM60
/// 6.00.8176 `0x66108E4E`). `data`'s `Record.Flags(3) As Byte`: one
/// dimension of 4 one-byte elements.
#[derive(Clone, Copy, Debug)]
pub struct ArrayDescriptor<'a> {
    bytes: &'a [u8],
}

impl<'a> ArrayDescriptor<'a> {
    /// Size of the header before the bounds.
    pub const HEADER_SIZE: usize = 0x10;

    /// Parses a descriptor from a slice that runs at least to its end.
    ///
    /// # Errors
    ///
    /// Returns [`Error::TooShort`] if `data` is shorter than its dimension
    /// count says.
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        let size = usize::from(read_u16_le(data, 0)?)
            .saturating_mul(8)
            .saturating_add(Self::HEADER_SIZE);
        let bytes = data.get(..size).ok_or(Error::TooShort {
            expected: size,
            actual: data.len(),
            context: "ArrayDescriptor",
        })?;
        Ok(Self { bytes })
    }

    /// Reads the descriptor at `va`.
    ///
    /// # Errors
    ///
    /// Returns an error if `va` is not mapped or the descriptor does not
    /// fit.
    pub fn at(map: &AddressMap<'a>, va: u32) -> Result<Self, Error> {
        Self::parse(map.slice_from_va(va, Self::HEADER_SIZE)?)
    }

    /// Returns the bytes the descriptor occupies: `0x10 + 8 * cDims`.
    #[inline]
    pub fn size(&self) -> usize {
        self.bytes.len()
    }

    /// Number of dimensions (`cDims`), at offset 0x00.
    #[inline]
    pub fn dimensions(&self) -> Result<u16, Error> {
        read_u16_le(self.bytes, 0x00)
    }

    /// `fFeatures`, at offset 0x02.
    #[inline]
    pub fn features(&self) -> Result<u16, Error> {
        read_u16_le(self.bytes, 0x02)
    }

    /// Size of an element (`cbElements`), at offset 0x04.
    #[inline]
    pub fn element_size(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x04)
    }

    /// The bounds, `(element count, lower bound)` per dimension, rightmost
    /// dimension first.
    pub fn bounds(&self) -> Vec<(u32, i32)> {
        self.bytes
            .get(Self::HEADER_SIZE..)
            .unwrap_or(&[])
            .as_chunks::<8>()
            .0
            .iter()
            .filter_map(|bound| {
                Some((
                    read_u32_le(bound, 0).ok()?,
                    read_u32_le(bound, 4).ok()?.cast_signed(),
                ))
            })
            .collect()
    }
}

/// What `New` creates when it is not a class of the project (0x10 bytes):
/// the class's CLSID, the interface to query, and its licence key.
///
/// | Offset | Size | Field |
/// |--------|------|-------|
/// | 0x00 | 4 | Flags: bit 0 clear (a project class's `ObjectInfo` has it set); bit 1, try the running object first |
/// | 0x04 | 4 | VA of the CLSID |
/// | 0x08 | 4 | VA of the IID queried after creation |
/// | 0x0C | 4 | VA of the licence key, `{u32 byte length, UTF-16}`, or 0 |
///
/// The runtime creates the class with `CoCreateInstance`, or with
/// `IClassFactory2::CreateInstanceLic` when there is a key, and queries
/// the interface (MSVBVM60 6.00.8176 `0x66011021`, from the `New` and
/// `NewIfNull*` handlers). `forms`: VB's `Global` (`{FCFB3D23-...}`) as
/// `VBGlobal`, flags 2; `coverage`: VBA's `Collection` as `_Collection`.
#[derive(Clone, Copy, Debug)]
pub struct CreationDescriptor<'a> {
    bytes: &'a [u8],
}

impl<'a> CreationDescriptor<'a> {
    /// Size of the structure in bytes.
    pub const SIZE: usize = 0x10;

    /// Reads the creation descriptor at `va`, `None` when the bytes there
    /// are a project class's `ObjectInfo` (flag bit 0 set) or do not read.
    pub fn at(map: &AddressMap<'a>, va: u32) -> Option<Self> {
        let bytes = map.slice_from_va(va, Self::SIZE).ok()?.get(..Self::SIZE)?;
        (bytes.first()? & 1 == 0).then_some(Self { bytes })
    }

    /// The flags at offset 0x00.
    #[inline]
    pub fn flags(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x00)
    }

    /// VA of the class's CLSID, at offset 0x04.
    #[inline]
    pub fn clsid_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x04)
    }

    /// VA of the interface's IID, at offset 0x08.
    #[inline]
    pub fn iid_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x08)
    }

    /// VA of the licence key, at offset 0x0C; 0 for none.
    #[inline]
    pub fn license_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x0C)
    }
}
