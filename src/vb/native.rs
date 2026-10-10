//! Structures of natively compiled procedures.
//!
//! A native procedure that releases anything when it unwinds (an error
//! raised inside it, or in what it calls), or that handles errors, stores
//! the VA of a [`ProcUnwindInfo`] in its frame in its prologue, after
//! installing `__vbaExceptHandler` as its SEH handler:
//!
//! ```x86asm
//! push    ebp
//! mov     ebp, esp
//! sub     esp, ...
//! push    offset __vbaExceptHandler
//! mov     eax, fs:[0]
//! push    eax
//! mov     fs:[0], esp
//! ...
//! mov     [ebp-8], esp
//! mov     dword ptr [ebp-4], offset ProcUnwindInfo   ; C7 45 FC imm32
//! ```
//!
//! The record names the frame slot it is stored in: `ebp-4`, `ebp-8` in a
//! class method and in some module procedures (`C7 45 F8 imm32`), `ebp-0x10`
//! in a procedure with an `On Error GoTo` handler (`C7 45 F0 imm32`), and
//! `ebp-0x14` in one that also uses `Resume`, `Resume Next`,
//! `On Error Resume Next` or `Erl` (`C7 45 EC imm32`), which numbers its
//! statements in `ebp-4` as it runs them. The words between the slot and the
//! registration are the handler's state: `ebp-slot+4` its flags,
//! `ebp-slot+8` the number of the active handler (`__vbaOnError`'s
//! argument).

use std::ops::Range;

use crate::{
    addressmap::AddressMap,
    error::Error,
    util::{read_u16_le, read_u32_le},
};

/// The unwind record of a native procedure (8, 16, 24 or 28 bytes): the
/// code that releases what the procedure holds when it unwinds, and the
/// tables its error handling reads.
///
/// | Offset | Size | Field |
/// |--------|------|-------|
/// | 0x00 | 2 | Flags: bit 0x01 [`exit_va`](Self::exit_va), bit 0x02 [`locals_va`](Self::locals_va), bits 0x04 or 0x08 [`temps_va`](Self::temps_va), bit 0x10 [`handlers_va`](Self::handlers_va), bit 0x20 [`resume_va`](Self::resume_va), bit 0x40 [`lines_va`](Self::lines_va); bit 0x80 set on some class methods (`events-native`), meaning unmeasured |
/// | 0x02 | 2 | Frame slot: the record's VA is stored at `ebp - slot` (4, 8, 0x10 or 0x14) |
/// | 0x04 | 4 | Exit block VA (bit 0x01), else 0 |
/// | 0x08 | 4 | Locals routine VA (bit 0x02), else 0 |
/// | 0x0C | 4 | Temporaries routine VA (bit 0x04 or 0x08), else 0 |
/// | 0x10 | 4 | [`ErrorHandlerTable`] VA (bit 0x10), else 0 |
/// | 0x14 | 4 | [`ResumeTable`] VA (bit 0x20), else 0 |
/// | 0x18 | 4 | [`LineNumberTable`] VA (bit 0x40) |
///
/// The record runs to the last field its flags use: 28 bytes with bit 0x40,
/// 24 with bit 0x10 or 0x20, 16 with any of bits 0x0E, else 8 (a class
/// method that holds only `Me`: `events-native` `Source`'s methods, flags
/// 0x01). Its tables follow it in the order resume, handlers, lines, each
/// at the next multiple of 8.
///
/// The runtime enters every address the record names in the procedure's
/// own frame, with `ebp` and `esp` restored from it. The locals routine
/// (freeing the procedure's `String`, object and `Variant` variables:
/// `__vbaFreeStr`, `__vbaFreeObjList`, `__vbaFreeVarList`) and the
/// temporaries routine (freeing the current statement's) are called, and
/// end in `ret`; the runtime calls the temporaries routine on every unwind
/// through the procedure with bit 0x04, only on the one that leaves it with
/// bit 0x08, and the locals routine when the unwind leaves it. The exit
/// block, a handler and a `Resume` target are jumped to. The exit block is
/// a class method's epilogue, which its own code also runs into: it
/// releases `Me` (`mov eax, [ebp+8]; push eax; call [ecx+8]`) and returns
/// the word at `ebp-slot+4`, where the runtime stores the error when one
/// leaves the method with no handler active. The records sit together in
/// `.text`, among the native code's floating-point constants, in no table.
#[derive(Clone, Copy, Debug)]
pub struct ProcUnwindInfo<'a> {
    bytes: &'a [u8],
}

impl<'a> ProcUnwindInfo<'a> {
    /// How far into a procedure [`from_prologue`](Self::from_prologue)
    /// looks for the store of the record's VA.
    pub const PROLOGUE_SCAN: usize = 0x60;

    /// The frame slots a record can name: `ebp-4`, `ebp-8`, `ebp-0x10` and
    /// `ebp-0x14`.
    pub const FRAME_SLOTS: [u16; 4] = [0x04, 0x08, 0x10, 0x14];

    /// Parses an unwind record from the given byte slice.
    ///
    /// # Errors
    ///
    /// Returns [`Error::TooShort`] if `data` is shorter than the record its
    /// flags call for.
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        let flags = read_u16_le(data, 0)?;
        let size = if flags & 0x40 != 0 {
            0x1C
        } else if flags & 0x30 != 0 {
            0x18
        } else if flags & 0x0E != 0 {
            0x10
        } else {
            0x08
        };
        let bytes = data.get(..size).ok_or(Error::TooShort {
            expected: size,
            actual: data.len(),
            context: "ProcUnwindInfo",
        })?;
        Ok(Self { bytes })
    }

    /// Reads the unwind record of the native procedure that starts at
    /// `proc_va` with `push ebp; mov ebp, esp` (`55 8B EC`): the first
    /// `mov [ebp-slot], imm32` (`C7 45 disp8 imm32`) in its first
    /// [`PROLOGUE_SCAN`](Self::PROLOGUE_SCAN) bytes, `slot` one of
    /// [`FRAME_SLOTS`](Self::FRAME_SLOTS), whose immediate is a record
    /// naming that slot. Returns the record's VA and the record; `None` if
    /// the code is not such a procedure or it stores none.
    pub fn from_prologue(map: &AddressMap<'a>, proc_va: u32) -> Option<(u32, Self)> {
        let code = map.slice_from_va(proc_va, 3).ok()?;
        if !code.starts_with(&[0x55, 0x8B, 0xEC]) {
            return None;
        }
        let code = code.get(..code.len().min(Self::PROLOGUE_SCAN))?;
        code.windows(7)
            .find_map(|window| Self::stored_by(map, window))
    }

    /// The record `instruction` stores, when it is a `mov [ebp-slot],
    /// imm32` whose immediate is a record naming that slot.
    fn stored_by(map: &AddressMap<'a>, instruction: &[u8]) -> Option<(u32, Self)> {
        let [0xC7, 0x45, disp, a, b, c, d] = *instruction else {
            return None;
        };
        let slot = 0x100u16.checked_sub(u16::from(disp))?;
        if !Self::FRAME_SLOTS.contains(&slot) {
            return None;
        }
        let va = u32::from_le_bytes([a, b, c, d]);
        let info = Self::parse(map.slice_from_va(va, 8).ok()?).ok()?;
        (info.frame_slot().ok()? == slot).then_some((va, info))
    }

    /// Size of the record: 28, 24, 16 or 8 bytes, by its flags.
    #[inline]
    pub fn size(&self) -> usize {
        self.bytes.len()
    }

    /// The flags at offset 0x00.
    #[inline]
    pub fn flags(&self) -> Result<u16, Error> {
        read_u16_le(self.bytes, 0x00)
    }

    /// The frame slot at offset 0x02: the record's VA is at `ebp - slot`.
    #[inline]
    pub fn frame_slot(&self) -> Result<u16, Error> {
        read_u16_le(self.bytes, 0x02)
    }

    /// Reads the VA at `offset` when flag `mask` is set.
    fn va_at(&self, mask: u16, offset: usize) -> Option<u32> {
        if self.flags().ok()? & mask == 0 {
            return None;
        }
        read_u32_le(self.bytes, offset).ok().filter(|&va| va != 0)
    }

    /// VA of a class method's exit block (flag 0x01): its epilogue, which
    /// returns the error the runtime stores when one leaves the method.
    pub fn exit_va(&self) -> Option<u32> {
        self.va_at(0x01, 0x04)
    }

    /// VA of the routine that frees the procedure's variables (flag 0x02).
    pub fn locals_va(&self) -> Option<u32> {
        self.va_at(0x02, 0x08)
    }

    /// VA of the routine that frees the current statement's temporaries
    /// (flag 0x04 or 0x08).
    pub fn temps_va(&self) -> Option<u32> {
        self.va_at(0x0C, 0x0C)
    }

    /// VA of the procedure's [`ErrorHandlerTable`] (flag 0x10): the labels
    /// its `On Error GoTo` statements name.
    pub fn handlers_va(&self) -> Option<u32> {
        self.va_at(0x10, 0x10)
    }

    /// VA of the procedure's [`ResumeTable`] (flag 0x20): its statements'
    /// addresses, for `Resume` and `Resume Next`.
    pub fn resume_va(&self) -> Option<u32> {
        self.va_at(0x20, 0x14)
    }

    /// VA of the procedure's [`LineNumberTable`] (flag 0x40): its
    /// statements' line numbers, for `Erl`.
    pub fn lines_va(&self) -> Option<u32> {
        self.va_at(0x40, 0x18)
    }

    /// Reads the procedure's [`ErrorHandlerTable`], when it has one.
    pub fn handlers(&self, map: &AddressMap<'a>) -> Option<ErrorHandlerTable<'a>> {
        let va = self.handlers_va()?;
        ErrorHandlerTable::parse(map.slice_from_va(va, 4).ok()?).ok()
    }

    /// Reads the procedure's [`ResumeTable`], when it has one.
    pub fn resume(&self, map: &AddressMap<'a>) -> Option<ResumeTable<'a>> {
        let va = self.resume_va()?;
        ResumeTable::parse(map.slice_from_va(va, 4).ok()?).ok()
    }

    /// Reads the procedure's [`LineNumberTable`], when it has one.
    pub fn lines(&self, map: &AddressMap<'a>) -> Option<LineNumberTable<'a>> {
        let va = self.lines_va()?;
        LineNumberTable::parse(map.slice_from_va(va, 2).ok()?).ok()
    }

    /// Every code address the record names: the exit block, the two
    /// routines, each handler and each statement a `Resume` can go to.
    pub fn code_vas(&self, map: &AddressMap<'a>) -> Vec<u32> {
        let mut code: Vec<u32> = [self.exit_va(), self.locals_va(), self.temps_va()]
            .into_iter()
            .flatten()
            .collect();
        if let Some(handlers) = self.handlers(map) {
            code.extend(handlers.iter().map(|handler| handler.va));
        }
        if let Some(resume) = self.resume(map) {
            code.extend(resume.iter());
        }
        code.sort_unstable();
        code.dedup();
        code
    }
}

/// One `On Error GoTo` label of a native procedure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ErrorHandler {
    /// The handler's number: what the procedure passes `__vbaOnError` to
    /// make it the active one (from 1, in the order the labels are first
    /// named).
    pub number: u32,
    /// VA of the label's code.
    pub va: u32,
}

/// The labels a native procedure's `On Error GoTo` statements name
/// ([`ProcUnwindInfo::handlers_va`]).
///
/// | Offset | Size | Field |
/// |--------|------|-------|
/// | 0x00 | 4 | Count |
/// | 0x04 | 8 x count | `{u32 number; u32 va}` per handler ([`ErrorHandler`]) |
///
/// On an error the runtime looks up the active handler's number and jumps
/// to its label in the procedure's frame.
#[derive(Clone, Copy, Debug)]
pub struct ErrorHandlerTable<'a> {
    bytes: &'a [u8],
}

impl<'a> ErrorHandlerTable<'a> {
    /// Parses the table from the given byte slice.
    ///
    /// # Errors
    ///
    /// Returns [`Error::TooShort`] if `data` is shorter than its count
    /// calls for, or [`Error::ArithmeticOverflow`] if the count is too
    /// large to size.
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        let count = read_u32_le(data, 0)? as usize;
        let size = count
            .checked_mul(8)
            .and_then(|entries| entries.checked_add(4))
            .ok_or(Error::ArithmeticOverflow {
                context: "ErrorHandlerTable",
            })?;
        let bytes = data.get(..size).ok_or(Error::TooShort {
            expected: size,
            actual: data.len(),
            context: "ErrorHandlerTable",
        })?;
        Ok(Self { bytes })
    }

    /// Size of the table in bytes.
    #[inline]
    pub fn size(&self) -> usize {
        self.bytes.len()
    }

    /// Number of handlers.
    #[inline]
    pub fn count(&self) -> usize {
        self.bytes.len().saturating_sub(4) / 8
    }

    /// The handlers, in table order.
    pub fn iter(&self) -> impl Iterator<Item = ErrorHandler> + 'a {
        let bytes = self.bytes;
        (0..self.count()).filter_map(move |index| {
            let at = index.checked_mul(8)?.checked_add(4)?;
            Some(ErrorHandler {
                number: read_u32_le(bytes, at).ok()?,
                va: read_u32_le(bytes, at.checked_add(4)?).ok()?,
            })
        })
    }
}

/// The address of each statement of a native procedure, for `Resume` and
/// `Resume Next` ([`ProcUnwindInfo::resume_va`]).
///
/// | Offset | Size | Field |
/// |--------|------|-------|
/// | 0x00 | 4 | Count |
/// | 0x04 | 4 x count | VA of statement 1, 2, ... |
/// | 4 x (count + 1) | 4 | 0 |
///
/// The procedure stores the number of the statement it is running in
/// `ebp-4`; `Resume` jumps to word `n` of the table (the statement that
/// failed), `Resume Next` to word `n + 1`, the closing 0 after the last
/// statement. A statement that compiles to no code (a label) shares the
/// next one's address.
#[derive(Clone, Copy, Debug)]
pub struct ResumeTable<'a> {
    bytes: &'a [u8],
}

impl<'a> ResumeTable<'a> {
    /// Parses the table from the given byte slice.
    ///
    /// # Errors
    ///
    /// Returns [`Error::TooShort`] if `data` is shorter than its count
    /// calls for, or [`Error::ArithmeticOverflow`] if the count is too
    /// large to size.
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        let count = read_u32_le(data, 0)? as usize;
        let size = count
            .checked_add(2)
            .and_then(|words| words.checked_mul(4))
            .ok_or(Error::ArithmeticOverflow {
                context: "ResumeTable",
            })?;
        let bytes = data.get(..size).ok_or(Error::TooShort {
            expected: size,
            actual: data.len(),
            context: "ResumeTable",
        })?;
        Ok(Self { bytes })
    }

    /// Size of the table in bytes, the closing 0 included.
    #[inline]
    pub fn size(&self) -> usize {
        self.bytes.len()
    }

    /// Number of statements.
    #[inline]
    pub fn count(&self) -> usize {
        (self.bytes.len() / 4).saturating_sub(2)
    }

    /// VA of statement `number` (from 1); `None` outside the table.
    pub fn statement_va(&self, number: usize) -> Option<u32> {
        if number == 0 || number > self.count() {
            return None;
        }
        read_u32_le(self.bytes, number.checked_mul(4)?).ok()
    }

    /// The statements' VAs, statement 1 first.
    pub fn iter(&self) -> impl Iterator<Item = u32> + 'a {
        let table = *self;
        (1..=self.count()).filter_map(move |number| table.statement_va(number))
    }
}

/// The line number of each statement of a native procedure, for `Erl`
/// ([`ProcUnwindInfo::lines_va`]).
///
/// | Offset | Size | Field |
/// |--------|------|-------|
/// | 0x00 | 2 | Count |
/// | 0x02 | 2 x (count + 1) | Line number of statement 0, 1, ..., count |
///
/// Indexed by the statement number the procedure stores in `ebp-4`, from
/// statement 0 (before the first, line 0). A statement with no line number
/// of its own has the last one before it; a numbered line is two
/// statements (its label and its code), both with its number.
#[derive(Clone, Copy, Debug)]
pub struct LineNumberTable<'a> {
    bytes: &'a [u8],
}

impl<'a> LineNumberTable<'a> {
    /// Parses the table from the given byte slice.
    ///
    /// # Errors
    ///
    /// Returns [`Error::TooShort`] if `data` is shorter than its count
    /// calls for.
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        let count = usize::from(read_u16_le(data, 0)?);
        let size = count.saturating_add(2).saturating_mul(2);
        let bytes = data.get(..size).ok_or(Error::TooShort {
            expected: size,
            actual: data.len(),
            context: "LineNumberTable",
        })?;
        Ok(Self { bytes })
    }

    /// Size of the table in bytes.
    #[inline]
    pub fn size(&self) -> usize {
        self.bytes.len()
    }

    /// Number of statements (statement 0 not counted).
    #[inline]
    pub fn count(&self) -> usize {
        (self.bytes.len() / 2).saturating_sub(2)
    }

    /// Line number of statement `number` (0 to [`count`](Self::count));
    /// `None` outside the table.
    pub fn line(&self, number: usize) -> Option<u16> {
        if number > self.count() {
            return None;
        }
        read_u16_le(self.bytes, number.checked_mul(2)?.checked_add(2)?).ok()
    }
}

/// A native procedure and the unwind record its prologue stores
/// ([`VbProject::unwind_records`](crate::VbProject::unwind_records)).
#[derive(Clone, Copy, Debug)]
pub struct ProcedureUnwind<'a> {
    /// VA of the procedure's first instruction (`push ebp`).
    pub procedure_va: u32,
    /// VA of its record.
    pub record_va: u32,
    /// The record.
    pub info: ProcUnwindInfo<'a>,
}

impl<'a> ProcedureUnwind<'a> {
    /// Finds every procedure in `code` that stores an unwind record: each
    /// `mov [ebp-slot], imm32` there whose immediate is a record naming
    /// that slot, paired with the nearest `push ebp; mov ebp, esp` at most
    /// [`ProcUnwindInfo::PROLOGUE_SCAN`] bytes before it whose
    /// [`from_prologue`](ProcUnwindInfo::from_prologue) is that store.
    /// Ordered by procedure; empty when `code` does not read.
    pub fn scan(map: &AddressMap<'a>, code: Range<u32>) -> Vec<Self> {
        let Some(bytes) = code.end.checked_sub(code.start).and_then(|len| {
            map.slice_from_va(code.start, len as usize)
                .ok()?
                .get(..len as usize)
        }) else {
            return Vec::new();
        };
        let mut found: Vec<Self> = Vec::new();
        for (offset, window) in bytes.windows(7).enumerate() {
            let Some((record_va, _)) = ProcUnwindInfo::stored_by(map, window) else {
                continue;
            };
            let earliest = offset
                .saturating_add(7)
                .saturating_sub(ProcUnwindInfo::PROLOGUE_SCAN);
            let procedure = (earliest..=offset).rev().find_map(|start| {
                if !bytes.get(start..)?.starts_with(&[0x55, 0x8B, 0xEC]) {
                    return None;
                }
                let procedure_va = code.start.checked_add(u32::try_from(start).ok()?)?;
                let (stored, info) = ProcUnwindInfo::from_prologue(map, procedure_va)?;
                (stored == record_va).then_some(Self {
                    procedure_va,
                    record_va,
                    info,
                })
            });
            found.extend(procedure);
        }
        found.dedup_by_key(|procedure| procedure.procedure_va);
        found
    }
}
