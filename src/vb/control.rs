//! ControlInfo structure for GUI controls.
//!
//! Describes ActiveX/VB controls embedded in forms. Each form's
//! [`OptionalObjectInfo`](crate::vb::object::OptionalObjectInfo) points
//! to an array of `ControlInfo` entries via `lpControls`: one per control
//! that has event handlers, `WithEvents` variable or implemented interface,
//! and one for the object itself. [`ControlInfo`] gives the layout as the
//! compiler writes it and MSVBVM60.DLL reads it.

use std::{fmt, str::FromStr};

use crate::{
    addressmap::AddressMap,
    error::Error,
    util::{read_u16_le, read_u32_le},
};

/// A COM GUID (CLSID/IID) as stored in PE data - 16 bytes, little-endian.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Guid {
    /// Raw 16-byte GUID in binary form.
    pub bytes: [u8; 16],
}

impl Guid {
    /// Parses a GUID from a 16-byte slice.
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        let slice = data.get(..16)?;
        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(slice);
        Some(Self { bytes })
    }

    /// Returns a human-readable name if this is a well-known VB6 intrinsic control.
    ///
    /// Uses exact matching against the GUIDs a ControlInfo (+0x08) names: a
    /// control's events (source) interface from `VB6.OLB` (`TextBoxEvents`
    /// `{33AD4EE2-...}` → `"TextBox"`), the same IID + 1 for a control array
    /// (`{33AD4EF3-...}` → `"CommandButton"`), `IClassModuleEvt`
    /// `{FCFB3D21-...}` → `"Class"`; coclass CLSIDs match too. The lookup
    /// table is generated at build time from `data/vb6_control_guids.csv`.
    ///
    /// For reliable control type identification, prefer
    /// [`FormControlType`](crate::vb::formdata::FormControlType) from form binary
    /// data (via [`VbControl::form_control_type`](crate::VbControl::form_control_type)).
    pub fn control_class_name(&self) -> Option<&'static str> {
        generated::lookup_control_name(&self.bytes)
    }
}

impl fmt::Debug for Guid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self}")
    }
}

impl fmt::Display for Guid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let b = &self.bytes;
        // Destructure the fixed 16-byte array to avoid lint indexing warnings.
        let &[
            b0,
            b1,
            b2,
            b3,
            b4,
            b5,
            b6,
            b7,
            b8,
            b9,
            b10,
            b11,
            b12,
            b13,
            b14,
            b15,
        ] = b;
        let d1 = u32::from_le_bytes([b0, b1, b2, b3]);
        let d2 = u16::from_le_bytes([b4, b5]);
        let d3 = u16::from_le_bytes([b6, b7]);
        write!(
            f,
            "{{{d1:08X}-{d2:04X}-{d3:04X}-{b8:02X}{b9:02X}-{b10:02X}{b11:02X}{b12:02X}{b13:02X}{b14:02X}{b15:02X}}}"
        )
    }
}

impl FromStr for Guid {
    type Err = Error;

    /// Parses the registry form [`Display`](fmt::Display) writes,
    /// `{XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX}` (braces required, hex digits
    /// of either case): the first three groups little-endian into
    /// [`bytes`](Self::bytes), the last two as written.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidGuid`] for any other text.
    fn from_str(text: &str) -> Result<Self, Error> {
        let invalid = || Error::InvalidGuid {
            text: text.to_string(),
        };
        let inner = text
            .strip_prefix('{')
            .and_then(|t| t.strip_suffix('}'))
            .ok_or_else(invalid)?;
        let groups: Vec<&str> = inner.split('-').collect();
        let [d1, d2, d3, d4, d5] = groups.as_slice() else {
            return Err(invalid());
        };
        let widths = [(d1, 8), (d2, 4), (d3, 4), (d4, 4), (d5, 12)];
        if widths.iter().any(|(group, width)| {
            group.len() != *width || !group.bytes().all(|b| b.is_ascii_hexdigit())
        }) {
            return Err(invalid());
        }
        let d1 = u32::from_str_radix(d1, 16).map_err(|_| invalid())?;
        let d2 = u16::from_str_radix(d2, 16).map_err(|_| invalid())?;
        let d3 = u16::from_str_radix(d3, 16).map_err(|_| invalid())?;
        let mut bytes = [0u8; 16];
        let tail = format!("{d4}{d5}");
        let (head, rest) = bytes.split_at_mut(8);
        head.copy_from_slice(
            &[
                d1.to_le_bytes().as_slice(),
                &d2.to_le_bytes(),
                &d3.to_le_bytes(),
            ]
            .concat(),
        );
        for (k, byte) in rest.iter_mut().enumerate() {
            let start = k.checked_mul(2).ok_or_else(invalid)?;
            let pair = tail
                .get(start..start.saturating_add(2))
                .ok_or_else(invalid)?;
            *byte = u8::from_str_radix(pair, 16).map_err(|_| invalid())?;
        }
        Ok(Self { bytes })
    }
}

/// Build-time generated lookup tables from CSV data files.
pub(crate) mod generated {
    include!(concat!(env!("OUT_DIR"), "/vb6_data_generated.rs"));
}

// Event-related types (EventHandlerThunk, NativeEventThunk, EventSinkVtable)

/// What a [`ControlInfo`] entry describes, from its flags at offset 0x00.
///
/// The flag values of `tests/fixtures` (P-Code and native builds), and the
/// layout of the entry's event sink vtable after its 0x18-byte header:
///
/// | Flags | Kind | Sink vtable |
/// |-------|------|-------------|
/// | 0x0040 | [`Control`](Self::Control) | one handler slot per event (`controls` `Command1`: slot 0, `Click`, at +0x18) |
/// | 0x002E | [`WithEvents`](Self::WithEvents) of a project class | four `IDispatch` slots, then one slot per event; the slot count at 0x02 is the events only |
/// | 0x0046 | [`WithEvents`](Self::WithEvents) of a dispinterface source (`typerefs` `Watcher.Txt As VB.TextBox`) | one handler slot per event |
/// | 0x007A | [`Implements`](Self::Implements) | four `IDispatch` slots, then one slot per interface member; the slot count at 0x02 includes the four |
///
/// Bit 0x10 marks an `Implements`, bit 0x04 a `WithEvents` variable, and
/// bits 0x28 a sink that is a dual interface's vtable
/// ([`ControlInfo::dispatch_slots`]): after `QueryInterface`, `AddRef` and
/// `Release` (+0x0C..+0x14) come `GetTypeInfoCount`, `GetTypeInfo`,
/// `GetIDsOfNames` and `Invoke`, import thunks to
/// `Zombie_GetTypeInfoCount`, `Zombie_GetTypeInfo`,
/// `EVENT_SINK_GetIDsOfNames` and `EVENT_SINK_Invoke` (`events`
/// `Listener.m_Source`: 0x0040106E-0x00401080, then the handlers of
/// `Started`, `Ticked` and `Named`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlKind {
    /// A control, a control array, or the object itself (`Form`,
    /// `UserControl`, `Class`).
    Control,
    /// A `WithEvents` variable.
    WithEvents,
    /// An `Implements` statement.
    Implements,
}

impl ControlKind {
    /// Classifies the flags at ControlInfo offset 0x00.
    #[must_use]
    pub fn from_flags(flags: u16) -> Self {
        if flags & 0x10 != 0 {
            Self::Implements
        } else if flags & 0x04 != 0 {
            Self::WithEvents
        } else {
            Self::Control
        }
    }
}

/// View over a ControlInfo structure (0x28 bytes).
///
/// Each entry describes one GUI control on a VB6 form. The array is
/// at [`OptionalObjectInfo::controls_va`](crate::vb::object::OptionalObjectInfo::controls_va)
/// with [`control_count`](crate::vb::object::OptionalObjectInfo::control_count) entries.
///
/// # Layout
///
/// | Offset | Size | Field | Description |
/// |--------|------|-------|-------------|
/// | 0x00 | 2 | `wFlags` | What the entry describes ([`ControlKind`]): 0x0040 a control or the object itself, 0x002E or 0x0046 a `WithEvents` variable, 0x007A an `Implements` |
/// | 0x02 | 2 | `wEventHandlerSlots` | Slot count of the event sink vtable after its 0x18-byte header (see [`ControlKind`]) |
/// | 0x04 | 2 | `wDispatchOffset` | Offset of this control's event sink in the object's instance (the value the event stubs load into eax) |
/// | 0x06 | 2 | Reserved | Always 0 |
/// | 0x08 | 4 | `lpGuid` | VA of the control's events (source) IID: `TextBoxEvents` `{33AD4EE2-...}`; IID + 1 for a control array of an intrinsic control (CommandButton, TextBox, Label: `tests/fixtures/mdi`); for a hosted ActiveX control or UserControl, the build-generated IID its [`ExternalComponentEntry`](crate::vb::external::ExternalComponentEntry) names (`instance_events_iid`, `array_events_iid`); `FormEvents` / `UserControlEvents` / `IClassModuleEvt` for the object itself; the interface's IID for an `Implements` entry; the source's events IID for a `WithEvents` variable |
/// | 0x0C | 2 | `wIndex` | Control index (Name property ID, 0xFFFF = the object itself); a form's or UserControl's vtable has this control's getter at `designer interface size + 4 * index` |
/// | 0x0E | 2 | `wMemberType` | Member type constant (always 3 for normal controls, 0xFFFF for default) |
/// | 0x10 | 2 | Number of pairs in the DISPID map ([`dispid_map`](ControlInfo::dispid_map)); 0 for a control |
/// | 0x12 | 2 | 4 or 0x10 when there is a map (unread; 4 for 1 to 3 pairs, 0x10 for 9 in the fixtures) |
/// | 0x14 | 4 | VA of the DISPID map, 0 for a control |
/// | 0x18 | 4 | `lpEventSinkVtable` | VA of event sink vtable (0x18 header + slots×4) |
/// | 0x1C | 4 | `lpLinkerTypeData` | Per-control-type linker workspace VA (unpatched, not in PE image) |
/// | 0x20 | 4 | `lpName` | VA of control name string (null-terminated ANSI) |
/// | 0x24 | 4 | `dwControlId` | Packed control identifier: `(wMemberType << 16) \| wIndex` |
///
/// # DISPID map (+0x10, +0x14)
///
/// A `WithEvents` variable's and an `Implements`' entry name the object
/// procedure that handles each member of the source interface: `{source
/// DISPID, handler MEMID}` pairs, the source DISPID being the event's number
/// for a `WithEvents` variable and the interface member's DISPID for an
/// `Implements` (`events` `Listener`'s `m_Source`: 1 to 0x60030002, 2 to
/// 0x60030003, 3 to 0x60030004). The compiler writes the map right before
/// the entry's event sink vtable. The sink's `Invoke` looks the incoming
/// DISPID up in it and calls the object's own `Invoke` with the handler's
/// MEMID, and its `GetIDsOfNames` maps back (MSVBVM60 6.00.8176
/// `0x6601846A`, `0x660E2614`).
///
/// # Control Identifier (+0x24)
///
/// The packed `(wMemberType << 16) | wIndex` value serves as the hash key
/// for runtime control lookup via `ControlNameHashLookup`. `LoadFormControls`
/// passes this dword directly to `CreateControlEntry`.
///
/// # Linker Type Data (+0x1C)
///
/// The VA at +0x1C points to per-control-type data in the linker's workspace
/// address space (typically 0x0073xxxx). Controls with the same CLSID share
/// the same +0x1C value. This pointer is NOT patched to a valid PE VA - it's
/// a vestigial linker artifact. In memory dumps it may be overwritten.
///
/// # Name Resolution
///
/// The control name is at [`name_va`](Self::name_va) (+0x20), NOT at +0x18.
/// The name is a null-terminated ANSI string followed by padding to 4-byte
/// alignment, then a 16-byte interface GUID for the control.
#[derive(Clone, Copy, Debug)]
pub struct ControlInfo<'a> {
    /// Raw backing bytes borrowed from the PE file buffer.
    bytes: &'a [u8],
}

impl<'a> ControlInfo<'a> {
    /// Size of the ControlInfo structure in bytes.
    pub const MIN_SIZE: usize = 0x28;

    /// Parses a ControlInfo from the given byte slice.
    ///
    /// # Errors
    ///
    /// Returns [`Error::TooShort`] if `data.len() < 0x28`.
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        let bytes = data.get(..Self::MIN_SIZE).ok_or(Error::TooShort {
            expected: Self::MIN_SIZE,
            actual: data.len(),
            context: "ControlInfo",
        })?;
        Ok(Self { bytes })
    }

    /// Flags at offset 0x00: what the entry describes (see [`kind`](Self::kind)).
    ///
    /// The runtime's control consumers (`CreateControlEntry`,
    /// `ControlNameHashLookup`, `LoadFormControls`, `FormWrapper_Init`,
    /// `InitControlProperties`, `EventSink_GetVtableBase`) do not read it.
    #[inline]
    pub fn flags(&self) -> Result<u16, Error> {
        read_u16_le(self.bytes, 0x00)
    }

    /// What the entry describes, from the [`flags`](Self::flags) at 0x00.
    ///
    /// # Errors
    ///
    /// Returns an error if the flags cannot be read.
    #[inline]
    pub fn kind(&self) -> Result<ControlKind, Error> {
        Ok(ControlKind::from_flags(self.flags()?))
    }

    /// Slot count at offset 0x02: the entries of the event sink vtable
    /// after its 0x18-byte header.
    ///
    /// For a control or a `WithEvents` variable, its events (PictureBox 20,
    /// TextBox 24, Menu 15, CheckBox 1, OptionButton 31), which follow the
    /// [`dispatch_slots`](Self::dispatch_slots) when there are any; for an
    /// `Implements`, the four `IDispatch` slots and the interface's members.
    /// See [`ControlKind`] and [`event_count`](Self::event_count).
    #[inline]
    pub fn event_handler_slots(&self) -> Result<u16, Error> {
        read_u16_le(self.bytes, 0x02)
    }

    /// Number of `IDispatch` slots the event sink vtable has before its
    /// handlers: 4 when it is a dual interface's vtable (flag bits 0x28: a
    /// `WithEvents` variable of a project class, an `Implements`), else 0.
    ///
    /// # Errors
    ///
    /// Returns an error if the flags cannot be read.
    pub fn dispatch_slots(&self) -> Result<u16, Error> {
        Ok(if self.flags()? & 0x28 == 0x28 { 4 } else { 0 })
    }

    /// Number of handler slots of the event sink vtable: its events, or an
    /// `Implements`' members, without the `IDispatch` slots.
    ///
    /// # Errors
    ///
    /// Returns an error if the flags or the slot count cannot be read.
    pub fn event_count(&self) -> Result<u16, Error> {
        let slots = self.event_handler_slots()?;
        Ok(match self.kind()? {
            ControlKind::Implements => slots.saturating_sub(self.dispatch_slots()?),
            ControlKind::Control | ControlKind::WithEvents => slots,
        })
    }

    /// Control type flags at offset 0x00 (u32 view of flags + event_handler_slots).
    #[inline]
    pub fn control_type(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x00)
    }

    /// Offset of the control's event sink in the instance, at offset 0x04.
    ///
    /// Sequential multiples of 4 after the object's member variables (Board
    /// in `tests/fixtures/forms`: `m_Clicks` at 0x34, then 0x38-0x48 for its
    /// 5 ControlInfos; `tests/fixtures/events` Listener: the `WithEvents`
    /// variable at 0x38, its sink at 0x3C). The event stub of a handler
    /// for this control loads the same value into eax (`mov eax, imm32`
    /// before `xor eax, eax`), the `this` adjustment back to the object.
    #[inline]
    pub fn dispatch_offset(&self) -> Result<u16, Error> {
        read_u16_le(self.bytes, 0x04)
    }

    /// VA of the 16-byte events (source) IID the control's sink implements,
    /// at offset 0x08 (see the layout table; not a CLSID).
    #[inline]
    pub fn guid_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x08)
    }

    /// Control index at offset 0x0C.
    ///
    /// Corresponds to the control's Name property index in the VB6 IDE.
    /// Value 0xFFFF indicates the form's default/implicit control.
    #[inline]
    pub fn index(&self) -> Result<u16, Error> {
        read_u16_le(self.bytes, 0x0C)
    }

    /// COM dispatch member type at offset 0x0E (`DESCKIND`).
    ///
    /// Always 3 (`DESCKIND_TYPECOMP`) for normal controls - controls are type
    /// components in the COM IDispatch namespace. 0xFFFF for the form's default
    /// control (the implicit control with `index == 0xFFFF`). Used as the high
    /// word of the packed [`control_id`](Self::control_id) at +0x24 for hash
    /// lookup in `ControlNameHashLookup`.
    #[inline]
    pub fn member_type(&self) -> Result<u16, Error> {
        read_u16_le(self.bytes, 0x0E)
    }

    /// Number of pairs in the DISPID map, the u16 at offset 0x10: one per
    /// event of a `WithEvents` variable or member of an `Implements`, 0 for
    /// a control.
    #[inline]
    pub fn dispid_map_count(&self) -> Result<u16, Error> {
        read_u16_le(self.bytes, 0x10)
    }

    /// VA of the DISPID map at offset 0x14, 0 for a control.
    #[inline]
    pub fn dispid_map_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x14)
    }

    /// Reads the DISPID map: for each source interface member, the MEMID of
    /// the object procedure that handles it. Empty for a control, or when
    /// the map does not read.
    pub fn dispid_map(&self, map: &AddressMap<'_>) -> Vec<DispidMapping> {
        let (Ok(count), Ok(va)) = (self.dispid_map_count(), self.dispid_map_va()) else {
            return Vec::new();
        };
        let len = usize::from(count).saturating_mul(8);
        let Some(data) = map
            .slice_from_va(va, len)
            .ok()
            .and_then(|data| data.get(..len))
        else {
            return Vec::new();
        };
        data.as_chunks::<8>()
            .0
            .iter()
            .filter_map(|pair| {
                Some(DispidMapping {
                    source: read_u32_le(pair, 0).ok()?,
                    handler: read_u32_le(pair, 4).ok()?,
                })
            })
            .collect()
    }

    /// Number of slots of the event sink vtable after its 0x18-byte
    /// header: [`event_handler_slots`](Self::event_handler_slots), and 4
    /// more for a `WithEvents` variable or an `Implements` (flag bit 0x08).
    ///
    /// For a `WithEvents` variable the 4 are its `IDispatch` slots; an
    /// `Implements`' slot count already includes them and its last 4 slots
    /// are zero (the compiler sizes every sink `0x0C + 4 * (slots + 3)`,
    /// `+ 7` with bit 0x08: VBA6 6.0.9782 `0x0FA92F98`).
    ///
    /// # Errors
    ///
    /// Returns an error if the flags or the slot count cannot be read.
    pub fn sink_slots(&self) -> Result<u16, Error> {
        let extra = if self.flags()? & 0x08 != 0 { 4 } else { 0 };
        Ok(self.event_handler_slots()?.saturating_add(extra))
    }

    /// VA of the control's event sink vtable at offset 0x18.
    ///
    /// Points to a variable-length structure:
    ///
    /// | Offset | Field |
    /// |--------|-------|
    /// | +0x00 | null (reserved) |
    /// | +0x04 | back-pointer to this ControlInfo entry |
    /// | +0x08 | back-pointer to parent ObjectInfo |
    /// | +0x0C | EVENT_SINK_QueryInterface thunk VA |
    /// | +0x10 | EVENT_SINK_AddRef thunk VA |
    /// | +0x14 | EVENT_SINK_Release thunk VA |
    /// | +0x18 | event handler VAs ([`event_handler_slots`](Self::event_handler_slots) entries) |
    ///
    /// Total size = `0x18 + event_handler_slots * 4`.
    ///
    /// On disk, the event handler VAs are typically zero (populated at
    /// runtime when event handlers are connected to controls).
    #[inline]
    pub fn event_sink_vtable_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x18)
    }

    /// VA of the control name string at offset 0x20.
    ///
    /// Points to a null-terminated ANSI name string (the control's Name
    /// property from the VB6 IDE, e.g., "Command1", "Timer1").
    /// The name is stored in a shared data region alongside control CLSIDs.
    #[inline]
    pub fn name_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x20)
    }

    /// Per-type linker workspace VA at offset 0x1C.
    ///
    /// Unpatched pointer into the VB6 linker's address space (typically
    /// 0x0073xxxx). Controls with the same CLSID share the same value.
    /// Not a valid VA within the PE image. In memory dumps, this field
    /// may be overwritten by the runtime.
    #[inline]
    pub fn linker_type_data_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x1C)
    }

    /// Packed control identifier at offset 0x24.
    ///
    /// Equals `(member_type << 16) | index`. Used as the hash key by
    /// `ControlNameHashLookup` in the runtime. The value 0xFFFFFFFF
    /// indicates the form's default/implicit control.
    #[inline]
    pub fn control_id(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x24)
    }
}

/// One pair of a [`ControlInfo`]'s DISPID map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DispidMapping {
    /// The source interface member's DISPID: an event's number, or an
    /// implemented interface member's DISPID.
    pub source: u32,
    /// The MEMID of the object procedure that handles it.
    pub handler: u32,
}

/// Iterator over control info entries in an object.
///
/// Created from [`OptionalObjectInfo`](crate::vb::object::OptionalObjectInfo)
/// when iterating controls on a form.
#[must_use = "iterators are lazy and do nothing unless consumed"]
pub struct ControlIterator<'a> {
    /// Byte slice spanning the full control info array.
    data: &'a [u8],
    /// Current byte offset into `data`.
    offset: usize,
    /// Number of control entries left to yield.
    remaining: u32,
}

impl<'a> ControlIterator<'a> {
    /// Creates a new iterator over `count` controls starting at `data`.
    ///
    /// # Arguments
    ///
    /// * `data` - Byte slice starting at the first ControlInfo entry.
    /// * `count` - Number of controls to iterate.
    pub fn new(data: &'a [u8], count: u32) -> Self {
        Self {
            data,
            offset: 0,
            remaining: count,
        }
    }
}

impl<'a> Iterator for ControlIterator<'a> {
    type Item = Result<ControlInfo<'a>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        self.remaining = self.remaining.checked_sub(1)?;

        let entry_end = match self.offset.checked_add(ControlInfo::MIN_SIZE) {
            Some(e) => e,
            None => {
                return Some(Err(Error::ArithmeticOverflow {
                    context: "ControlIterator offset+MIN_SIZE",
                }));
            }
        };
        if entry_end > self.data.len() {
            return Some(Err(Error::TooShort {
                expected: ControlInfo::MIN_SIZE,
                actual: self.data.len().saturating_sub(self.offset),
                context: "ControlInfo",
            }));
        }

        let slice = match self.data.get(self.offset..) {
            Some(s) => s,
            None => {
                return Some(Err(Error::Truncated {
                    needed: ControlInfo::MIN_SIZE,
                    available: self.data.len().saturating_sub(self.offset),
                }));
            }
        };
        match ControlInfo::parse(slice) {
            Ok(ctrl) => {
                self.offset = entry_end;
                Some(Ok(ctrl))
            }
            Err(e) => Some(Err(e)),
        }
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_guid_parses_its_display() {
        let text = "{33AD4EE2-6699-11CF-B70C-00AA0060D393}";
        let guid: Guid = text.parse().unwrap();
        assert_eq!(&guid.bytes[..4], &[0xE2, 0x4E, 0xAD, 0x33]);
        assert_eq!(
            &guid.bytes[8..],
            &[0xB7, 0x0C, 0x00, 0xAA, 0x00, 0x60, 0xD3, 0x93]
        );
        assert_eq!(guid.to_string(), text);
        // Either case; every byte value round-trips.
        let lower: Guid = text.to_lowercase().parse().unwrap();
        assert_eq!(lower, guid);
        let all = Guid {
            bytes: core::array::from_fn(|i| (i as u8).wrapping_mul(17)),
        };
        assert_eq!(all.to_string().parse::<Guid>().unwrap(), all);
        // Control: anything else is refused.
        for bad in [
            "33AD4EE2-6699-11CF-B70C-00AA0060D393",
            "{33AD4EE2-6699-11CF-B70C-00AA0060D39}",
            "{33AD4EE2-6699-11CF-B70C00AA0060D393}",
            "{33AD4EE2-6699-11CF-B70C-00AA0060D39G}",
            "{+3AD4EE2-6699-11CF-B70C-00AA0060D393}",
            "",
        ] {
            assert!(
                matches!(bad.parse::<Guid>(), Err(Error::InvalidGuid { .. })),
                "{bad}"
            );
        }
    }
    use super::*;

    fn make_control_info() -> Vec<u8> {
        let mut buf = vec![0u8; ControlInfo::MIN_SIZE];
        buf[0x00..0x02].copy_from_slice(&0x0040u16.to_le_bytes()); // flags
        buf[0x02..0x04].copy_from_slice(&0x0014u16.to_le_bytes()); // event_handler_slots
        buf[0x04..0x06].copy_from_slice(&52u16.to_le_bytes()); // event_count
        buf[0x08..0x0C].copy_from_slice(&0x00405000u32.to_le_bytes()); // guid_va
        buf[0x0C..0x0E].copy_from_slice(&1u16.to_le_bytes()); // index
        buf[0x0E..0x10].copy_from_slice(&3u16.to_le_bytes()); // field_0e
        buf[0x10..0x12].copy_from_slice(&2u16.to_le_bytes()); // DISPID map pairs
        buf[0x14..0x18].copy_from_slice(&0x00406000u32.to_le_bytes()); // DISPID map
        buf[0x20..0x24].copy_from_slice(&0x00407000u32.to_le_bytes()); // name_va (+0x20)
        buf
    }

    #[test]
    fn test_parse_valid() {
        let data = make_control_info();
        let ctrl = ControlInfo::parse(&data).unwrap();
        assert_eq!(ctrl.flags().unwrap(), 0x0040);
        assert_eq!(ctrl.event_handler_slots().unwrap(), 0x0014);
        assert_eq!(ctrl.control_type().unwrap(), 0x00140040);
        assert_eq!(ctrl.dispatch_offset().unwrap(), 52);
        assert_eq!(ctrl.guid_va().unwrap(), 0x00405000);
        assert_eq!(ctrl.index().unwrap(), 1);
        assert_eq!(ctrl.member_type().unwrap(), 3);
        assert_eq!(ctrl.dispid_map_count().unwrap(), 2);
        assert_eq!(ctrl.dispid_map_va().unwrap(), 0x00406000);
        assert_eq!(ctrl.name_va().unwrap(), 0x00407000);
    }

    #[test]
    fn test_parse_too_short() {
        let data = vec![0u8; ControlInfo::MIN_SIZE - 1];
        assert!(matches!(
            ControlInfo::parse(&data),
            Err(Error::TooShort { .. })
        ));
    }

    #[test]
    fn test_control_iterator() {
        // Two controls back-to-back
        let mut data = make_control_info();
        let mut ctrl2 = make_control_info();
        ctrl2[0x0C..0x0E].copy_from_slice(&2u16.to_le_bytes()); // index = 2
        data.extend_from_slice(&ctrl2);

        let iter = ControlIterator::new(&data, 2);
        let results: Vec<_> = iter.collect();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].as_ref().unwrap().index().unwrap(), 1);
        assert_eq!(results[1].as_ref().unwrap().index().unwrap(), 2);
    }

    #[test]
    fn test_control_iterator_empty() {
        let data = vec![0u8; 0];
        let iter = ControlIterator::new(&data, 0);
        let results: Vec<_> = iter.collect();
        assert!(results.is_empty());
    }

    #[test]
    fn test_control_iterator_truncated() {
        // Claim 2 controls but only provide data for 1
        let data = make_control_info();
        let iter = ControlIterator::new(&data, 2);
        let results: Vec<_> = iter.collect();
        assert_eq!(results.len(), 2);
        assert!(results[0].is_ok());
        assert!(results[1].is_err());
    }
}
