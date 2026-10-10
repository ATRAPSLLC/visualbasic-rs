//! VB6 form binary data parser.
//!
//! Parses the form design data blob at [`GuiTableEntry::form_data_va`](crate::vb::guitable::GuiTableEntry::form_data_va).
//! This blob contains the visual layout of a VB6 form: control hierarchy,
//! property values, embedded images, and menu definitions.
//!
//! # Format Overview
//!
//! ```text
//! [FormDataHeader]              - magic 0xCCFF, GUIDs, dimensions
//! [designer record]             - control id 0, name, cType, properties, 0xFF
//! [controls]                    - optional
//!   0x01 [record]               - opens a level with its first record
//!   0x03 [record]               - the next record at the level
//!   0x02                        - closes the level
//! [menus]                       - optional
//!   0x05 [record]               - opens the menu list with its first record
//!   0x02 [record]               - each further menu record
//!   0x03                        - closes a level
//! 0x04                          - form end
//! ```
//!
//! A control record is followed by `0x01` when it contains controls (a
//! Frame, a PictureBox). A menu record with items carries its table entry
//! 7 ([`FormControlRecord::has_submenu`]) and its items follow it, each
//! after `0x02`, until the `0x03` that closes its level; the last `0x03`
//! closes the menu list. A tree `Top1 { A1, A2 { B1 } }, Top2` is
//! `05 Top1 02 A1 02 A2 02 B1 03 03 02 Top2 03 04` (`tests/fixtures/props`).
//!
//! # Control Type Authority
//!
//! The `cType` byte in each child record is the **authoritative** control
//! type identifier. The GUID in [`ControlInfo`](crate::vb::control::ControlInfo)
//! may contain IID variants that produce incorrect fuzzy matches (verified:
//! 8 of 12 controls misidentified by GUID in the vb_inject malware sample).

use std::{borrow::Cow, fmt};

use crate::{
    error::Error,
    util::{read_u16_le, read_u32_le},
    vb::control::Guid,
    vb::property::PropertyIter,
};

/// Magic marker at the start of form binary data (0xCCFF as u16 LE).
pub const FORM_DATA_MAGIC: u16 = 0xCCFF;

/// Version field following the magic (always 0x0031 = 49).
pub const FORM_DATA_VERSION: u16 = 0x0031;

/// VB6 control type code from the form binary data `cType` byte.
///
/// This is the **authoritative** control type identifier, more reliable
/// than GUID-based identification (which fails for malware samples).
///
/// The type codes are indices into the VB6 compiler's table of intrinsic
/// controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormControlType {
    /// PictureBox control (type 0).
    PictureBox,
    /// Label control (type 1).
    Label,
    /// TextBox control (type 2).
    TextBox,
    /// Frame container control (type 3).
    Frame,
    /// CommandButton control (type 4).
    CommandButton,
    /// CheckBox control (type 5).
    CheckBox,
    /// OptionButton control (type 6).
    OptionButton,
    /// ComboBox control (type 7).
    ComboBox,
    /// ListBox control (type 8).
    ListBox,
    /// Horizontal scrollbar (type 9).
    HScrollBar,
    /// Vertical scrollbar (type 10).
    VScrollBar,
    /// Timer control (type 11).
    Timer,
    /// Form (type 13).
    Form,
    /// DriveListBox control (type 16).
    DriveListBox,
    /// DirListBox control (type 17).
    DirListBox,
    /// FileListBox control (type 18).
    FileListBox,
    /// Menu item (type 19).
    Menu,
    /// MDI Form (type 20).
    MDIForm,
    /// Shape control (type 22).
    Shape,
    /// Line control (type 23).
    Line,
    /// Image control (type 24).
    Image,
    /// Data control (type 37).
    Data,
    /// OLE container (type 38).
    OLE,
    /// UserControl (type 40).
    UserControl,
    /// PropertyPage (type 41).
    PropertyPage,
    /// UserDocument (type 42).
    UserDocument,
    /// Unknown control type.
    Unknown(u8),
}

impl FormControlType {
    /// Converts a raw `cType` byte to a [`FormControlType`].
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::PictureBox,
            1 => Self::Label,
            2 => Self::TextBox,
            3 => Self::Frame,
            4 => Self::CommandButton,
            5 => Self::CheckBox,
            6 => Self::OptionButton,
            7 => Self::ComboBox,
            8 => Self::ListBox,
            9 => Self::HScrollBar,
            10 => Self::VScrollBar,
            11 => Self::Timer,
            13 => Self::Form,
            16 => Self::DriveListBox,
            17 => Self::DirListBox,
            18 => Self::FileListBox,
            19 => Self::Menu,
            20 => Self::MDIForm,
            22 => Self::Shape,
            23 => Self::Line,
            24 => Self::Image,
            37 => Self::Data,
            38 => Self::OLE,
            40 => Self::UserControl,
            41 => Self::PropertyPage,
            42 => Self::UserDocument,
            n => Self::Unknown(n),
        }
    }

    /// Converts a [`FormControlType`] back to its raw `cType` byte.
    pub fn to_u8(&self) -> u8 {
        match self {
            Self::PictureBox => 0,
            Self::Label => 1,
            Self::TextBox => 2,
            Self::Frame => 3,
            Self::CommandButton => 4,
            Self::CheckBox => 5,
            Self::OptionButton => 6,
            Self::ComboBox => 7,
            Self::ListBox => 8,
            Self::HScrollBar => 9,
            Self::VScrollBar => 10,
            Self::Timer => 11,
            Self::Form => 13,
            Self::DriveListBox => 16,
            Self::DirListBox => 17,
            Self::FileListBox => 18,
            Self::Menu => 19,
            Self::MDIForm => 20,
            Self::Shape => 22,
            Self::Line => 23,
            Self::Image => 24,
            Self::Data => 37,
            Self::OLE => 38,
            Self::UserControl => 40,
            Self::PropertyPage => 41,
            Self::UserDocument => 42,
            Self::Unknown(n) => *n,
        }
    }

    /// Converts a control class name to a [`FormControlType`].
    ///
    /// Accepts the names returned by [`Guid::control_class_name()`](crate::vb::control::Guid::control_class_name)
    /// and [`FormControlType::name()`].
    pub fn from_class_name(name: &str) -> Option<Self> {
        match name {
            "PictureBox" => Some(Self::PictureBox),
            "Label" => Some(Self::Label),
            "TextBox" => Some(Self::TextBox),
            "Frame" => Some(Self::Frame),
            "CommandButton" => Some(Self::CommandButton),
            "CheckBox" => Some(Self::CheckBox),
            "OptionButton" => Some(Self::OptionButton),
            "ComboBox" => Some(Self::ComboBox),
            "ListBox" => Some(Self::ListBox),
            "HScrollBar" => Some(Self::HScrollBar),
            "VScrollBar" => Some(Self::VScrollBar),
            "Timer" => Some(Self::Timer),
            "Form" => Some(Self::Form),
            "DriveListBox" => Some(Self::DriveListBox),
            "DirListBox" => Some(Self::DirListBox),
            "FileListBox" => Some(Self::FileListBox),
            "Menu" => Some(Self::Menu),
            "MDIForm" => Some(Self::MDIForm),
            "Shape" => Some(Self::Shape),
            "Line" => Some(Self::Line),
            "Image" => Some(Self::Image),
            "Data" => Some(Self::Data),
            "OLE" => Some(Self::OLE),
            "UserControl" => Some(Self::UserControl),
            "PropertyPage" => Some(Self::PropertyPage),
            "UserDocument" => Some(Self::UserDocument),
            _ => None,
        }
    }

    /// Returns the stable persistence string for this control type.
    ///
    /// These strings are part of the public API contract and are suitable
    /// for database storage. Unknown raw type codes return `"Unknown"`;
    /// use [`to_u8`](Self::to_u8) when the original numeric code must be
    /// preserved as well.
    pub fn name(&self) -> &'static str {
        match self {
            Self::PictureBox => "PictureBox",
            Self::Label => "Label",
            Self::TextBox => "TextBox",
            Self::Frame => "Frame",
            Self::CommandButton => "CommandButton",
            Self::CheckBox => "CheckBox",
            Self::OptionButton => "OptionButton",
            Self::ComboBox => "ComboBox",
            Self::ListBox => "ListBox",
            Self::HScrollBar => "HScrollBar",
            Self::VScrollBar => "VScrollBar",
            Self::Timer => "Timer",
            Self::Form => "Form",
            Self::DriveListBox => "DriveListBox",
            Self::DirListBox => "DirListBox",
            Self::FileListBox => "FileListBox",
            Self::Menu => "Menu",
            Self::MDIForm => "MDIForm",
            Self::Shape => "Shape",
            Self::Line => "Line",
            Self::Image => "Image",
            Self::Data => "Data",
            Self::OLE => "OLE",
            Self::UserControl => "UserControl",
            Self::PropertyPage => "PropertyPage",
            Self::UserDocument => "UserDocument",
            Self::Unknown(_) => "Unknown",
        }
    }

    /// Alias for [`name`](Self::name), matching other discriminator enums.
    pub fn as_str(&self) -> &'static str {
        self.name()
    }

    /// Returns `true` if this control type can contain child controls.
    pub fn is_container(&self) -> bool {
        matches!(self, Self::Frame | Self::PictureBox)
    }
}

impl fmt::Display for FormControlType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown(n) => write!(f, "Unknown({n})"),
            _ => write!(f, "{}", self.name()),
        }
    }
}

/// Hierarchy marker byte in form binary data.
///
/// These are **single bytes** that appear after the `0xFF` property stream
/// terminator. They are NOT 2-byte values (the Semi-VBDecompiler convention
/// of `0x01FF` etc. includes the terminator byte).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormMarker {
    /// `0x01`: First child in a container group.
    NewChild,
    /// `0x02`: End of current child group; in the menu section, the
    /// marker before each menu record after the first.
    EndChildren,
    /// `0x03`: Next sibling at same level; in the menu section, the end of
    /// a menu level.
    Sibling,
    /// `0x04`: End of entire form data.
    FormEnd,
    /// `0x05`: Menu section begins, with its first record.
    MenuStart,
}

impl FormMarker {
    /// Converts a raw byte to a [`FormMarker`], or `None` if not a marker.
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            0x01 => Some(Self::NewChild),
            0x02 => Some(Self::EndChildren),
            0x03 => Some(Self::Sibling),
            0x04 => Some(Self::FormEnd),
            0x05 => Some(Self::MenuStart),
            _ => None,
        }
    }
}

/// View over the form binary data header.
///
/// # Layout (0x61 bytes minimum)
///
/// | Offset | Size | Field |
/// |--------|------|-------|
/// | 0x00 | 2 | Magic (0xCCFF) |
/// | 0x02 | 2 | Version (0x0031) |
/// | 0x04 | 1 | Number of named controls: the highest control index (Board 4, Gauge 1, Form1 2, Host 1, Dial 0 in `tests/fixtures`) |
/// | 0x05 | 16 | Form's own GUI GUID (the GUI table entry's) |
/// | 0x15 | 16 | The object's own full interface IID: built-in designer interface, 256 control getters, then the object's members |
/// | 0x25 | 16 | The designer's events IID (`FormEvents` `{33AD4F3A-...}`, `UserControlEvents` `{33AD5012-...}`) |
/// | 0x35 | 36 | Reserved (zeros) |
/// | 0x59 | 4 | Form width (twips) |
/// | 0x5D | 4 | Form height (twips) |
#[derive(Clone, Copy, Debug)]
pub struct FormDataHeader<'a> {
    bytes: &'a [u8],
}

impl<'a> FormDataHeader<'a> {
    /// Minimum header size in bytes.
    pub const MIN_SIZE: usize = 0x61;

    /// Parses the form data header.
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        let bytes = data.get(..Self::MIN_SIZE).ok_or(Error::TooShort {
            expected: Self::MIN_SIZE,
            actual: data.len(),
            context: "FormDataHeader",
        })?;
        let magic = read_u16_le(bytes, 0x00)?;
        if magic != FORM_DATA_MAGIC {
            let got: [u8; 4] = bytes
                .get(..4)
                .and_then(|s| <[u8; 4]>::try_from(s).ok())
                .unwrap_or([0; 4]);
            return Err(Error::BadMagic {
                expected: "CCFF (form data)",
                got,
            });
        }
        Ok(Self { bytes })
    }

    /// Magic marker at offset 0x00 (should be 0xCCFF).
    #[inline]
    pub fn magic(&self) -> Result<u16, Error> {
        read_u16_le(self.bytes, 0x00)
    }

    /// Version field at offset 0x02 (should be 0x0031).
    #[inline]
    pub fn version(&self) -> Result<u16, Error> {
        read_u16_le(self.bytes, 0x02)
    }

    /// Site count or flags byte at offset 0x04.
    #[inline]
    pub fn site_flags(&self) -> u8 {
        self.bytes.get(0x04).copied().unwrap_or(0)
    }

    /// Form's own GUI GUID at offset 0x05 (16 bytes).
    pub fn form_guid(&self) -> Option<Guid> {
        Guid::from_bytes(self.bytes.get(0x05..0x15)?)
    }

    /// The object's own interface IID at offset 0x15 (16 bytes).
    ///
    /// The interface whose vtable is the built-in designer interface
    /// (`_Form`, 0x2F8 bytes; `_UserControl`, 0x3A4), 256 control getters,
    /// then the object's own members. `VCallHresult` names it for members of
    /// that vtable (`Me.Caption`, `f.Show`): `{80E4A9D0-...}` (Board) and
    /// `{0D54F9E0-...}` (Gauge) in `tests/fixtures/forms`.
    pub fn secondary_guid(&self) -> Option<Guid> {
        let data = self.bytes.get(0x15..0x25)?;
        if data.iter().all(|&b| b == 0) {
            return None;
        }
        Guid::from_bytes(data)
    }

    /// The designer's events IID at offset 0x25 (16 bytes): `FormEvents`
    /// `{33AD4F3A-...}` for a form, `UserControlEvents` `{33AD5012-...}` for
    /// a UserControl, the GUID the object's own ControlInfo (index 0xFFFF)
    /// also names.
    pub fn default_control_guid(&self) -> Option<Guid> {
        Guid::from_bytes(self.bytes.get(0x25..0x35)?)
    }

    /// Form width in twips at offset 0x59.
    #[inline]
    pub fn width(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x59)
    }

    /// Form height in twips at offset 0x5D.
    #[inline]
    pub fn height(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x5D)
    }
}

/// A child control record parsed from form binary data.
///
/// # Record Layout
///
/// ```text
/// [u32 size]                    - total record size (bit 31 = has array index)
/// [u8 cId]                      - control ID (links to ControlInfo.index)
/// [u16_le name_len]             - name string length
/// [name_len + 1 bytes]          - name + null terminator
/// [u8 cType]                    - authoritative control type code
/// [property bytes...]           - opcode + value pairs
/// [0xFF]                        - property stream terminator
/// ```
///
/// For control array members (bit 31 of size set), a 2-byte array index
/// appears between cId and the name.
#[derive(Clone, Debug)]
pub struct FormControlRecord<'a> {
    /// Control ID (links to ControlInfo.index).
    cid: u8,
    /// Control array index, if present.
    array_index: Option<u16>,
    /// Control name (without null terminator).
    name: &'a [u8],
    /// Authoritative control type.
    ctype: FormControlType,
    /// Raw property stream bytes (between cType and 0xFF terminator).
    properties: &'a [u8],
    /// Total record size from the size field (bit 31 masked off).
    total_size: u32,
    /// Nesting depth (0 = top-level form child, 1 = inside a Frame, etc.).
    depth: u16,
    /// Byte offset of this record's size field within the form data blob.
    offset_in_blob: u32,
    /// Byte offset of the property stream within the form data blob.
    properties_offset_in_blob: u32,
    /// Index of the containing parent control in [`FormDataParser::controls`].
    parent_index: Option<usize>,
}

impl<'a> FormControlRecord<'a> {
    /// Parses a single control record starting at the given offset.
    ///
    /// `data` should point to the first byte of the record (the size field),
    /// NOT to the hierarchy marker byte before it.
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        if data.len() < 8 {
            return Err(Error::TooShort {
                expected: 8,
                actual: data.len(),
                context: "FormControlRecord",
            });
        }

        let raw_size = read_u32_le(data, 0)?;
        let has_array_index = raw_size & 0x80000000 != 0;
        let total_size = raw_size & 0x7FFFFFFF;

        let record = data.get(..total_size as usize).ok_or(Error::TooShort {
            expected: total_size as usize,
            actual: data.len(),
            context: "FormControlRecord size",
        })?;
        if total_size < 8 {
            return Err(Error::TooShort {
                expected: 8,
                actual: total_size as usize,
                context: "FormControlRecord size",
            });
        }
        let mut pos: usize = 4; // past size field

        // cId
        let cid = *record.get(pos).ok_or(Error::TooShort {
            expected: pos.saturating_add(1),
            actual: record.len(),
            context: "FormControlRecord cId",
        })?;
        pos = pos.saturating_add(1);

        // Optional array index (if bit 31 was set)
        let array_index = if has_array_index {
            let idx = read_u16_le(record, pos)?;
            pos = pos.saturating_add(2);
            Some(idx)
        } else {
            None
        };

        // Name: [u16_le length] [string bytes + null]
        let name_len = read_u16_le(record, pos)? as usize;
        pos = pos.saturating_add(2);

        let name_end = pos.checked_add(name_len).ok_or(Error::ArithmeticOverflow {
            context: "FormControlRecord name end",
        })?;
        let name = record.get(pos..name_end).ok_or(Error::TooShort {
            expected: name_end.saturating_add(1),
            actual: record.len(),
            context: "FormControlRecord name",
        })?;
        pos = name_end.checked_add(1).ok_or(Error::ArithmeticOverflow {
            context: "FormControlRecord name terminator",
        })?; // skip null terminator

        // cType byte
        let ctype_byte = *record.get(pos).ok_or(Error::TooShort {
            expected: pos.saturating_add(1),
            actual: record.len(),
            context: "FormControlRecord cType",
        })?;
        let ctype = FormControlType::from_u8(ctype_byte);
        pos = pos.saturating_add(1);

        // Property stream: everything from here to the 0xFF terminator
        // The 0xFF should be the last byte of the record
        let tail = record.get(pos..).unwrap_or(&[]);
        let props_end = if let Some(ff_pos) = tail.iter().rposition(|&b| b == 0xFF) {
            pos.saturating_add(ff_pos)
        } else {
            record.len()
        };
        let properties = record.get(pos..props_end).unwrap_or(&[]);

        let properties_offset_local = pos as u32; // offset within this record

        Ok(Self {
            cid,
            array_index,
            name,
            ctype,
            properties,
            total_size,
            depth: 0,
            offset_in_blob: 0,
            properties_offset_in_blob: properties_offset_local,
            parent_index: None,
        })
    }

    /// Returns `true` if this is a menu record with a submenu: its
    /// properties include table entry 7, which the compiler writes on
    /// exactly the menus that have items, and the item records follow it
    /// in the menu section.
    pub fn has_submenu(&self) -> bool {
        self.ctype == FormControlType::Menu && self.properties().any(|p| p.index == 7)
    }

    /// Control ID (links to [`ControlInfo::index`](crate::vb::control::ControlInfo::index)).
    #[inline]
    pub fn cid(&self) -> u8 {
        self.cid
    }

    /// Control array index, if this is a control array member.
    #[inline]
    pub fn array_index(&self) -> Option<u16> {
        self.array_index
    }

    /// Control name as a lossy UTF-8 string (e.g., `"Timer1"`, `"Command1"`).
    ///
    /// Borrows when the underlying bytes are already valid UTF-8.
    /// Use [`name_bytes`](Self::name_bytes) for the raw bytes.
    #[inline]
    pub fn name(&self) -> Cow<'a, str> {
        String::from_utf8_lossy(self.name)
    }

    /// Control name as raw bytes from the form binary.
    #[inline]
    pub fn name_bytes(&self) -> &'a [u8] {
        self.name
    }

    /// Authoritative control type from the form binary data.
    #[inline]
    pub fn control_type(&self) -> FormControlType {
        self.ctype
    }

    /// The ProgID of a hosted control's class as raw bytes: an ActiveX
    /// control's (`MSWinsockLib.Winsock`) or a project's own UserControl's
    /// (`DispId.Dial`).
    ///
    /// A hosted control's record has type 0xFF and begins its properties
    /// with the ProgID, as a `u16` length, the bytes and a NUL
    /// (`tests/fixtures/activex`, `dispid`, `ocx`), whether or not the
    /// control has an event handler. It is the
    /// [`ExternalComponentEntry::prog_id`](crate::vb::external::ExternalComponentEntry::prog_id)
    /// of the control's class. `None` for any other record.
    pub fn prog_id_bytes(&self) -> Option<&'a [u8]> {
        if self.ctype != FormControlType::Unknown(0xFF) {
            return None;
        }
        let length = usize::from(read_u16_le(self.properties, 0).ok()?);
        let end = length.checked_add(2)?;
        let bytes = self.properties.get(2..end)?;
        (self.properties.get(end) == Some(&0)).then_some(bytes)
    }

    /// The ProgID of a hosted control's class
    /// ([`prog_id_bytes`](Self::prog_id_bytes)) as a lossy UTF-8 string.
    pub fn prog_id(&self) -> Option<Cow<'a, str>> {
        self.prog_id_bytes().map(String::from_utf8_lossy)
    }

    /// Raw property stream bytes (opcode+value pairs, excluding the 0xFF terminator).
    #[inline]
    pub fn raw_properties(&self) -> &'a [u8] {
        self.properties
    }

    /// Total record size in bytes (from the size field, bit 31 masked off).
    #[inline]
    pub fn total_size(&self) -> u32 {
        self.total_size
    }

    /// Nesting depth (0 = top-level form child, 1 = inside a Frame, etc.).
    #[inline]
    pub fn depth(&self) -> u16 {
        self.depth
    }

    /// Byte offset of this record's size field within the form data blob.
    ///
    /// Set by [`FormDataParser::parse`] during hierarchy walking.
    #[inline]
    pub fn offset_in_blob(&self) -> u32 {
        self.offset_in_blob
    }

    /// Byte offset of the property stream within the form data blob.
    ///
    /// Use with [`Property::offset`](crate::vb::property::Property::offset) to compute
    /// absolute offsets: `properties_offset_in_blob + prop.offset`.
    #[inline]
    pub fn properties_offset_in_blob(&self) -> u32 {
        self.properties_offset_in_blob
    }

    /// Index of this control's parent in [`FormDataParser::controls`].
    ///
    /// Returns `None` for top-level controls. The index is stable for the
    /// lifetime of the parsed [`FormDataParser`] and refers to the flat
    /// control slice returned by [`FormDataParser::controls`].
    #[inline]
    pub fn parent_index(&self) -> Option<usize> {
        self.parent_index
    }

    /// Decodes the property stream into an iterator of named property values.
    pub fn properties(&self) -> PropertyIter<'a> {
        PropertyIter::new(self.properties, self.ctype)
    }
}

/// Parsed form binary data with header and flat control list.
///
/// Use [`FormDataParser::parse`] to parse the blob at
/// [`GuiTableEntry::form_data_va`](crate::vb::guitable::GuiTableEntry::form_data_va).
pub struct FormDataParser<'a> {
    /// Parsed header.
    header: FormDataHeader<'a>,
    /// Flat list of child control records in parse order.
    controls: Vec<FormControlRecord<'a>>,
    /// Raw form data bytes.
    data: &'a [u8],
    /// The designer object's name, from its own record.
    form_name: &'a [u8],
    /// The designer's control type, from its own record.
    form_type: FormControlType,
    /// Form-level property stream (after the object's own record header, up
    /// to the first child marker).
    form_properties: &'a [u8],
}

/// The designer object's own record, decoded to its terminator.
struct FormRecord<'a> {
    /// The designer's name.
    name: &'a [u8],
    /// The designer's control type.
    ctype: FormControlType,
    /// Its property stream, without the terminator.
    properties: &'a [u8],
    /// The offset of the first hierarchy marker after the terminator.
    markers: usize,
}

impl<'a> FormDataParser<'a> {
    /// Parses form binary data from the given byte slice.
    ///
    /// Returns the header and a flat list of child control records.
    /// The hierarchy (nesting, menus) is preserved in the record order
    /// and can be reconstructed from the marker sequence.
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        let header = FormDataHeader::parse(data)?;
        let (form_name, form_type, form_properties, markers) = match Self::split_form(data) {
            Some(form) => (form.name, form.ctype, form.properties, Some(form.markers)),
            None => {
                let (name, ctype, properties) =
                    Self::split_form_record(Self::extract_form_properties(data));
                (name, ctype, properties, None)
            }
        };
        let controls = Self::parse_controls(data, markers);

        Ok(Self {
            header,
            controls,
            data,
            form_name,
            form_type,
            form_properties,
        })
    }

    /// Splits the designer object's own record (see
    /// [`split_form_record`](Self::split_form_record)) and decodes its
    /// properties to their `0xFF` terminator. `None` if the record does not have that shape or a property does
    /// not decode.
    fn split_form(data: &'a [u8]) -> Option<FormRecord<'a>> {
        let record = data.get(FormDataHeader::MIN_SIZE..)?;
        let (name, ctype, properties) = Self::split_form_record(record);
        if matches!(ctype, FormControlType::Unknown(_)) {
            return None;
        }
        let mut iter = PropertyIter::new(properties, ctype);
        iter.by_ref().for_each(drop);
        if !iter.at_terminator() {
            return None;
        }
        let end = iter.position();
        let start = data.len().checked_sub(properties.len())?;
        Some(FormRecord {
            name,
            ctype,
            properties: properties.get(..end)?,
            markers: start.checked_add(end)?.checked_add(1)?,
        })
    }

    /// Splits the designer object's own record, which has a control
    /// record's layout without the size: control id 0, the name (`u16`
    /// length, the bytes, a NUL), the cType byte, then the properties. A
    /// record that does not have that shape is left whole, with an unknown
    /// type.
    fn split_form_record(record: &'a [u8]) -> (&'a [u8], FormControlType, &'a [u8]) {
        let split = || {
            let (&0, rest) = record.split_first()? else {
                return None;
            };
            let length = usize::from(read_u16_le(rest, 0).ok()?);
            let name = rest.get(2..length.checked_add(2)?)?;
            let after = rest.get(length.checked_add(3)?..)?;
            let (&ctype, properties) = after.split_first()?;
            Some((name, FormControlType::from_u8(ctype), properties))
        };
        split().unwrap_or((&[], FormControlType::Unknown(0), record))
    }

    /// Returns the form data header.
    #[inline]
    pub fn header(&self) -> &FormDataHeader<'a> {
        &self.header
    }

    /// Returns the flat list of child control records.
    #[inline]
    pub fn controls(&self) -> &[FormControlRecord<'a>] {
        &self.controls
    }

    /// Returns the raw form data bytes.
    #[inline]
    pub fn raw_data(&self) -> &'a [u8] {
        self.data
    }

    /// Returns the designer object's name as a lossy UTF-8 string, from its
    /// own record (empty if the record could not be read).
    pub fn form_name(&self) -> Cow<'a, str> {
        String::from_utf8_lossy(self.form_name)
    }

    /// Returns the designer's control type from its own record's cType
    /// byte: [`FormControlType::Form`] (13, also an MDI child form),
    /// `MDIForm` (20), `UserControl` (40), `PropertyPage` (41),
    /// `UserDocument` (42) (`tests/fixtures/{forms,mdi,ocx,docs}`).
    #[inline]
    pub fn form_type(&self) -> FormControlType {
        self.form_type
    }

    /// Returns the form-level property stream bytes: the designer object's
    /// own properties (Caption, BackColor, Font, Icon, ...) after its name
    /// and type, up to the first child marker (or form end).
    #[inline]
    pub fn form_properties(&self) -> &'a [u8] {
        self.form_properties
    }

    /// Decodes the form-level property stream with the property table of
    /// the designer's own type ([`form_type`](Self::form_type)).
    pub fn form_properties_decoded(&self) -> PropertyIter<'a> {
        PropertyIter::new(self.form_properties, self.form_type)
    }

    /// Finds a control record by cId.
    pub fn control_by_id(&self, cid: u8) -> Option<&FormControlRecord<'a>> {
        self.controls.iter().find(|c| c.cid() == cid)
    }

    /// Extracts the form-level property stream between header and first child marker.
    fn extract_form_properties(data: &'a [u8]) -> &'a [u8] {
        let start = FormDataHeader::MIN_SIZE;
        if start >= data.len() {
            return &[];
        }
        // The form property stream runs from after the header until we hit
        // 0xFF followed by a hierarchy marker (0x01-0x05). The 0xFF is the
        // property stream terminator.
        let mut pos = start;
        while pos.saturating_add(1) < data.len() {
            let cur = match data.get(pos).copied() {
                Some(b) => b,
                None => break,
            };
            let next = match data.get(pos.saturating_add(1)).copied() {
                Some(b) => b,
                None => break,
            };
            if cur == 0xFF && FormMarker::from_byte(next).is_some() {
                // Validate: for child markers (0x01, 0x03, 0x05), check
                // that what follows looks like a valid record size
                match next {
                    0x01 | 0x03 | 0x05 if pos.saturating_add(6) < data.len() => {
                        let size = read_u32_le(data, pos.saturating_add(2))
                            .map(|v| v & 0x7FFFFFFF)
                            .unwrap_or(0);
                        if (8..5000).contains(&size) {
                            return data.get(start..pos).unwrap_or(&[]);
                        }
                    }
                    0x04 | 0x02 => return data.get(start..pos).unwrap_or(&[]),
                    _ => {}
                }
            }
            pos = pos.saturating_add(1);
        }
        // No marker found - return everything after header
        let end = data.len().min(start.saturating_add(256));
        data.get(start..end).unwrap_or(&[])
    }

    /// Walks the hierarchy markers and records after the designer's own
    /// record: from `markers` (the offset of the first marker), or, when
    /// the designer's properties did not decode to their terminator, from
    /// the first `0xFF` followed by a plausible marker and record.
    ///
    /// Controls: `0x01` opens a level with its first record, `0x03` is the
    /// next record at the level, `0x02` closes the level. Menus, after the
    /// controls: `0x05` opens the menu list with its first record, `0x02`
    /// precedes each further record, and `0x03` closes a level; a menu
    /// record with a submenu ([`FormControlRecord::has_submenu`]) opens the
    /// level its items follow in. `0x04` ends the form.
    fn parse_controls(data: &'a [u8], markers: Option<usize>) -> Vec<FormControlRecord<'a>> {
        let mut controls = Vec::new();
        let Some(mut pos) = markers.or_else(|| Self::scan_for_markers(data)) else {
            return controls;
        };
        let mut depth: u16 = 0;
        let mut parent_stack: Vec<usize> = Vec::new();
        // Inside the menu section: the open submenus, innermost last.
        let mut menus: Option<Vec<usize>> = None;
        while let Some(&byte) = data.get(pos) {
            pos = pos.saturating_add(1);
            let in_menus = menus.is_some();
            let (depth_of_record, parent) = match (in_menus, byte) {
                (_, 0x04) => break,
                (false, 0x02) => {
                    depth = depth.saturating_sub(1);
                    parent_stack.truncate(usize::from(depth));
                    continue;
                }
                (true, 0x03) => {
                    let open = menus.as_mut().and_then(Vec::pop);
                    if open.is_none() {
                        menus = None;
                    }
                    continue;
                }
                (false, 0x01) => {
                    let level = usize::from(depth);
                    (
                        depth,
                        level
                            .checked_sub(1)
                            .and_then(|l| parent_stack.get(l).copied()),
                    )
                }
                (false, 0x03) => {
                    let record_depth = depth.saturating_sub(1);
                    let level = usize::from(record_depth);
                    (
                        record_depth,
                        level
                            .checked_sub(1)
                            .and_then(|l| parent_stack.get(l).copied()),
                    )
                }
                (false, 0x05) => {
                    menus = Some(Vec::new());
                    (0, None)
                }
                (true, 0x02) => {
                    let open = menus.as_deref().unwrap_or_default();
                    (
                        u16::try_from(open.len()).unwrap_or(u16::MAX),
                        open.last().copied(),
                    )
                }
                _ => break,
            };
            let Some((mut record, end)) = Self::read_record(data, pos) else {
                break;
            };
            pos = end;
            record.depth = depth_of_record;
            record.parent_index = parent;
            let index = controls.len();
            if let Some(open) = menus.as_mut() {
                if record.has_submenu() {
                    open.push(index);
                }
            } else {
                let level = usize::from(depth_of_record);
                parent_stack.truncate(level);
                parent_stack.push(index);
                if byte == 0x01 {
                    depth = depth.saturating_add(1);
                }
            }
            controls.push(record);
        }
        controls
    }

    /// Reads the record whose size field is at `pos`: the record, with its
    /// offsets in the blob set, and the offset after it.
    fn read_record(data: &'a [u8], pos: usize) -> Option<(FormControlRecord<'a>, usize)> {
        let size = usize::try_from(read_u32_le(data, pos).ok()? & 0x7FFF_FFFF).ok()?;
        let end = pos.checked_add(size).filter(|&end| end <= data.len())?;
        if size < 8 {
            return None;
        }
        let mut record = FormControlRecord::parse(data.get(pos..)?).ok()?;
        let at = u32::try_from(pos).ok()?;
        record.offset_in_blob = at;
        record.properties_offset_in_blob = record.properties_offset_in_blob.checked_add(at)?;
        Some((record, end))
    }

    /// Finds the first marker after the designer's record by scanning for
    /// `0xFF` followed by `0x01` or `0x05` and a record of plausible size
    /// (8 to 5000 bytes), or by `0x04` (no controls).
    fn scan_for_markers(data: &[u8]) -> Option<usize> {
        let mut pos = FormDataHeader::MIN_SIZE;
        while pos.saturating_add(6) < data.len() {
            let cur = data.get(pos).copied().unwrap_or(0);
            let next = data.get(pos.saturating_add(1)).copied().unwrap_or(0);
            if cur == 0xFF && matches!(next, 0x01 | 0x05) {
                let size = read_u32_le(data, pos.saturating_add(2))
                    .map(|v| v & 0x7FFF_FFFF)
                    .unwrap_or(0);
                let end_check = (size as usize).saturating_add(pos.saturating_add(2));
                if (8..5000).contains(&size) && end_check <= data.len() {
                    return Some(pos.saturating_add(1));
                }
            }
            if cur == 0xFF && next == 0x04 {
                return None;
            }
            pos = pos.saturating_add(1);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_form_control_type_from_u8() {
        assert_eq!(FormControlType::from_u8(0), FormControlType::PictureBox);
        assert_eq!(FormControlType::from_u8(1), FormControlType::Label);
        assert_eq!(FormControlType::from_u8(11), FormControlType::Timer);
        assert_eq!(FormControlType::from_u8(13), FormControlType::Form);
        assert_eq!(FormControlType::from_u8(19), FormControlType::Menu);
        assert_eq!(FormControlType::from_u8(42), FormControlType::UserDocument);
        assert_eq!(FormControlType::from_u8(99), FormControlType::Unknown(99));
    }

    #[test]
    fn test_form_control_type_name() {
        assert_eq!(FormControlType::Timer.name(), "Timer");
        assert_eq!(FormControlType::CommandButton.name(), "CommandButton");
        assert_eq!(FormControlType::Unknown(55).name(), "Unknown");
    }

    #[test]
    fn test_form_control_type_display() {
        assert_eq!(format!("{}", FormControlType::Label), "Label");
        assert_eq!(format!("{}", FormControlType::Unknown(99)), "Unknown(99)");
    }

    #[test]
    fn test_form_control_type_is_container() {
        assert!(FormControlType::Frame.is_container());
        assert!(FormControlType::PictureBox.is_container());
        assert!(!FormControlType::Timer.is_container());
        assert!(!FormControlType::Label.is_container());
    }

    #[test]
    fn test_form_marker_from_byte() {
        assert_eq!(FormMarker::from_byte(0x01), Some(FormMarker::NewChild));
        assert_eq!(FormMarker::from_byte(0x02), Some(FormMarker::EndChildren));
        assert_eq!(FormMarker::from_byte(0x03), Some(FormMarker::Sibling));
        assert_eq!(FormMarker::from_byte(0x04), Some(FormMarker::FormEnd));
        assert_eq!(FormMarker::from_byte(0x05), Some(FormMarker::MenuStart));
        assert_eq!(FormMarker::from_byte(0x00), None);
        assert_eq!(FormMarker::from_byte(0xFF), None);
    }

    // Real data: Timer1 from pe_x86_vb_loader (33 bytes, verified in spec)
    #[test]
    fn test_parse_timer_record() {
        let record: [u8; 33] = [
            0x21, 0x00, 0x00, 0x00, // size = 33
            0x01, // cId = 1
            0x06, 0x00, // name_len = 6
            0x54, 0x69, 0x6D, 0x65, 0x72, 0x31, 0x00, // "Timer1\0"
            0x0B, // cType = 11 (Timer)
            // Properties:
            0x02, 0x00, // prop[2]=Byte, value=0 (Enabled=False)
            0x03, 0x20, 0x4E, 0x00, 0x00, // prop[3]=Long, value=20000 (Interval)
            0x07, 0x78, 0x00, 0x00, 0x00, // prop[7]=Long, value=120 (Left)
            0x08, 0x78, 0x00, 0x00, 0x00, // prop[8]=Long, value=120 (Top)
            0xFF, // terminator
        ];

        let ctrl = FormControlRecord::parse(&record).unwrap();
        assert_eq!(ctrl.cid(), 1);
        assert_eq!(ctrl.array_index(), None);
        assert_eq!(ctrl.name_bytes(), b"Timer1");
        assert_eq!(ctrl.name(), "Timer1");
        assert_eq!(ctrl.control_type(), FormControlType::Timer);
        assert_eq!(ctrl.total_size(), 33);
        assert_eq!(ctrl.raw_properties().len(), 17); // 33 - 15 (header) - 1 (0xFF)
    }

    #[test]
    fn test_parse_control_array_record() {
        // Simulated control array member (bit 31 set, array_index present)
        let record: [u8; 16] = [
            0x10, 0x00, 0x00, 0x80, // size = 16 | 0x80000000
            0x05, // cId = 5
            0x03, 0x00, // array_index = 3
            0x03, 0x00, // name_len = 3
            0x42, 0x74, 0x6E, 0x00, // "Btn\0"
            0x04, // cType = 4 (CommandButton)
            0x02, // property data
            0xFF, // terminator
        ];

        let ctrl = FormControlRecord::parse(&record).unwrap();
        assert_eq!(ctrl.cid(), 5);
        assert_eq!(ctrl.array_index(), Some(3));
        assert_eq!(ctrl.name_bytes(), b"Btn");
        assert_eq!(ctrl.name(), "Btn");
        assert_eq!(ctrl.control_type(), FormControlType::CommandButton);
    }
}
