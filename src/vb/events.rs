//! Event sink structures for VB6 control event dispatch.
//!
//! VB6 controls fire events (Click, DblClick, KeyPress, etc.) through COM
//! connection point interfaces. Each control has an [`EventSinkVtable`] that
//! maps event slots to handler methods. The handler VAs point to P-Code
//! [`EventHandlerThunk`]s (every fixture) or, by the crate's reading of
//! natively compiled controls, [`NativeEventThunk`]s (no fixture has one).

use std::fmt;

use crate::{addressmap::AddressMap, error::Error, util::read_u32_le, vb::control::ControlInfo};

/// Parsed P-Code event handler thunk (20-byte dual-entry method stub).
///
/// A method that handles an event has a 0x14-byte stub with **two entry
/// points**:
///
/// ```text
/// +0x00  B8 XX XX XX XX   mov eax, this_adjust         <- event sink slot
/// +0x05  66 3D            cmp ax, imm16 (its imm16 is the next 2 bytes)
/// +0x07  33 C0            xor eax, eax                 <- method link entry
/// +0x09  BA XX XX XX XX   mov edx, ProcDscInfo_VA
/// +0x0E  68 XX XX XX XX   push engine_thunk_va         ; jmp [MethCallEngine]
/// +0x13  C3               ret
/// ```
///
/// The control's [`EventSinkVtable`] slot points to +0x00 and the object's
/// method link table entry (see [`MethodLink`](crate::project::MethodLink))
/// to +0x07. Both reach `MethCallEngine` (MSVBVM60 6.00.8176
/// `0x661080B8`), which starts with `sub [esp+4], eax`: `eax` is subtracted
/// from the `this` pointer the caller passed. Through the method link entry
/// it is 0; through the event sink it is the sink's offset in the object
/// instance, the control's
/// [`ControlInfo::dispatch_offset`](crate::vb::control::ControlInfo::dispatch_offset)
/// (`tests/fixtures/controls`: 0x3C for `Command1_Click`, 0x38 for
/// `Form_Load`), which turns the sink pointer back into the object. The
/// `cmp ax, imm16` only skips the `xor` on the event path.
#[derive(Clone, Copy, Debug)]
pub struct EventHandlerThunk {
    /// The value the event entry loads into `eax` (`mov eax, imm32` at
    /// +0x00) and `MethCallEngine` subtracts from `this`.
    pub this_adjust: u32,
    /// VA of the ProcDscInfo (RTMI) structure (from `mov edx, imm32` at +0x09).
    pub proc_dsc_info_va: u32,
    /// VA the stub pushes and returns to (`push imm32` at +0x0E): the
    /// `jmp [MethCallEngine]` import thunk.
    pub engine_thunk_va: u32,
    /// VA of the method link entry point (+0x07 from the event entry).
    pub method_entry_va: u32,
}

impl EventHandlerThunk {
    /// Total size of the thunk in bytes.
    pub const SIZE: usize = 0x14;

    /// Byte offset from the event entry to the method entry (`xor eax, eax`).
    pub const METHOD_ENTRY_OFFSET: usize = 0x07;

    /// Parses an event handler thunk from the event sink entry point.
    ///
    /// `data` should start at the `mov eax, imm32` instruction (+0x00).
    /// Returns `None` if the byte pattern doesn't match the expected stub.
    pub fn parse_from_event_entry(data: &[u8], event_entry_va: u32) -> Option<Self> {
        let bytes: &[u8; Self::SIZE] = data.get(..Self::SIZE)?.try_into().ok()?;
        if bytes[0] != 0xB8 {
            return None;
        }
        let this_adjust = u32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);
        if bytes[5] != 0x66 || bytes[6] != 0x3D {
            return None;
        }
        if bytes[7] != 0x33 || bytes[8] != 0xC0 {
            return None;
        }
        if bytes[9] != 0xBA {
            return None;
        }
        let proc_dsc_info_va = u32::from_le_bytes([bytes[10], bytes[11], bytes[12], bytes[13]]);
        if bytes[14] != 0x68 {
            return None;
        }
        let engine_thunk_va = u32::from_le_bytes([bytes[15], bytes[16], bytes[17], bytes[18]]);
        if bytes[19] != 0xC3 {
            return None;
        }
        Some(Self {
            this_adjust,
            proc_dsc_info_va,
            engine_thunk_va,
            method_entry_va: event_entry_va.wrapping_add(Self::METHOD_ENTRY_OFFSET as u32),
        })
    }

    /// Parses from the method link entry point (`xor eax, eax` at +0x07).
    ///
    /// Reads 7 bytes backwards to find the event prefix. Returns `None` if
    /// the bytes before the method entry don't match the event thunk pattern
    /// (the method may not have an event prefix).
    pub fn parse_from_method_entry(data: &[u8], method_entry_va: u32) -> Option<Self> {
        if data.len() < Self::SIZE {
            return None;
        }
        Self::parse_from_event_entry(
            data,
            method_entry_va.wrapping_sub(Self::METHOD_ENTRY_OFFSET as u32),
        )
    }
}

impl fmt::Display for EventHandlerThunk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "this_adjust=0x{:X} rtmi=0x{:08X} method=0x{:08X}",
            self.this_adjust, self.proc_dsc_info_va, self.method_entry_va
        )
    }
}

/// Parsed native adjustor thunk (13-byte `this`-adjusting JMP stub).
///
/// A natively compiled object enters its procedures through these: an
/// event sink's handler slot names a thunk's first entry, and a method link
/// its bare jump at +8 ([`JUMP_OFFSET`](Self::JUMP_OFFSET)). The compiler
/// also places thunks nothing names, whose adjustment is 0xFFFF
/// (`events-native`).
///
/// ```text
/// +0x00  81 6C 24 04 XX XX XX XX   sub dword [esp+4], this_adjust
/// +0x08  E9 XX XX XX XX            jmp native_handler
/// ```
#[derive(Clone, Copy, Debug)]
pub struct NativeEventThunk {
    /// Adjustment subtracted from the COM `this` pointer.
    pub this_adjust: u32,
    /// VA of the native method body (JMP target).
    pub handler_va: u32,
}

impl NativeEventThunk {
    /// Total size of the native thunk in bytes.
    pub const SIZE: usize = 13;

    /// Offset of the thunk's second entry, its `jmp rel32`.
    pub const JUMP_OFFSET: u32 = 8;

    /// Parses a native event thunk from the given bytes.
    pub fn parse(data: &[u8], thunk_va: u32) -> Option<Self> {
        let bytes: &[u8; Self::SIZE] = data.get(..Self::SIZE)?.try_into().ok()?;
        if bytes[0..4] != [0x81, 0x6C, 0x24, 0x04] {
            return None;
        }
        let this_adjust = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        if bytes[8] != 0xE9 {
            return None;
        }
        let rel32 = i32::from_le_bytes([bytes[9], bytes[10], bytes[11], bytes[12]]);
        let handler_va = thunk_va
            .wrapping_add(Self::SIZE as u32)
            .wrapping_add(rel32 as u32);
        Some(Self {
            this_adjust,
            handler_va,
        })
    }
}

impl fmt::Display for NativeEventThunk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "this_adjust=0x{:X} -> 0x{:08X}",
            self.this_adjust, self.handler_va
        )
    }
}

/// Parsed IUnknown thunk from EventSinkVtable (+0x0C, +0x10, +0x14).
///
/// These are 6-byte `FF 25 imm32` (`jmp [IAT_addr]`) indirect jumps through
/// the Import Address Table to `EVENT_SINK_QueryInterface`, `EVENT_SINK_AddRef`,
/// and `EVENT_SINK_Release` in MSVBVM60.DLL.
///
/// All controls in the same object share the same three thunk VAs.
#[derive(Clone, Copy, Debug)]
pub struct IUnknownThunk {
    /// VA of the IAT entry (target of the `jmp [addr]` instruction).
    pub iat_va: u32,
}

impl IUnknownThunk {
    /// Thunk instruction size in bytes (`FF 25 imm32` = 6 bytes).
    pub const SIZE: usize = 6;

    /// Parses a `jmp [IAT_addr]` thunk from the given bytes.
    ///
    /// Returns `None` if the bytes don't start with `FF 25`.
    pub fn parse(data: &[u8]) -> Option<Self> {
        let bytes: &[u8; Self::SIZE] = data.get(..Self::SIZE)?.try_into().ok()?;
        if bytes[0] != 0xFF || bytes[1] != 0x25 {
            return None;
        }
        let iat_va = u32::from_le_bytes([bytes[2], bytes[3], bytes[4], bytes[5]]);
        Some(Self { iat_va })
    }
}

impl fmt::Display for IUnknownThunk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "jmp [0x{:08X}]", self.iat_va)
    }
}

/// View over a control's event sink vtable.
///
/// This is a COM connection point interface that receives events from the
/// control (Click, DblClick, KeyPress, etc.). The compiler fills in a slot
/// for each event the object handles (`Private Sub Command1_Click()`); the
/// other slots are 0.
///
/// # Layout (variable-length: 0x18 + 4 * [`ControlInfo::sink_slots`])
///
/// | Offset | Field |
/// |--------|-------|
/// | 0x00 | Reserved (always 0) |
/// | 0x04 | Back-pointer to this control's [`ControlInfo`] entry |
/// | 0x08 | Back-pointer to parent [`ObjectInfo`](crate::vb::object::ObjectInfo) |
/// | 0x0C | `EVENT_SINK_QueryInterface` thunk VA - `jmp [IAT]` to MSVBVM60 |
/// | 0x10 | `EVENT_SINK_AddRef` thunk VA - `jmp [IAT]` to MSVBVM60 |
/// | 0x14 | `EVENT_SINK_Release` thunk VA - `jmp [IAT]` to MSVBVM60 |
/// | 0x18 | `IDispatch` thunk VAs ([`dispatch_va`](Self::dispatch_va)), 4 for a `WithEvents` or `Implements` sink, none for a control |
/// | then | Event handler VAs ([`handler_va`](Self::handler_va); 0 = not connected) |
///
/// [`ControlInfo::dispatch_slots`] says which sinks have the `IDispatch`
/// slots (see [`ControlKind`](crate::vb::control::ControlKind)).
///
/// The IUnknown thunks at +0x0C-0x14 are 6-byte `FF 25 imm32` indirect jumps
/// through the Import Address Table to MSVBVM60.DLL. All controls in the same
/// object share the same three thunk VAs. Use [`resolve_iunknown_thunk`](Self::resolve_iunknown_thunk)
/// to parse the thunk code. Event handler slots at +0x18+ hold the handled
/// events' [`EventHandlerThunk`] VAs on disk (`tests/fixtures/controls`:
/// `Command1`'s sink 0x004018CC has slot 0, `Click`, = 0x00401928) and 0
/// for the others.
#[derive(Clone, Copy, Debug)]
pub struct EventSinkVtable<'a> {
    bytes: &'a [u8],
    dispatch_slots: u16,
    handler_count: u16,
}

impl<'a> EventSinkVtable<'a> {
    /// Header size before event handler entries.
    pub const HEADER_SIZE: usize = 0x18;

    /// Parses the event sink vtable of the ControlInfo entry `info` from a
    /// byte slice.
    ///
    /// The entry gives the slot count and whether `IDispatch` slots come
    /// first ([`ControlInfo::dispatch_slots`]).
    ///
    /// # Errors
    ///
    /// Returns an error if the entry's fields cannot be read, or
    /// [`Error::TooShort`] if `data` is shorter than the header and its
    /// slots.
    pub fn parse(data: &'a [u8], info: &ControlInfo<'_>) -> Result<Self, Error> {
        let dispatch_slots = info.dispatch_slots()?;
        let handler_count = info.event_count()?;
        let total =
            Self::HEADER_SIZE.saturating_add(usize::from(info.sink_slots()?).saturating_mul(4));
        let bytes = data.get(..total).ok_or(Error::TooShort {
            expected: total,
            actual: data.len(),
            context: "EventSinkVtable",
        })?;
        Ok(Self {
            bytes,
            dispatch_slots,
            handler_count,
        })
    }

    /// Returns the VA of `IDispatch` slot `index` (0 `GetTypeInfoCount`,
    /// 1 `GetTypeInfo`, 2 `GetIDsOfNames`, 3 `Invoke`), or `None` for a
    /// sink without them or an index past them.
    pub fn dispatch_va(&self, index: u16) -> Option<u32> {
        if index >= self.dispatch_slots {
            return None;
        }
        let offset = Self::HEADER_SIZE.checked_add((index as usize).checked_mul(4)?)?;
        read_u32_le(self.bytes, offset).ok()
    }

    /// Number of `IDispatch` slots before the handlers (4 or 0).
    #[inline]
    pub fn dispatch_slots(&self) -> u16 {
        self.dispatch_slots
    }

    /// Returns the bytes the vtable occupies: its header and
    /// [`ControlInfo::sink_slots`] slots, the `IDispatch` and handler slots
    /// and, for an `Implements`, four zero slots after them.
    #[inline]
    pub fn size(&self) -> usize {
        self.bytes.len()
    }

    /// Back-pointer to this control's ControlInfo entry at +0x04.
    #[inline]
    pub fn control_info_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x04)
    }

    /// Back-pointer to the parent ObjectInfo at +0x08.
    #[inline]
    pub fn object_info_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x08)
    }

    /// VA of the EVENT_SINK_QueryInterface thunk at +0x0C.
    #[inline]
    pub fn query_interface_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x0C)
    }

    /// VA of the EVENT_SINK_AddRef thunk at +0x10.
    #[inline]
    pub fn add_ref_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x10)
    }

    /// VA of the EVENT_SINK_Release thunk at +0x14.
    #[inline]
    pub fn release_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x14)
    }

    /// Number of handler slots after the `IDispatch` ones: the events, or
    /// an `Implements`' interface members.
    #[inline]
    pub fn handler_count(&self) -> u16 {
        self.handler_count
    }

    /// Returns the VA of the event handler at the given slot index (the
    /// event's index in its interface; for an `Implements`, the member's).
    ///
    /// Returns 0 if the object does not handle the event.
    /// Returns `None` if `slot >= handler_count`.
    pub fn handler_va(&self, slot: u16) -> Option<u32> {
        if slot >= self.handler_count {
            return None;
        }
        let index = usize::from(self.dispatch_slots).checked_add(usize::from(slot))?;
        let offset = Self::HEADER_SIZE.checked_add(index.checked_mul(4)?)?;
        read_u32_le(self.bytes, offset).ok()
    }

    /// Resolves an event handler VA into a parsed [`EventHandlerThunk`].
    ///
    /// Reads the 20-byte dual-entry stub at the handler VA and extracts
    /// the `this` adjustment, ProcDscInfo VA, and method entry point.
    /// Returns `None` if the slot is empty or the bytes don't match.
    pub fn resolve_handler_thunk(
        &self,
        slot: u16,
        map: &AddressMap<'_>,
    ) -> Option<EventHandlerThunk> {
        let va = self.handler_va(slot)?;
        if va == 0 {
            return None;
        }
        let data = map.slice_from_va(va, EventHandlerThunk::SIZE).ok()?;
        EventHandlerThunk::parse_from_event_entry(data, va)
    }

    /// Resolves an event handler VA into a parsed [`NativeEventThunk`].
    ///
    /// Tries the `sub [esp+4]; jmp` pattern used by native-compiled controls.
    pub fn resolve_native_thunk(
        &self,
        slot: u16,
        map: &AddressMap<'_>,
    ) -> Option<NativeEventThunk> {
        let va = self.handler_va(slot)?;
        if va == 0 {
            return None;
        }
        let data = map.slice_from_va(va, NativeEventThunk::SIZE).ok()?;
        NativeEventThunk::parse(data, va)
    }

    /// Resolves an IUnknown thunk VA (QI, AddRef, or Release) into a
    /// parsed [`IUnknownThunk`].
    ///
    /// The thunk is a 6-byte `FF 25 imm32` indirect jump through the IAT.
    /// Returns `None` if the VA is zero or the bytes don't match.
    pub fn resolve_iunknown_thunk(&self, va: u32, map: &AddressMap<'_>) -> Option<IUnknownThunk> {
        if va == 0 {
            return None;
        }
        let data = map.slice_from_va(va, IUnknownThunk::SIZE).ok()?;
        IUnknownThunk::parse(data)
    }

    /// Returns the number of connected (non-zero) event handlers.
    pub fn connected_count(&self) -> u16 {
        (0..self.handler_count)
            .filter(|&i| self.handler_va(i).is_some_and(|va| va != 0))
            .count() as u16
    }

    /// Returns an iterator over `(slot_index, handler_va)` for all
    /// connected (non-zero) event handlers.
    pub fn connected_handlers(&self) -> impl Iterator<Item = (u16, u32)> + '_ {
        (0..self.handler_count).filter_map(|i| {
            let va = self.handler_va(i)?;
            if va != 0 { Some((i, va)) } else { None }
        })
    }
}
