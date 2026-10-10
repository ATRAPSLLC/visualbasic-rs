//! GUI control representation with resolved metadata.
//!
//! In VB6, controls are GUI elements placed on forms (buttons, text boxes,
//! list boxes, etc.). Each control has a [`ControlInfo`](crate::vb::control::ControlInfo)
//! structure in the binary that stores its type, event count, name VA, GUID VA,
//! and event handler table VA.
//!
//! [`VbControl`] wraps a raw `ControlInfo` with resolved name, GUID, class
//! identification, and the event handler VAs of its event sink vtable.

use std::borrow::Cow;

use crate::{
    addressmap::AddressMap,
    error::Error,
    util::{read_cstr, read_u32_le},
    vb::{
        control::{ControlInfo, Guid},
        events::EventSinkVtable,
        formdata::{FormControlType, FormDataParser},
    },
};

/// A GUI control on a VB6 form with resolved metadata.
///
/// Constructed by [`ControlEntryIterator`], which resolves the raw
/// [`ControlInfo`] pointers into usable name, GUID, and event table slices.
#[derive(Debug)]
pub struct VbControl<'a> {
    /// Underlying raw ControlInfo structure.
    info: ControlInfo<'a>,
    /// Null-terminated control name (e.g., `"Command1"`).
    name: &'a [u8],
    /// COM CLSID identifying the control class; `None` if the GUID VA was
    /// null or could not be resolved.
    guid: Option<Guid>,
    /// Raw byte slice over the event handler slots of the event sink vtable
    /// (4 bytes per event, after its 0x18-byte header). Empty if the control
    /// has no events, no sink, or the slots cannot be read.
    event_handler_vas: &'a [u8],
    /// Authoritative control type from form binary data (`cType` byte).
    ///
    /// When available, this is more reliable than the GUID, which names
    /// the control's events interface and is unknown for an ActiveX
    /// control the crate has no table for.
    form_control_type: Option<FormControlType>,
}

impl<'a> VbControl<'a> {
    /// Returns the underlying [`ControlInfo`] structure.
    #[inline]
    pub fn info(&self) -> &ControlInfo<'a> {
        &self.info
    }

    /// The control's name as a lossy UTF-8 string (e.g., `"Command1"`,
    /// `"txtName"`).
    ///
    /// Borrows when the underlying bytes are already valid UTF-8 (the
    /// common case). Use [`name_bytes`](Self::name_bytes) for the raw
    /// bytes.
    #[inline]
    pub fn name(&self) -> Cow<'a, str> {
        String::from_utf8_lossy(self.name)
    }

    /// The control's name as raw bytes from the PE image.
    #[inline]
    pub fn name_bytes(&self) -> &'a [u8] {
        self.name
    }

    /// Control type flags.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying ControlInfo field cannot be read.
    #[inline]
    pub fn control_type(&self) -> Result<u32, Error> {
        self.info.control_type()
    }

    /// Number of event handler slots on this control:
    /// [`ControlInfo::event_count`], the events (an `Implements`' interface
    /// members), without a `WithEvents` or `Implements` sink's `IDispatch`
    /// slots.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying ControlInfo fields cannot be read.
    #[inline]
    pub fn event_count(&self) -> Result<u16, Error> {
        self.info.event_count()
    }

    /// Control index within the form.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying ControlInfo field cannot be read.
    #[inline]
    pub fn index(&self) -> Result<u16, Error> {
        self.info.index()
    }

    /// The control's COM CLSID, if the GUID VA could be resolved.
    #[inline]
    pub fn guid(&self) -> Option<&Guid> {
        self.guid.as_ref()
    }

    /// Returns the authoritative control type from form binary data.
    ///
    /// This is the most reliable type identification, derived from the
    /// `cType` byte in the form binary data. Returns `None` when form
    /// data is not available (e.g., native-compiled modules without forms).
    #[inline]
    pub fn form_control_type(&self) -> Option<FormControlType> {
        self.form_control_type
    }

    /// Returns the control class name (e.g., `"CommandButton"`, `"TextBox"`).
    ///
    /// Resolution order:
    /// 1. Form binary data `cType` (authoritative, from [`form_control_type`](Self::form_control_type)),
    ///    unless it is an unknown code
    /// 2. The ControlInfo GUID: the control's events interface (or, for a
    ///    control array, that IID + 1), `"Class"` for a class module's own
    ///    `IClassModuleEvt`
    /// 3. `None` for a hosted ActiveX control or UserControl instance: its
    ///    GUID is a build-generated events IID, which
    ///    [`VbProject::components`](crate::VbProject::components) pairs with
    ///    the control's class
    pub fn class_name(&self) -> Option<&'static str> {
        if let Some(fct) = self.form_control_type
            && !matches!(fct, FormControlType::Unknown(_))
        {
            return Some(fct.name());
        }
        self.guid.as_ref().and_then(|g| g.control_class_name())
    }

    /// Returns the VA of the event handler at `event_index`: handler slot
    /// `event_index` of the control's [`EventSinkVtable`]
    /// ([`EventSinkVtable::handler_va`]), after any `IDispatch` slots.
    ///
    /// A return value of `0` means the event is not handled. A non-zero VA
    /// is the handler's entry stub, filled in on disk
    /// (`tests/fixtures/controls`: `Command1`'s slot 0 is 0x00401928, the
    /// 20-byte stub entering `Command1_Click`; see
    /// [`EventHandlerThunk`](crate::vb::events::EventHandlerThunk)).
    /// `None` if `event_index` is past the slots.
    pub fn event_handler_va(&self, event_index: u16) -> Option<u32> {
        let offset = (event_index as usize).checked_mul(4)?;
        let end = offset.checked_add(4)?;
        if end > self.event_handler_vas.len() {
            return None;
        }
        read_u32_le(self.event_handler_vas, offset).ok()
    }

    /// Resolves and returns the control's [`EventSinkVtable`].
    ///
    /// The event sink vtable contains back-pointers, IUnknown thunks,
    /// and per-event handler VAs. Returns `None` if the vtable VA is
    /// null or cannot be resolved.
    pub fn event_sink<'p>(&self, map: &'p AddressMap<'a>) -> Option<EventSinkVtable<'a>> {
        let va = self.info.event_sink_vtable_va().ok()?;
        if va == 0 {
            return None;
        }
        let slots = self.info.event_handler_slots().ok()?;
        let size = EventSinkVtable::HEADER_SIZE.checked_add((slots as usize).checked_mul(4)?)?;
        let data = map.slice_from_va(va, size).ok()?;
        EventSinkVtable::parse(data, &self.info).ok()
    }

    /// Returns the number of events with handler VAs connected.
    ///
    /// # Errors
    ///
    /// Returns an error if the event handler slot count cannot be read.
    pub fn connected_event_count(&self) -> Result<u16, Error> {
        let mut count: u16 = 0;
        for i in 0..self.event_count()? {
            if self.event_handler_va(i).is_some_and(|va| va != 0) {
                count = count.saturating_add(1);
            }
        }
        Ok(count)
    }
}

/// Iterator over controls on a VB6 object (form).
///
/// Walks the control array starting at `controls_va` from the
/// [`OptionalObjectInfo`](crate::vb::object::OptionalObjectInfo), resolving
/// each entry into a [`VbControl`] with name, GUID, and the handler slots of
/// its event sink vtable.
///
/// When form binary data is available (via [`with_form_data`](Self::with_form_data)),
/// each control's [`form_control_type`](VbControl::form_control_type) is populated
/// with the authoritative `cType` byte from the form data.
#[must_use = "iterators are lazy and do nothing unless consumed"]
pub struct ControlEntryIterator<'a, 'p> {
    /// Address map for VA resolution.
    map: &'p AddressMap<'a>,
    /// Base VA of the control array.
    controls_va: u32,
    /// Current zero-based position in the array.
    index: u32,
    /// Total number of controls on the form.
    total: u32,
    /// Optional form data for authoritative control type identification.
    form_data: Option<&'p FormDataParser<'a>>,
}

impl<'a, 'p> ControlEntryIterator<'a, 'p> {
    /// Creates a new iterator over `total` controls starting at
    /// `controls_va`.
    ///
    /// `total` is capped at the number of
    /// [`ControlInfo`](crate::vb::control::ControlInfo) entries the file
    /// holds from `controls_va` on, so a corrupt count cannot make the walk
    /// outlast the data.
    pub fn new(map: &'p AddressMap<'a>, controls_va: u32, total: u32) -> Self {
        let available =
            map.slice_from_va(controls_va, 0).map_or(0, <[u8]>::len) / ControlInfo::MIN_SIZE;
        Self {
            map,
            controls_va,
            index: 0,
            total: total.min(u32::try_from(available).unwrap_or(u32::MAX)),
            form_data: None,
        }
    }

    /// Attaches parsed form binary data for authoritative control type resolution.
    ///
    /// When set, each yielded [`VbControl`] will have its
    /// [`form_control_type`](VbControl::form_control_type) populated by matching
    /// `ControlInfo.index` to `FormControlRecord.cid`.
    pub fn with_form_data(mut self, form_data: &'p FormDataParser<'a>) -> Self {
        self.form_data = Some(form_data);
        self
    }
}

impl<'a, 'p> Iterator for ControlEntryIterator<'a, 'p> {
    type Item = Result<VbControl<'a>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.total || self.controls_va == 0 {
            return None;
        }

        let offset = self.index.saturating_mul(ControlInfo::MIN_SIZE as u32);
        let entry_va = self.controls_va.wrapping_add(offset);
        self.index = self.index.saturating_add(1);

        let data = match self.map.slice_from_va(entry_va, ControlInfo::MIN_SIZE) {
            Ok(d) => d,
            Err(e) => return Some(Err(e)),
        };

        let info = match ControlInfo::parse(data) {
            Ok(c) => c,
            Err(e) => return Some(Err(e)),
        };

        let name_va = match info.name_va() {
            Ok(v) => v,
            Err(e) => return Some(Err(e)),
        };
        let name: &[u8] = if name_va != 0 {
            let off = match self.map.va_to_offset(name_va) {
                Ok(o) => o,
                Err(e) => return Some(Err(e)),
            };
            match read_cstr(self.map.file(), off) {
                Ok(s) => s,
                Err(e) => return Some(Err(e)),
            }
        } else {
            b""
        };

        let guid_va = match info.guid_va() {
            Ok(v) => v,
            Err(e) => return Some(Err(e)),
        };
        let guid = if guid_va != 0 {
            self.map
                .slice_from_va(guid_va, 16)
                .ok()
                .and_then(Guid::from_bytes)
        } else {
            None
        };

        let sink_va = match info.event_sink_vtable_va() {
            Ok(v) => v,
            Err(e) => return Some(Err(e)),
        };
        let (count, dispatch_slots) = match (info.event_count(), info.dispatch_slots()) {
            (Ok(count), Ok(dispatch_slots)) => (count, dispatch_slots),
            (Err(e), _) | (_, Err(e)) => return Some(Err(e)),
        };
        // The handler slots follow the sink vtable's 0x18-byte header and
        // the IDispatch slots of a dual interface's vtable.
        let event_handler_vas: &[u8] = if sink_va != 0 && count > 0 {
            let size = usize::from(count).saturating_mul(4);
            let skip = u32::from(dispatch_slots).saturating_mul(4);
            sink_va
                .checked_add(EventSinkVtable::HEADER_SIZE as u32)
                .and_then(|va| va.checked_add(skip))
                .and_then(|va| self.map.slice_from_va(va, size).ok())
                .and_then(|data| data.get(..size))
                .unwrap_or(b"")
        } else {
            b""
        };

        let info_index = match info.index() {
            Ok(v) => v,
            Err(e) => return Some(Err(e)),
        };

        // Look up authoritative control type from form data
        let form_control_type = self.form_data.and_then(|fd| {
            fd.control_by_id(info_index as u8)
                .map(|fc| fc.control_type())
        });

        Some(Ok(VbControl {
            info,
            name,
            guid,
            event_handler_vas,
            form_control_type,
        }))
    }
}
