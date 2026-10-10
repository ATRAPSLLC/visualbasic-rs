//! ProcDscInfo (RTMI) structure parser.
//!
//! `ProcDscInfo` trails each P-Code byte stream and contains the frame size,
//! argument size, and a pointer to the parent [`ObjectInfo`](super::object::ObjectInfo).
//!
//! The critical relationship for locating P-Code bytes:
//!
//! ```text
//! P-Code Start = &ProcDscInfo - ProcDscInfo.wPCodeBackOffset
//! ```
//!
//! # Runtime Confirmation
//!
//! `ProcCallEngine` (MSVBVM60 6.00.8176, 0x66104a99) dereferences the first
//! dword of ProcDscInfo as an ObjectInfo pointer:
//! - `*ProcDscInfo` → ObjectInfo
//! - `ObjectInfo.lpConstants` (+0x34) → constant pool base for P-Code execution
//! - `ObjectInfo.lpObjectTable` (+0x04) → ObjectTable → project data
//!
//! # Variable-Length Structure
//!
//! ProcDscInfo is **not** a fixed-size struct. The base header is 0x18 bytes,
//! followed by two [`CleanupTable`]s: the primary one at +0x18 and the
//! secondary one at `wTotalSize` (+0x0A), which is `0x18 + ` the primary
//! table's size rounded up to a multiple of 4. A procedure with line numbers
//! also has a line-number table at the self-relative offset in +0x0E (see
//! [`ProcDscInfo::line_numbers`]).

use std::{fmt, ops::Range};

use crate::{
    error::Error,
    util::{read_u16_le, read_u32_le},
    vb::controlprop::ControlPropertyIter,
};

/// View over a cleanup/property table.
///
/// This is the common table format of the two tables that follow a
/// [`ProcDscInfo`] header (MSVBVM60 6.00.8176 addresses):
///
/// - the primary table (ProcDscInfo +0x18) lists the procedure's locals that
///   hold a resource, and a function's return value: `ProcCallEngine`
///   initializes its first `wCount` entries on entry (0x66104b44, 0x660e99bc)
///   and `ExitProc` releases its entries on return (0x661064a5, 0x66027da2);
/// - the secondary table (ProcDscInfo + `wTotalSize`) lists the temporaries
///   the `FFree*` opcodes release; the runtime releases them when an error
///   unwinds the procedure (0x66107ff6..0x6610800e), not on entry or a normal
///   exit.
///
/// # Layout
///
/// | Offset | Size | Field |
/// |--------|------|-------|
/// | 0x00 | 2 | `wSize` - total table size in bytes (including this header; not always a multiple of 4) |
/// | 0x02 | 2 | Reserved (0 in every table of the fixtures) |
/// | 0x04 | 2 | `wCount` - leading entries to initialize on entry (fixed-size arrays, UDTs; 0 in a secondary table) |
/// | 0x06 | 2 | `wTotal` - entry count; every entry is released |
/// | 0x08 | 4 | Flags: bit 0 of byte +0x0B = the first entry is the function's return value |
/// | 0x0C | var | [`ControlPropertyEntry`](super::controlprop::ControlPropertyEntry) records |
///
/// Minimum size is 0x0C (header only, no entries).
#[derive(Clone, Copy, Debug)]
pub struct CleanupTable<'a> {
    bytes: &'a [u8],
}

impl<'a> CleanupTable<'a> {
    /// Size of the fixed header before entries.
    pub const HEADER_SIZE: usize = 0x0C;

    /// Parses a cleanup table from the given byte slice.
    ///
    /// The slice must be at least [`HEADER_SIZE`](Self::HEADER_SIZE) bytes.
    /// The actual table extent is [`size`](Self::size) bytes.
    pub fn parse(data: &'a [u8]) -> Option<Self> {
        if data.len() < Self::HEADER_SIZE {
            return None;
        }
        let size = read_u16_le(data, 0x00).ok()? as usize;
        if size < Self::HEADER_SIZE || size > data.len() {
            return None;
        }
        Some(Self {
            bytes: data.get(..size)?,
        })
    }

    /// Total table size in bytes (header + entries) at offset 0x00.
    #[inline]
    pub fn size(&self) -> Result<u16, Error> {
        read_u16_le(self.bytes, 0x00)
    }

    /// Number of leading entries the runtime initializes on entry, at
    /// offset 0x04.
    ///
    /// `ProcCallEngine` calls its table initializer only when this is
    /// non-zero (MSVBVM60 6.00.8176, 0x66104b44), and the initializer
    /// (0x660e99bc) stops after this many entries. In the fixtures it counts
    /// the fixed-size arrays and UDTs among the locals (`Dim fixed(3) As
    /// Long` gives 1); it is 0 in every secondary table.
    #[inline]
    pub fn count(&self) -> Result<u16, Error> {
        read_u16_le(self.bytes, 0x04)
    }

    /// Total number of entries in the table at offset 0x06.
    ///
    /// The release routine (MSVBVM60 6.00.8176, 0x66027da2) walks all of
    /// them; [`count`](Self::count) only limits initialization.
    #[inline]
    pub fn total(&self) -> Result<u16, Error> {
        read_u16_le(self.bytes, 0x06)
    }

    /// Flags dword at offset 0x08.
    ///
    /// Bit 0 of byte +0x0B marks the first entry as the function's return
    /// value (a String, Object, Variant or array result): the initializer
    /// (0x660e99ce) and the release on a normal exit (0x66027d7e) skip it.
    /// The fixtures also show byte +0x0B = 0x10 in four procedures, meaning
    /// unknown.
    #[inline]
    pub fn flags(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x08)
    }

    /// Returns `true` if the table has any entries.
    #[inline]
    pub fn has_entries(&self) -> bool {
        self.total().unwrap_or(0) > 0
    }

    /// Returns an iterator over the table's entries.
    ///
    /// Each entry is a [`ControlPropertyEntry`](super::controlprop::ControlPropertyEntry)
    /// with a frame offset and type. For cleanup tables, the frame offset
    /// is a **signed i16** (negative offset from EBP), unlike instance
    /// data entries which use unsigned offsets.
    pub fn entries(&self) -> ControlPropertyIter<'a> {
        if self.bytes.len() > Self::HEADER_SIZE {
            let total = self.total().unwrap_or(0);
            match self.bytes.get(Self::HEADER_SIZE..) {
                Some(rest) => ControlPropertyIter::new(rest, total),
                None => ControlPropertyIter::new(&[], 0),
            }
        } else {
            ControlPropertyIter::new(&[], 0)
        }
    }

    /// Raw bytes of the table (header + entries).
    #[inline]
    pub fn as_bytes(&self) -> &'a [u8] {
        self.bytes
    }
}

/// View over a ProcDscInfo (RTMI) structure.
///
/// This structure immediately follows the P-Code byte stream for each
/// procedure. The P-Code start address is calculated as:
///
/// ```text
/// pcode_start = address_of(ProcDscInfo) - ProcDscInfo.wPCodeBackOffset
/// ```
///
/// # Layout
///
/// ```text
/// +0x00: Base header (0x18 bytes)
///   +0x00  u32  lpObjectInfo       VA of parent ObjectInfo
///   +0x04  u16  wArgSize           bytes ExitProc pops: the ebp+8 slot and the arguments
///   +0x06  u16  wFrameSize         local variable frame size
///   +0x08  u16  wPCodeBackOffset   P-Code stream size (back-offset)
///   +0x0A  u16  wTotalSize         offset of the secondary table: align4(0x18 + primary size)
///   +0x0C  u16  wProcOptFlags      entry options (0x10 Friend, 0x20; see ProcOptFlags)
///   +0x0E  i16  wLineTableOff      self-relative offset of the line-number table, or 0
///   +0x10  u16  wResumeFixupOff    Resume Next fallback fixup-table offset (0 in the fixtures)
///   +0x12  u16  unknown            0x26 or 0 in the fixtures
///   +0x14  u16  reserved           (0 in the fixtures)
///   +0x16  u16  reserved           (0 in the fixtures)
///
/// +0x18: Primary CleanupTable (locals and return value; entry and exit)
///   +0x00  u16  wSize              table size including this header
///   +0x02  u16  reserved
///   +0x04  u16  wCount             leading entries to initialize on entry
///   +0x06  u16  wTotal             entry count (all released)
///   +0x08  u32  flags              byte +0x0B bit 0: first entry is the return value
///   +0x0C  var  ControlPropertyEntry[] records
///
/// +wTotalSize: Secondary CleanupTable (temporaries; released when an
///   error unwinds the procedure). Same header format; always present
///   (minimum 0x0C bytes).
///
/// +wLineTableOff (when non-zero): line-number table
///   +0x00  u16  count
///   +0x02  (u16 P-Code offset, u16 line number)[count]
/// ```
///
/// The primary cleanup table describes the local variables needing resource
/// release on procedure exit or error, and a String, Object, Variant or
/// array return value (strings via `SysFreeString`, COM objects via
/// `IUnknown::Release`, SafeArrays via `SafeArrayDestroy`, etc.).
///
/// `wTotalSize` at +0x0A only covers the header and the primary table. Use
/// [`actual_size`](Self::actual_size) for the extent including the
/// secondary table. When procedures are adjacent, the next one's P-Code
/// starts at the next multiple of 4 after the secondary table, or after the
/// line-number table when there is one.
#[derive(Clone, Copy, Debug)]
pub struct ProcDscInfo<'a> {
    /// Raw backing bytes borrowed from the PE file buffer.
    bytes: &'a [u8],
}

impl<'a> ProcDscInfo<'a> {
    /// Minimum size needed to read the fixed fields (through +0x1C).
    pub const MIN_SIZE: usize = 0x1E;

    /// Size of the fixed header portion (before error handler table).
    pub const HEADER_SIZE: usize = 0x18;

    /// Parses a ProcDscInfo from the given byte slice.
    ///
    /// Reads at least [`MIN_SIZE`](Self::MIN_SIZE) bytes. The full structure
    /// may be larger - use [`total_size`](Self::total_size) to determine
    /// the actual extent.
    ///
    /// # Errors
    ///
    /// Returns [`Error::TooShort`] if `data.len() < MIN_SIZE`.
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        if data.len() < Self::MIN_SIZE {
            return Err(Error::TooShort {
                expected: Self::MIN_SIZE,
                actual: data.len(),
                context: "ProcDscInfo",
            });
        }
        // Keep all available data (structure is variable-length)
        Ok(Self { bytes: data })
    }

    /// Virtual address of the parent [`ObjectInfo`](super::object::ObjectInfo)
    /// structure at offset 0x00.
    ///
    /// Read the constant pool base via [`read_constants_va`]:
    /// ```ignore
    /// let oi_data = map.slice_from_va(pdi.object_info_va()?, OBJECT_INFO_MIN_SIZE)?;
    /// let const_va = read_constants_va(oi_data);
    /// ```
    ///
    /// `ProcCallEngine` (MSVBVM60 6.00.8176, 0x66104ada) dereferences this to
    /// access:
    /// - [`ObjectInfo::constants_va`](super::object::ObjectInfo::constants_va) (+0x34)
    /// - [`ObjectInfo::object_table_va`](super::object::ObjectInfo::object_table_va) (+0x04)
    ///
    /// In every procedure of the fixtures it is the ObjectInfo of the object
    /// the procedure belongs to.
    #[inline]
    pub fn object_info_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x00)
    }

    /// Bytes `ExitProc` pops on return, at offset 0x04, like a stdcall
    /// `retn N`.
    ///
    /// The shared exit path (MSVBVM60 6.00.8176, 0x661064a0) reads it and
    /// removes that many bytes above the return address. It counts:
    /// - the `ebp+8` slot: `Me` in an object's method, the module's data
    ///   block in a standard module's procedure (see [`pcode_frame::ME`]);
    /// - each parameter at its stack width: 4 bytes for a `ByRef` parameter
    ///   and the 4-byte types, 8 for a `ByVal` `Double`, `Date` or
    ///   `Currency`, 16 for a `ByVal` `Variant`;
    /// - 4 more for the return-value pointer of an object's `Function` or
    ///   `Property Get`.
    ///
    /// Fixture values: a module's `Function Add(a As Long, b As Long) As
    /// Long` 0x0C; a class's `Property Get Value() As Long` 0x08; `Function
    /// D(ByVal x As Date) As Date` 0x10; `Function V(ByVal x As Variant) As
    /// Variant` 0x18; a module's `Sub Main` 0x04.
    #[inline]
    pub fn arg_size(&self) -> Result<u16, Error> {
        read_u16_le(self.bytes, 0x04)
    }

    /// Stack frame size for local variables at offset 0x06.
    ///
    /// `ProcCallEngine` (MSVBVM60 6.00.8176, 0x66104b04..0x66104b3a)
    /// subtracts it from `esp` and zeroes the region (see
    /// [`zeroed_frame`](Self::zeroed_frame)).
    #[inline]
    pub fn frame_size(&self) -> Result<u16, Error> {
        read_u16_le(self.bytes, 0x06)
    }

    /// Returns the `ebp`-relative byte range `ProcCallEngine` zeroes before
    /// the procedure's first instruction: the locals, from
    /// `ebp - 0x84 - frame_size` up to the saved registers at `ebp - 0x84`.
    ///
    /// The engine clears `frame_size / 4` dwords (`rep stosd`, MSVBVM60
    /// 6.00.8176, 0x66104b3a) on every entry path, so a local read before
    /// any write is 0. The interpreter's own slots above it are not part of
    /// the range (see [`pcode_frame`] for their state on entry). Every
    /// fixture's frame size is a multiple of 4.
    ///
    /// # Errors
    ///
    /// Returns an error if the frame size cannot be read.
    pub fn zeroed_frame(&self) -> Result<Range<i32>, Error> {
        let frame_size = u32::from(self.frame_size()?);
        let zeroed = frame_size & !3;
        let start = 0i32
            .saturating_sub_unsigned(pcode_frame::HOUSEKEEPING_SIZE)
            .saturating_sub_unsigned(frame_size);
        Ok(start..start.saturating_add_unsigned(zeroed))
    }

    /// P-Code byte stream back-offset at offset 0x08.
    ///
    /// The P-Code bytes are located at `[addr - offset .. addr]`
    /// where `addr` is the address of this ProcDscInfo structure
    /// (`ProcCallEngine`, MSVBVM60 6.00.8176, 0x66104b7f).
    #[inline]
    pub fn pcode_back_offset(&self) -> Result<u16, Error> {
        read_u16_le(self.bytes, 0x08)
    }

    /// Same as [`pcode_back_offset`](Self::pcode_back_offset): the size of
    /// the procedure's P-Code in bytes.
    #[inline]
    pub fn proc_size(&self) -> Result<u16, Error> {
        self.pcode_back_offset()
    }

    /// Size of the header and the primary cleanup table at offset 0x0A,
    /// which is the offset of the secondary cleanup table.
    ///
    /// Equals `HEADER_SIZE (0x18) + ` [`cleanup_table_size`](Self::cleanup_table_size)
    /// rounded up to a multiple of 4 (0x90 for a 0x76-byte primary table).
    /// The runtime's error unwinding reads it to find the secondary table
    /// (MSVBVM60 6.00.8176, 0x66107ff9).
    #[inline]
    pub fn total_size(&self) -> Result<u16, Error> {
        read_u16_le(self.bytes, 0x0A)
    }

    /// Procedure option flags at offset 0x0C as a [`ProcOptFlags`] wrapper:
    /// how `ProcCallEngine` enters an object's method (see [`ProcOptFlags`]).
    #[inline]
    pub fn proc_opt_flags(&self) -> Result<ProcOptFlags, Error> {
        Ok(ProcOptFlags(read_u16_le(self.bytes, 0x0C)?))
    }

    /// Raw procedure option flags value at offset 0x0C.
    #[inline]
    pub fn proc_opt_flags_raw(&self) -> Result<u16, Error> {
        read_u16_le(self.bytes, 0x0C)
    }

    /// Returns `true` if this procedure is a `Friend` method (see
    /// [`ProcOptFlags::FRIEND`]).
    #[inline]
    pub fn is_friend(&self) -> bool {
        self.proc_opt_flags().is_ok_and(ProcOptFlags::is_friend)
    }

    /// Returns `true` if bit 0x20 is set ([`ProcOptFlags::ADJUSTED_ME_AS_PRIMARY`]):
    /// a `Class_Initialize` or an `Implements` member. Whether `Me` is
    /// `AddRef`ed on entry also depends on the thunk and the runtime context
    /// (see [`ProcOptFlags`]).
    #[inline]
    pub fn enters_adjusted_me_as_primary(&self) -> bool {
        self.proc_opt_flags()
            .is_ok_and(ProcOptFlags::enters_adjusted_me_as_primary)
    }

    /// Self-relative offset of the procedure's line-number table at offset
    /// 0x0E, or 0 when the procedure has no line numbers.
    ///
    /// The table is a `u16` count followed by that many (`u16` P-Code
    /// offset, `u16` line number) pairs. When an error occurs, the runtime
    /// looks up the last pair at or before the faulting P-Code offset and
    /// stores its line as `Erl` (MSVBVM60 6.00.8176, 0x66108075 and
    /// 0x6610d15a, which reads the field as signed). In the fixtures only
    /// `flow`'s `ErrLines` (numbered lines 10 to 60) has one: offset 0x44,
    /// six pairs, placed right after the secondary cleanup table, outside
    /// [`actual_size`](Self::actual_size).
    #[inline]
    pub fn line_table_offset(&self) -> Result<i16, Error> {
        Ok(read_u16_le(self.bytes, 0x0E)?.cast_signed())
    }

    /// Returns the procedure's line numbers: each numbered line's P-Code
    /// offset and number, from the table at
    /// [`line_table_offset`](Self::line_table_offset); empty for a
    /// procedure without line numbers or whose table is not in the parsed
    /// slice.
    pub fn line_numbers(&self) -> Vec<LineNumber> {
        let Some(start) = self
            .line_table_offset()
            .ok()
            .filter(|&offset| offset > 0)
            .and_then(|offset| usize::try_from(offset).ok())
        else {
            return Vec::new();
        };
        let Ok(count) = read_u16_le(self.bytes, start) else {
            return Vec::new();
        };
        (0..usize::from(count))
            .map_while(|index| {
                let at = start.checked_add(2)?.checked_add(index.checked_mul(4)?)?;
                Some(LineNumber {
                    offset: read_u16_le(self.bytes, at).ok()?,
                    line: read_u16_le(self.bytes, at.checked_add(2)?).ok()?,
                })
            })
            .collect()
    }

    /// `Resume Next` fixup-table offset at offset 0x10.
    ///
    /// Self-relative offset from the start of `ProcDscInfo` to a per-procedure
    /// fixup table consulted when `Resume Next` resumes after a statement
    /// that does not start with a statement marker.
    ///
    /// # How `Resume Next` uses it
    ///
    /// The `Resume` handler (MSVBVM60 6.00.8176, 0x6610b0b6) dispatches on its
    /// operand: a label offset jumps there; `0xFFFE` (`Resume`) re-executes the
    /// statement the handler is handling (`[ebp-0x3C]`); `0xFFFF` (`Resume
    /// Next`) skips it. When that statement starts with `LargeBos` (opcode
    /// 0x00), the skip is the marker's inline length byte; otherwise the
    /// handler reads this field, and the `u16` at `ProcDscInfo + offset + 2 +
    /// 2 * k`, where `k` is the statement's second byte, is the length. The
    /// error dispatcher's `On Error Resume Next` path does the same
    /// (0x6610808e, which reads the field as signed).
    ///
    /// The field is 0 in all 370 P-Code procedures of the fixtures, including
    /// `flow`'s procedures with `Resume Next` and `On Error Resume Next` (VB
    /// marks their statements with `LargeBos`). No fixture has a table, so its
    /// extent is unconfirmed; like the line-number table of
    /// [`line_table_offset`](Self::line_table_offset) it would lie outside
    /// [`actual_size`](Self::actual_size).
    #[inline]
    pub fn resume_fixup_table_offset(&self) -> Result<u16, Error> {
        read_u16_le(self.bytes, 0x10)
    }

    /// Unexplained `u16` at offset 0x12.
    ///
    /// In the fixtures (built by VB6 6.00.8176) it is 0x26 in 176 of the 183
    /// procedures, in modules, classes, forms and UserControls alike, and 0
    /// in the 7 procedures that have an `Optional` parameter with a default
    /// value or a `For Each` loop. It is not `(initialize_event_offset / 4) -
    /// 1` (2 for these classes, 25 for these forms). No read of it was found
    /// in `ProcCallEngine`.
    #[inline]
    pub fn base_iface_slot_count(&self) -> Result<u16, Error> {
        read_u16_le(self.bytes, 0x12)
    }

    /// Reserved field at offset 0x14 (0 in every procedure of the fixtures).
    #[inline]
    pub fn reserved_14(&self) -> Result<u16, Error> {
        read_u16_le(self.bytes, 0x14)
    }

    /// Reserved field at offset 0x16 (0 in every procedure of the fixtures).
    #[inline]
    pub fn reserved_16(&self) -> Result<u16, Error> {
        read_u16_le(self.bytes, 0x16)
    }

    /// Size of the primary cleanup table at offset 0x18.
    ///
    /// The table starts at ProcDscInfo +0x18 and extends for this many
    /// bytes. Minimum value is 0x0C (header only, no entries). It need not
    /// be a multiple of 4.
    ///
    /// [`total_size`](Self::total_size) = 0x18 + `cleanup_table_size`,
    /// rounded up to a multiple of 4.
    #[inline]
    pub fn cleanup_table_size(&self) -> Result<u16, Error> {
        read_u16_le(self.bytes, 0x18)
    }

    /// Reserved field at offset 0x1A (the primary table's +0x02; 0 in every
    /// procedure of the fixtures).
    #[inline]
    pub fn reserved_1a(&self) -> Result<u16, Error> {
        read_u16_le(self.bytes, 0x1A)
    }

    /// Number of leading primary-table entries the runtime initializes on
    /// entry, at offset 0x1C (see [`CleanupTable::count`]).
    ///
    /// `ProcCallEngine` (MSVBVM60 6.00.8176, 0x66104b44) calls the table
    /// initializer only when it is non-zero.
    #[inline]
    pub fn cleanup_count(&self) -> Result<u16, Error> {
        read_u16_le(self.bytes, 0x1C)
    }

    /// Total number of primary cleanup entries at offset 0x1E.
    ///
    /// Every entry is released on exit (see [`CleanupTable::total`]);
    /// [`cleanup_count`](Self::cleanup_count) only limits initialization.
    #[inline]
    pub fn cleanup_total(&self) -> u16 {
        if self.bytes.len() > 0x1F {
            read_u16_le(self.bytes, 0x1E).unwrap_or(0)
        } else {
            0
        }
    }

    /// Returns `true` if the primary cleanup table has entries: locals
    /// needing cleanup, or a String, Object, Variant or array return value.
    #[inline]
    pub fn has_cleanup(&self) -> bool {
        self.cleanup_count().unwrap_or(0) > 0 || self.cleanup_total() > 0
    }

    /// Returns the primary [`CleanupTable`] (at ProcDscInfo +0x18).
    ///
    /// The runtime initializes its first [`CleanupTable::count`] entries on
    /// entry and releases its entries on exit. It describes the local
    /// variables needing cleanup (strings, COM objects, SafeArrays, etc.) and
    /// a String, Object, Variant or array return value.
    pub fn cleanup_table(&self) -> Option<CleanupTable<'a>> {
        let offset = Self::HEADER_SIZE; // 0x18
        let min_len = offset.checked_add(CleanupTable::HEADER_SIZE)?;
        if self.bytes.len() > min_len {
            CleanupTable::parse(self.bytes.get(offset..)?)
        } else {
            None
        }
    }

    /// Returns the secondary [`CleanupTable`] that follows the primary table.
    ///
    /// This table has the same header format as the primary table and is
    /// always present (minimum 0x0C bytes). It lists the procedure's
    /// temporaries, the frame slots the `FFree*` opcodes release (`late`:
    /// one Variant at `ebp-0x98`, freed by `FFree1Var var_98`). The runtime
    /// does not touch it on entry or on a normal exit; when an error unwinds
    /// the procedure it releases every entry (MSVBVM60 6.00.8176,
    /// 0x66107ff6..0x6610800e).
    ///
    /// Located at `ProcDscInfo + total_size`, after the primary cleanup
    /// table and its padding to a multiple of 4.
    pub fn secondary_table(&self) -> Option<CleanupTable<'a>> {
        let offset = self.total_size().ok()? as usize;
        let min_len = offset.checked_add(CleanupTable::HEADER_SIZE)?;
        if offset >= Self::HEADER_SIZE && self.bytes.len() > min_len {
            CleanupTable::parse(self.bytes.get(offset..)?)
        } else {
            None
        }
    }

    /// Actual total size of the ProcDscInfo structure including both
    /// cleanup tables.
    ///
    /// This is [`total_size`](Self::total_size) (the header, the primary
    /// table and its padding) plus the secondary table's size. It does not
    /// include a line-number table ([`line_table_offset`](Self::line_table_offset)) or a
    /// `Resume Next` fixup table
    /// ([`resume_fixup_table_offset`](Self::resume_fixup_table_offset)),
    /// which follow the secondary table: `flow`'s `ErrLines` has an
    /// `actual_size` of 0x44 and its line table ends at 0x5E. Neither is the
    /// result padded: the next procedure's P-Code, when it follows, starts
    /// at the next multiple of 4 after the last table.
    ///
    /// Note: [`total_size`](Self::total_size) at offset +0x0A only covers
    /// the header and primary table. This method accounts for both tables.
    pub fn actual_size(&self) -> Result<usize, Error> {
        let base = self.total_size()? as usize;
        if let Some(secondary) = self.secondary_table() {
            let sec_size = secondary.size()? as usize;
            base.checked_add(sec_size).ok_or(Error::ArithmeticOverflow {
                context: "ProcDscInfo::actual_size base + secondary",
            })
        } else {
            Ok(base)
        }
    }

    /// Returns the bytes the descriptor occupies with the tables that follow
    /// it: [`actual_size`](Self::actual_size), or the end of its line-number
    /// table when it has one.
    ///
    /// # Errors
    ///
    /// Returns an error if a table's size or the line-number count cannot be
    /// read.
    pub fn extent_size(&self) -> Result<usize, Error> {
        let base = self.actual_size()?;
        let Some(start) = usize::try_from(self.line_table_offset()?)
            .ok()
            .filter(|&start| start != 0)
        else {
            return Ok(base);
        };
        let count = usize::from(read_u16_le(self.bytes, start)?);
        let end = count
            .checked_mul(4)
            .and_then(|pairs| pairs.checked_add(2))
            .and_then(|len| len.checked_add(start))
            .ok_or(Error::ArithmeticOverflow {
                context: "ProcDscInfo::extent_size line table",
            })?;
        Ok(base.max(end))
    }

    /// Returns an iterator over primary cleanup table entries.
    ///
    /// Each entry describes a local variable that needs resource release
    /// (string, COM object, SafeArray, etc.) on procedure exit or error.
    /// Entry types reuse [`ControlPropertyType`](super::controlprop::ControlPropertyType).
    ///
    /// Note: `frame_offset` in cleanup entries is a **signed i16** (negative
    /// offset from EBP), unlike instance data entries which use unsigned offsets.
    pub fn cleanup_entries(&self) -> ControlPropertyIter<'a> {
        match self.cleanup_table() {
            Some(table) => table.entries(),
            None => ControlPropertyIter::new(&[], 0),
        }
    }

    /// Returns the number of stack dwords `ExitProc` pops (`arg_size / 4`).
    ///
    /// This is not the parameter count: it includes the `ebp+8` slot and an
    /// object method's return-value pointer, and a `ByVal` `Double`, `Date`
    /// or `Currency` takes 2 dwords and a `ByVal` `Variant` 4 (see
    /// [`arg_size`](Self::arg_size)).
    #[inline]
    pub fn arg_count(&self) -> Result<u16, Error> {
        Ok(self.arg_size()? / 4)
    }
}

/// Offset within ObjectInfo where the constant pool VA is stored.
///
/// This is `ObjectInfo.lpConstants` (+0x34). ProcDscInfo.object_info_va()?
/// points to ObjectInfo, and we read the constant pool base from +0x34.
pub const OBJECT_INFO_CONSTANTS_OFFSET: usize = 0x34;

/// Minimum bytes needed from ObjectInfo to read the constants VA.
pub const OBJECT_INFO_MIN_SIZE: usize = OBJECT_INFO_CONSTANTS_OFFSET + 4;

/// Reads the constant pool base VA from ObjectInfo data.
#[inline]
pub fn read_constants_va(object_info_data: &[u8]) -> Result<u32, Error> {
    read_u32_le(object_info_data, OBJECT_INFO_CONSTANTS_OFFSET)
}

/// Procedure option flags from `ProcDscInfo` offset 0x0C.
///
/// `ProcCallEngine` (MSVBVM60 6.00.8176, 0x66104b99) tests two bits when it
/// enters an object's method (through `MethCallEngine`, 0x661080b8):
///
/// | Bit | Meaning | Seen on (fixtures) |
/// |-----|---------|--------------------|
/// | 0x10 ([`FRIEND`](Self::FRIEND)) | `Me` must be an instance of the defining class: error 91 when it is `Nothing`, 97 ("Can not call friend function on object which is not an instance of defining class") when its vtable is another's | the `Friend` methods `Ring.Diameter` (`events`) and `Kinds.Rec` (`types`) |
/// | 0x20 ([`ADJUSTED_ME_AS_PRIMARY`](Self::ADJUSTED_ME_AS_PRIMARY)) | Selects how `Me` is referenced on entry (below) | `Class_Initialize`, the members an `Implements` provides (`calls`: `Square.Shape_*`; `events`: `Ring.Measure_*`) |
///
/// The method's thunk (`xor eax, eax; mov edx, <ProcDscInfo>; push
/// <MethCallEngine>; ret`) passes a `Me` adjustment in `eax`, which
/// `MethCallEngine` subtracts from `Me`. With bit 0x20 clear and a non-zero
/// adjustment, `ProcCallEngine` `AddRef`s `Me` and sets the frame flags
/// `[ebp-0x48]` to 0xE000. Otherwise (bit 0x20 set, or an adjustment of 0)
/// it `AddRef`s `Me` with frame flags 0xC000, unless the runtime context's
/// byte +0x74 has bit 0 set and bit 1 clear: then it clears those bits and
/// enters with frame flags 0x4000 and no `AddRef`. `ExitProc` releases `Me`
/// when the frame flags have 0x8000. Every thunk in the fixtures loads
/// `eax` = 0, so there the bit does not change the entry path.
///
/// Neither bit is about error handling: the procedures of
/// `tests/fixtures/flow` with `On Error GoTo`, `Resume` and `On Error Resume
/// Next` have no bit set. An error handler shows in the code
/// (`OnErrorGoto`).
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProcOptFlags(pub u16);

impl ProcOptFlags {
    /// A `Friend` method: the runtime checks `Me` is an instance of the
    /// defining class.
    pub const FRIEND: u16 = 0x10;
    /// Set on `Class_Initialize` and on `Implements` members: `ProcCallEngine`
    /// does not take the adjusted-`Me` `AddRef` path (see [`ProcOptFlags`]).
    pub const ADJUSTED_ME_AS_PRIMARY: u16 = 0x20;

    /// Tests whether the given flag bit(s) are set.
    #[inline]
    pub fn has(self, flag: u16) -> bool {
        self.0 & flag != 0
    }

    /// Returns `true` for a `Friend` method ([`FRIEND`](Self::FRIEND)).
    #[inline]
    pub fn is_friend(self) -> bool {
        self.has(Self::FRIEND)
    }

    /// Returns `true` if bit 0x20 ([`ADJUSTED_ME_AS_PRIMARY`](Self::ADJUSTED_ME_AS_PRIMARY)) is
    /// set.
    #[inline]
    pub fn enters_adjusted_me_as_primary(self) -> bool {
        self.has(Self::ADJUSTED_ME_AS_PRIMARY)
    }
}

impl fmt::Debug for ProcOptFlags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ProcOptFlags(0x{:02X}", self.0)?;
        if self.is_friend() {
            write!(f, " FRIEND")?;
        }
        if self.enters_adjusted_me_as_primary() {
            write!(f, " ADJUSTED_ME_AS_PRIMARY")?;
        }
        write!(f, ")")
    }
}

impl fmt::Display for ProcOptFlags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

/// A numbered source line: where its code starts and its number, which the
/// runtime reports as `Erl` for an error in it. Returned by
/// [`ProcDscInfo::line_numbers`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineNumber {
    /// The P-Code offset of the line's first instruction.
    pub offset: u16,
    /// The line number.
    pub line: u16,
}

/// P-Code runtime stack frame layout (housekeeping region).
///
/// When `ProcCallEngine` enters a P-Code procedure it establishes an x86
/// frame with 0x84 bytes of runtime housekeeping below `ebp`; the
/// procedure's locals (`ProcDscInfo.wFrameSize` bytes) lie below that, and
/// the evaluation stack is the native stack below them. Opcode handlers
/// reach the slots as `[ebp - N]`.
///
/// The frame is NOT stored in the PE file - it exists only at runtime. Only
/// the slots the opcode handlers of MSVBVM60 6.00.8176 and 6.00.9848 were
/// seen to use are named (handler addresses are 8176's).
///
/// # Stack Layout (high to low addresses)
///
/// ```text
/// ebp+0x0C..       arguments, each as wide as its type (4, 8 or 16 bytes)
/// ebp+0x08         Me, for a class, form or control method (FLdPrThis);
///                  in a standard module's procedure, the module's data
///                  block (`ProcCallEngine` inserts it, 0x66104a99)
/// ebp+0x04         return address
/// ebp              saved ebp
/// ebp-0x14         start of the current statement (LargeBos, 0x66104bfe)
/// ebp-0x1C         evaluation stack pointer GoSub/Return leave (0x66104d96)
/// ebp-0x34         GoSub nesting depth (GoSub / Return)
/// ebp-0x3C         statement an error handler is handling (Resume, 0x6610b0b6)
/// ebp-0x40         error handler: 0 none, -1 Resume Next, else its address
///                  (OnErrorGoto, 0x66105e46)
/// ebp-0x44         runtime context (+0x78 error info, +0x98 Erl, the line
///                  number of the last error, +0x9C Err.LastDllError)
/// ebp-0x48         frame flags (ExitProc releases Me when 0x8000 is set)
/// ebp-0x4C         Pr, the object register of the Mem* / VCall* opcodes
/// ebp-0x50         the procedure's ProcDscInfo (ExitProc reads its +4)
/// ebp-0x54         constant pool base (`%s` / `%c` operands: pool + 4 * index)
/// ebp-0x58         P-Code base (`%l` targets: base + offset)
/// ebp-0x68         handler scratch
/// ebp-0x6C         the engine's exit routine, set on entry: 0x66105ddf by
///                  ProcCallEngine, 0x66105e14 by MethCallEngine
/// ebp-0x7C..-0x84  saved ebx, esi, edi (ExitProc restores them from ebp-0x84)
/// ebp-0x84-n..     locals, wFrameSize bytes; a function's return value is at
///                  their top: an Integer at ebp-0x86, a Long, String or
///                  object at ebp-0x88, a Double or Currency at ebp-0x8C
/// ```
///
/// # State on entry
///
/// Before the first instruction `ProcCallEngine` (0x66104b04..0x66104b3f)
/// zeroes the locals ([`ProcDscInfo::zeroed_frame`]), so every local,
/// including a function's return value, starts as 0 (an empty String, a
/// `Nothing` object, an `Empty` Variant). A frame below the thread's stack
/// limit raises error 28 (Out of stack space) instead. Then, when
/// [`ProcDscInfo::cleanup_count`] is non-zero, the cleanup table's
/// initializer writes the descriptors of the fixed-size arrays and UDTs it
/// lists over those zeroes.
///
/// Of the housekeeping slots, the error handler (ebp-0x40), the handled
/// statement (ebp-0x3C) and the GoSub depth (ebp-0x34) start at 0; the frame
/// flags (ebp-0x48) at 0, or at 0x4000, 0xC000 or 0xE000 when the
/// procedure is a method entered through `MethCallEngine`; the
/// ProcDscInfo, constant pool, P-Code base, runtime context and exit routine
/// slots hold their values. Pr (ebp-0x4C) is not written: it holds
/// whatever the stack held until the procedure's first `FLdPr`-family
/// instruction loads it.
pub mod pcode_frame {
    /// Offset of `Me` from EBP in an object's method.
    ///
    /// A standard module's procedure has the module's data block in this
    /// slot instead: `ProcCallEngine` (0x66104a99) pops the return address,
    /// pushes `PublicObjectDescriptor.lpModulePublic` of the procedure's
    /// object and pushes the return address back, so every P-Code procedure
    /// has the same frame shape.
    pub const ME: i32 = 0x08;
    /// Offset of the first explicit argument from EBP, in every P-Code
    /// procedure (an object's method or a standard module's procedure).
    pub const FIRST_ARG: i32 = 0x0C;
    /// Offset of the current statement's start (written by `LargeBos`).
    pub const STATEMENT_IP: i32 = -0x14;
    /// Offset of the evaluation stack pointer `GoSub` and `Return` record.
    pub const STATEMENT_ESP: i32 = -0x1C;
    /// Offset of the `GoSub` nesting depth.
    pub const GOSUB_DEPTH: i32 = -0x34;
    /// Offset of the statement an error handler is handling (`Resume` target).
    pub const ERROR_STATEMENT: i32 = -0x3C;
    /// Offset of the error handler: 0 none, -1 `Resume Next`, else its address.
    pub const ERROR_HANDLER: i32 = -0x40;
    /// Offset of the runtime context pointer.
    pub const ENGINE_CONTEXT: i32 = -0x44;
    /// Offset of the frame flags.
    pub const FRAME_FLAGS: i32 = -0x48;
    /// Offset of Pr, the object register.
    pub const OBJECT_REGISTER: i32 = -0x4C;
    /// Offset of the procedure's ProcDscInfo pointer.
    pub const PROC_DSC_INFO: i32 = -0x50;
    /// Offset of the constant pool base.
    pub const CONST_POOL: i32 = -0x54;
    /// Offset of the P-Code base that jump targets are relative to.
    pub const CODE_BASE: i32 = -0x58;
    /// Total size of the housekeeping region (bytes above the locals).
    pub const HOUSEKEEPING_SIZE: u32 = 0x84;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proc_dsc_info_parse() {
        let mut data = vec![0u8; ProcDscInfo::MIN_SIZE];
        data[0x04..0x06].copy_from_slice(&0x0010u16.to_le_bytes()); // arg_size = 16
        data[0x06..0x08].copy_from_slice(&0x0100u16.to_le_bytes()); // frame_size = 256
        data[0x08..0x0A].copy_from_slice(&0x0050u16.to_le_bytes()); // pcode_back_offset = 80
        data[0x0A..0x0C].copy_from_slice(&0x0024u16.to_le_bytes()); // total_size = 36
        data[0x18..0x1A].copy_from_slice(&0x000Cu16.to_le_bytes()); // cleanup_table_size = 12
        data[0x1C..0x1E].copy_from_slice(&0x0000u16.to_le_bytes()); // cleanup_count = 0
        let pdi = ProcDscInfo::parse(&data).unwrap();
        assert_eq!(pdi.arg_size().unwrap(), 0x0010);
        assert_eq!(pdi.arg_count().unwrap(), 4);
        assert_eq!(pdi.frame_size().unwrap(), 0x0100);
        assert_eq!(pdi.zeroed_frame().unwrap(), -0x184..-0x84);
        assert_eq!(pdi.pcode_back_offset().unwrap(), 0x0050);
        assert_eq!(pdi.proc_size().unwrap(), 0x0050); // legacy alias
        assert_eq!(pdi.total_size().unwrap(), 0x0024);
        assert_eq!(pdi.cleanup_table_size().unwrap(), 0x000C);
        assert_eq!(pdi.cleanup_count().unwrap(), 0);
        assert!(!pdi.has_cleanup());
    }

    #[test]
    fn test_proc_dsc_info_with_error_handler() {
        let mut data = vec![0u8; ProcDscInfo::MIN_SIZE];
        data[0x0A..0x0C].copy_from_slice(&0x0054u16.to_le_bytes()); // total_size = 84
        data[0x18..0x1A].copy_from_slice(&0x003Cu16.to_le_bytes()); // cleanup_table_size = 60
        data[0x1C..0x1E].copy_from_slice(&0x0001u16.to_le_bytes()); // cleanup_count = 1
        let pdi = ProcDscInfo::parse(&data).unwrap();
        assert_eq!(pdi.total_size().unwrap(), 0x0054);
        assert_eq!(pdi.cleanup_table_size().unwrap(), 0x003C);
        assert_eq!(pdi.cleanup_count().unwrap(), 1);
        assert!(pdi.has_cleanup());
        // Verify: total = header(0x18) + err_table(0x3C) = 0x54
        assert_eq!(
            ProcDscInfo::HEADER_SIZE as u16 + pdi.cleanup_table_size().unwrap(),
            pdi.total_size().unwrap()
        );
    }

    #[test]
    fn test_proc_dsc_info_too_short() {
        let data = vec![0u8; ProcDscInfo::MIN_SIZE - 1];
        assert!(matches!(
            ProcDscInfo::parse(&data),
            Err(Error::TooShort { .. })
        ));
    }

    #[test]
    fn test_proc_dsc_info_all_fields() {
        let data = vec![0u8; ProcDscInfo::MIN_SIZE];
        let pdi = ProcDscInfo::parse(&data).unwrap();
        let _ = pdi.object_info_va().unwrap();
        let _ = pdi.arg_size().unwrap();
        let _ = pdi.arg_count().unwrap();
        let _ = pdi.pcode_back_offset().unwrap();
        let _ = pdi.total_size().unwrap();
        let _ = pdi.proc_opt_flags().unwrap();
        let _ = pdi.line_table_offset().unwrap();
        let _ = pdi.resume_fixup_table_offset().unwrap();
        let _ = pdi.base_iface_slot_count().unwrap();
        let _ = pdi.reserved_14().unwrap();
        let _ = pdi.reserved_16().unwrap();
        let _ = pdi.cleanup_table_size().unwrap();
        let _ = pdi.reserved_1a().unwrap();
        let _ = pdi.cleanup_count().unwrap();
        let _ = pdi.has_cleanup();
    }

    #[test]
    fn test_read_constants_va() {
        let mut data = vec![0u8; OBJECT_INFO_MIN_SIZE];
        data[0x34..0x38].copy_from_slice(&0x00405000u32.to_le_bytes());
        assert_eq!(read_constants_va(&data).unwrap(), 0x00405000);
    }

    // Real data from vb_inject sample, method_2B
    #[test]
    fn test_real_method_2b() {
        let data: [u8; 0x1E] = [
            0xD8, 0x21, 0x41, 0x00, // +0x00: lpObjectInfo = 0x004121D8
            0x10, 0x00, // +0x04: wArgSize = 16
            0x08, 0x00, // +0x06: wFrameSize = 8
            0x08, 0x00, // +0x08: wPCodeBackOffset = 8
            0x24, 0x00, // +0x0A: wTotalSize = 36
            0x00, 0x00, // +0x0C: wProcOptFlags = 0
            0x00, 0x00, // +0x0E
            0x00, 0x00, // +0x10
            0x19, 0x00, // +0x12: 25 (constant per object)
            0x00, 0x00, // +0x14
            0x00, 0x00, // +0x16
            0x0C, 0x00, // +0x18: wErrTableSize = 12
            0x00, 0x00, // +0x1A
            0x00, 0x00, // +0x1C: wErrBranchCount = 0
        ];
        let pdi = ProcDscInfo::parse(&data).unwrap();
        assert_eq!(pdi.object_info_va().unwrap(), 0x004121D8);
        assert_eq!(pdi.arg_size().unwrap(), 16);
        assert_eq!(pdi.arg_count().unwrap(), 4);
        assert_eq!(pdi.frame_size().unwrap(), 8);
        assert_eq!(pdi.pcode_back_offset().unwrap(), 8);
        assert_eq!(pdi.total_size().unwrap(), 0x24);
        assert!(!pdi.has_cleanup());
        // Verify: total = 0x18 + err_table(0x0C) = 0x24
        assert_eq!(
            0x18 + pdi.cleanup_table_size().unwrap(),
            pdi.total_size().unwrap()
        );
    }
}
