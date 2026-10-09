//! VB6 form property stream decoder.
//!
//! Decodes the property opcode+value streams in form binary data.
//! Each property is looked up by control type and opcode index in
//! build-time generated tables from `data/vb6_control_properties.csv`.
//!
//! The serialization format is determined by the descriptor flags in
//! MSVBVM60.DLL, which the compiler's property writer follows.

use std::fmt;

use crate::{
    util::{read_i16_le, read_i32_le, read_u16_le, read_u32_le},
    vb::formdata::FormControlType,
};

/// Magic value at the start of every StdDataFormat persistence blob.
///
/// This is the first DWORD of the StdDataFormat CLSID
/// `{6B263850-900B-11D0-9484-00A0C91110ED}`, written verbatim as the
/// header signature.
const STD_DATA_FORMAT_MAGIC: u32 = 0x6B263850;

/// VB6 data format type constants (`DataFormatTypeConstants`).
///
/// Determines how the StdDataFormat object applies formatting to
/// data-bound control values. Maps to the `Type` property of the
/// `StdDataFormat` COM object from MSSTDFMT.DLL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataFormatType {
    /// General format - no special formatting applied.
    General = 0,
    /// Number format - uses the format string for numeric display.
    Number = 1,
    /// Currency format.
    Currency = 2,
    /// Short date format.
    ShortDate = 3,
    /// Long date format.
    LongDate = 4,
    /// Custom format - fully user-defined via the format string.
    /// When this type is active, TrueValue/FalseValue/NullValue
    /// VARIANT entries are also serialized.
    Custom = 5,
}

impl DataFormatType {
    /// Converts a raw u32 to a `DataFormatType`.
    pub fn from_u32(v: u32) -> Option<Self> {
        match v {
            0 => Some(Self::General),
            1 => Some(Self::Number),
            2 => Some(Self::Currency),
            3 => Some(Self::ShortDate),
            4 => Some(Self::LongDate),
            5 => Some(Self::Custom),
            _ => None,
        }
    }
}

impl fmt::Display for DataFormatType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::General => write!(f, "General"),
            Self::Number => write!(f, "Number"),
            Self::Currency => write!(f, "Currency"),
            Self::ShortDate => write!(f, "ShortDate"),
            Self::LongDate => write!(f, "LongDate"),
            Self::Custom => write!(f, "Custom"),
        }
    }
}

/// Decoded StdDataFormat COM object from a form binary property stream.
///
/// The VB6 compiler serializes `DataFormat` properties (ser_type 0x16)
/// by calling `IPersistStream::Save` (IID `{00000109-...}`) on the
/// StdDataFormat COM object from MSSTDFMT.DLL.
///
/// # Binary Layout
///
/// The layout is that of the object's `IPersistStream::Save` in
/// MSSTDFMT.DLL 6.01.9839 (its save routine at 0x24DD240C).
///
/// ```text
/// HEADER (0x28 = 40 bytes):
///   +0x00  u32  magic           = 0x6B263850 (CLSID first DWORD, "P8&k")
///   +0x04  u32  version         = 0x60000 | 0x60001 | 0x60002
///   +0x08  u32  format_type     = DataFormatTypeConstants (0-5)
///   +0x0C  u32  reserved1       = 0 (zeroed by constructor)
///   +0x10  u32  reserved2       = 0
///   +0x14  u32  fmt_str_len     = format string length (UTF-16 chars)
///   +0x18  u32  has_custom      = 0 or 1 (1 only when type==Custom)
///   +0x1C  u32  true_val_len    = TrueValue BSTR char count
///   +0x20  u32  false_val_len   = FalseValue BSTR char count
///   +0x24  u32  null_val_len    = NullValue BSTR char count
///
/// FORMAT STRING (variable):
///   [fmt_str_len * 2 bytes]     UTF-16LE format string (e.g., "##,###.00")
///
/// CUSTOM VALUES (only when has_custom != 0):
///   For TrueValue, FalseValue, NullValue:
///     [16 bytes]                VARIANT header (VT at byte 0)
///     If VT == 8 (VT_BSTR):    [len * 2 bytes] BSTR character data
///
/// TRAILER (version-dependent):
///   If version >= 0x60001:      [4 bytes] u32 FirstDayOfWeek
///   If version >= 0x60002:      [4 bytes] u32 FirstWeekOfYear
/// ```
///
/// # Version History
///
/// - `0x60000`: Original format - header + format string + custom values only.
/// - `0x60001`: Adds FirstDayOfWeek trailer field.
/// - `0x60002`: Adds FirstWeekOfYear trailer field (current/most common).
#[derive(Debug, Clone)]
pub struct StdDataFormat {
    /// Persistence format version (0x60000, 0x60001, or 0x60002).
    pub version: u32,
    /// Format type determining how data-bound values are displayed.
    pub format_type: DataFormatType,
    /// Format string (e.g., `"##,###.00"` for Number, `"yyyy-mm-dd"` for dates).
    /// Empty for General type or when no custom format string is set.
    pub format: String,
    /// Whether custom TrueValue/FalseValue/NullValue entries are present.
    /// Only true when `format_type == Custom`.
    pub has_custom_values: bool,
    /// FirstDayOfWeek setting. Present when version >= 0x60001.
    /// Maps to VB6 `vbDayOfWeek` constants (0=system, 1=Sunday..7=Saturday).
    pub first_day_of_week: Option<u32>,
    /// FirstWeekOfYear setting. Present when version >= 0x60002.
    /// Maps to VB6 `vbFirstWeekOfYear` constants.
    pub first_week_of_year: Option<u32>,
    /// Total blob size in bytes consumed from the property stream.
    pub blob_size: u32,
}

/// Property value type encoding for the form binary format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropType {
    /// 1-byte value (booleans, enums, flags).
    Byte,
    /// 2-byte signed integer.
    Int16,
    /// 4-byte value (Long, Color, Single).
    Long,
    /// Variable-length ASCII string: `[u16_le byte_length][string + null]`.
    Str,
    /// Variable-length UTF-16 string (Tag/Connect encoding): `[u16_le char_count][char_count * 2 bytes UTF-16LE]`.
    ///
    /// Used by properties with serialization type 0x0D (Tag, Connect, DatabaseName,
    /// RecordSource): the compiler writes the BSTR's length
    /// (`SysStringLen`), then its characters. No null terminator.
    TagStr,
    /// 16-byte client rectangle: four `i32` (ClientLeft, ClientTop,
    /// ClientWidth, ClientHeight), [`ClientRect`].
    Size16,
    /// 11-byte font descriptor.
    Font,
    /// Picture: 4-byte size then data. `0xFFFFFFFF` = default.
    Picture,
    /// A control's bounds, [`ControlBounds`]: four `i16`, or `0x8000` and
    /// four `i32` (8 or 18 bytes).
    Bounds,
    /// `ScaleMode` with the scale and drawing state stored with it,
    /// [`ScaleState`]: 4 or 20 bytes.
    Scale,
    /// StdDataFormat COM object serialized via IPersistStream::Save.
    ///
    /// The compiler (serialization type 0x16) calls
    /// `IPersistStream::Save(stream, FALSE)` on the StdDataFormat object
    /// from MSSTDFMT.DLL. The persistence format is:
    ///
    /// - 0x28-byte header: magic(4) + version(4) + type(4) + reserved(8) +
    ///   fmt_str_len(4) + has_custom(4) + 3x custom_len(12)
    /// - Format string: `fmt_str_len * 2` bytes UTF-16LE
    /// - If has_custom: 3x VARIANT entries (0x10 each) + optional BSTRs
    /// - Trailer: `first_day_of_week(4)` (if version >= 0x60001) +
    ///   `first_week_of_year(4)` (if version >= 0x60002)
    ///
    /// The layout is MSSTDFMT.DLL 6.01.9839's save and load of the object.
    DataFormat,
    /// Flag-only: opcode is emitted with NO value data following.
    ///
    /// Used for font sub-properties (FontSize, FontBold, FontItalic,
    /// FontStrikethru, FontUnderline) where the descriptor flags have
    /// bits 16-17 both clear. The opcode marks the property as non-default,
    /// but the actual value is embedded in the Font blob (PropType::Font).
    /// The compiler writes a value only when
    /// `(flags & 0x10000) != 0 || (flags & 0x20000) != 0`; when both bits
    /// are clear, only the opcode byte is written to the stream.
    Flag,
}

impl PropType {
    /// Returns the fixed byte size, or `None` for variable-length types.
    pub fn fixed_size(&self) -> Option<usize> {
        match self {
            Self::Flag => Some(0),
            Self::Byte => Some(1),
            Self::Bounds => None, // 8 or 18 bytes
            Self::Scale => None,  // 4 or 20 bytes
            Self::Int16 => Some(2),
            Self::Long => Some(4),
            Self::Size16 => Some(16),
            Self::Font => None,       // 11 base + variable nameLen callback
            Self::DataFormat => None, // StdDataFormat IPersistStream blob
            Self::Str | Self::TagStr | Self::Picture => None,
        }
    }
}

/// Returns the property name and type for a form binary property opcode.
///
/// Property opcodes are **context-dependent** - the same index means different
/// properties for different control types.
///
/// Returns `None` for unknown opcodes.
///
/// Source: `data/vb6_control_properties.csv`, verified against MSVBVM60.DLL descriptor tables.
/// All 22 control types (1038 entries) traced from runtime property pointer tables.
pub fn property_info(ctype: FormControlType, opcode: u8) -> Option<(&'static str, PropType)> {
    generated::lookup_property(ctype.to_u8(), opcode).map(|desc| (desc.name, desc.prop_type))
}

/// Returns the full property descriptor for a form binary property opcode.
///
/// Unlike [`property_info`] which returns only `(name, PropType)`, this
/// provides the serialization type and callback byte count from the
/// MSVBVM60.DLL descriptor metadata.
pub fn property_descriptor(
    ctype: FormControlType,
    opcode: u8,
) -> Option<&'static generated::PropertyDesc> {
    generated::lookup_property(ctype.to_u8(), opcode)
}

/// A control's bounds (Left, Top, Width, Height), the composite its Left
/// property serializes when its descriptor has 4 callback bytes.
///
/// # Binary Layout
///
/// Four `i16` twips values, or, when a value does not fit (`geometry` `Far`:
/// Left 40000), the escape `0x8000` followed by four `i32`:
///
/// ```text
/// +0x00  i16  left     | +0x00  u16  0x8000
/// +0x02  i16  top      | +0x02  i32  left
/// +0x04  i16  width    | +0x06  i32  top
/// +0x06  i16  height   | +0x0A  i32  width
///                      | +0x0E  i32  height
/// ```
///
/// `tests/fixtures/geometry`: `Hidden` (-1200, -600, 1215, 495) in 8
/// bytes, `Far` (40000, 36000, 33000, 495) in 18.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlBounds {
    /// Left edge in twips.
    pub left: i32,
    /// Top edge in twips.
    pub top: i32,
    /// Width in twips.
    pub width: i32,
    /// Height in twips.
    pub height: i32,
}

impl ControlBounds {
    /// The first word that announces four 32-bit values.
    const WIDE: u16 = 0x8000;

    /// Parses the bounds from raw bytes.
    /// Returns the parsed value and bytes consumed, or `None` if too short.
    pub fn parse(data: &[u8]) -> Option<(Self, usize)> {
        if read_u16_le(data, 0).ok()? == Self::WIDE {
            return Some((
                Self {
                    left: read_i32_le(data, 2).ok()?,
                    top: read_i32_le(data, 6).ok()?,
                    width: read_i32_le(data, 10).ok()?,
                    height: read_i32_le(data, 14).ok()?,
                },
                18,
            ));
        }
        Some((
            Self {
                left: i32::from(read_i16_le(data, 0).ok()?),
                top: i32::from(read_i16_le(data, 2).ok()?),
                width: i32::from(read_i16_le(data, 4).ok()?),
                height: i32::from(read_i16_le(data, 6).ok()?),
            },
            8,
        ))
    }
}

impl fmt::Display for ControlBounds {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{},{},{},{}",
            self.left, self.top, self.width, self.height
        )
    }
}

/// Client area rectangle from callback data (ClientLeft/Top/Width/Height).
///
/// Written by the form's `vtable+0x34` callback when the ClientLeft
/// descriptor has 12 callback bytes (B2 bit 1 set, 12B trailing data).
///
/// # Binary Layout (16 bytes)
///
/// ```text
/// +0x00  i32  left    Client area left in twips
/// +0x04  i32  top     Client area top in twips (from callback)
/// +0x08  i32  width   Client area width in twips (from callback)
/// +0x0C  i32  height  Client area height in twips (from callback)
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientRect {
    /// Client area left in twips.
    pub left: i32,
    /// Client area top in twips.
    pub top: i32,
    /// Client area width in twips.
    pub width: i32,
    /// Client area height in twips.
    pub height: i32,
}

impl ClientRect {
    /// Parses a client rectangle from raw bytes.
    /// Returns the parsed value and bytes consumed, or `None` if too short.
    pub fn parse(data: &[u8]) -> Option<(Self, usize)> {
        if data.len() < 16 {
            return None;
        }
        Some((
            Self {
                left: read_i32_le(data, 0).ok()?,
                top: read_i32_le(data, 4).ok()?,
                width: read_i32_le(data, 8).ok()?,
                height: read_i32_le(data, 12).ok()?,
            },
            16,
        ))
    }
}

impl fmt::Display for ClientRect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{},{},{},{}",
            self.left, self.top, self.width, self.height
        )
    }
}

/// The `ScaleMode` property of a form, MDIForm, UserControl, UserDocument,
/// PropertyPage or PictureBox, with the coordinate scale and the drawing
/// flags the record stores with it.
///
/// ```text
/// u8      mode       ScaleMode (0 user, 1 twips, 2 points, 3 pixels,
///                    4 characters, 5 inches, 6 millimetres, 7 centimetres)
/// u8      0
/// mode 0: 4 x f32    ScaleLeft, ScaleTop, twips per unit across and down
/// u8      flags      0x20 AutoRedraw, 0x02 FontTransparent, 0x01 set for a
///                    user or pixel scale; 0x40 is set in every record
/// u8      0
/// ```
///
/// `AutoRedraw` and `FontTransparent` are stored only here: their own
/// table entries carry no data. A user scale is stored as its origin and
/// the size of one unit in twips, not as `ScaleWidth` and `ScaleHeight`
/// (`ScaleWidth = 1000` across a 540-twip client area is 0.54).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScaleState {
    /// The `ScaleMode` value.
    pub mode: u8,
    /// The user scale, when `mode` is 0.
    pub user_scale: Option<UserScale>,
    /// The flags byte.
    pub flags: u8,
}

/// A user-defined coordinate scale (`ScaleMode = 0`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UserScale {
    /// `ScaleLeft`: the x coordinate of the client area's left edge.
    pub left: f32,
    /// `ScaleTop`: the y coordinate of the client area's top edge.
    pub top: f32,
    /// Twips per horizontal unit: the client width over `ScaleWidth`.
    pub twips_per_unit_x: f32,
    /// Twips per vertical unit: the client height over `ScaleHeight`.
    pub twips_per_unit_y: f32,
}

impl ScaleState {
    /// The flag bit for `AutoRedraw`.
    pub const AUTO_REDRAW: u8 = 0x20;
    /// The flag bit for `FontTransparent`.
    pub const FONT_TRANSPARENT: u8 = 0x02;

    /// Parses the state from raw bytes.
    /// Returns the parsed value and bytes consumed, or `None` if too short.
    pub fn parse(data: &[u8]) -> Option<(Self, usize)> {
        let mode = *data.first()?;
        let (user_scale, flags_at) = if mode == 0 {
            let single = |at: usize| read_u32_le(data, at).ok().map(f32::from_bits);
            let scale = UserScale {
                left: single(2)?,
                top: single(6)?,
                twips_per_unit_x: single(10)?,
                twips_per_unit_y: single(14)?,
            };
            (Some(scale), 18)
        } else {
            (None, 2)
        };
        let flags = *data.get(flags_at)?;
        data.get(flags_at.checked_add(1)?)?;
        Some((
            Self {
                mode,
                user_scale,
                flags,
            },
            flags_at.checked_add(2)?,
        ))
    }

    /// Returns `AutoRedraw`.
    pub fn auto_redraw(&self) -> bool {
        self.flags & Self::AUTO_REDRAW != 0
    }

    /// Returns `FontTransparent`.
    pub fn font_transparent(&self) -> bool {
        self.flags & Self::FONT_TRANSPARENT != 0
    }
}

impl fmt::Display for ScaleState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.mode)?;
        if let Some(scale) = self.user_scale {
            write!(
                f,
                " ({}, {}, {}x{} twips)",
                scale.left, scale.top, scale.twips_per_unit_x, scale.twips_per_unit_y
            )?;
        }
        write!(f, " flags=0x{:02X}", self.flags)
    }
}

/// Font descriptor from a form binary property stream.
///
/// The VB6 compiler writes font properties as an 11-byte fixed header
/// followed by a variable-length ASCII font name (from the `vtable+0x34`
/// callback, where the name length is byte 10 of the header).
///
/// # Binary Layout (11 + name_len bytes)
///
/// ```text
/// +0x00  u16  charset         Font character set
/// +0x02  u8   pitch_family    Pitch and font family
/// +0x03  u8   flags           Font flags (italic=0x02, underline=0x04, strikeout=0x08)
/// +0x04  u16  weight          Font weight (400=Normal, 700=Bold)
/// +0x06  u32  size_raw        Font size in 1/10000 pt units
/// +0x0A  u8   name_len        Length of trailing font name (ASCII, no null)
/// +0x0B  [name_len bytes]     Font family name (e.g., "MS Sans Serif")
/// ```
#[derive(Debug, Clone)]
pub struct FontDescriptor {
    /// Font size in points (raw value / 10000).
    pub size_pt: u32,
    /// Whether the font is bold (weight >= 700).
    pub bold: bool,
    /// Font weight (400=Normal, 700=Bold, etc.).
    pub weight: u16,
    /// Font family name (e.g., "MS Sans Serif").
    pub name: String,
}

impl FontDescriptor {
    /// Parses a font descriptor from raw bytes.
    /// Returns the parsed value and bytes consumed, or `None` if too short.
    pub fn parse(data: &[u8]) -> Option<(Self, usize)> {
        if data.len() < 11 {
            return None;
        }
        let weight = read_u16_le(data, 4).ok()?;
        let raw_size = read_u32_le(data, 6).ok()?;
        let name_len = (*data.get(10)?) as usize;
        let mut consumed: usize = 11;
        let name = if name_len > 0 {
            let end = consumed.checked_add(name_len)?;
            if end <= data.len() {
                let s = String::from_utf8_lossy(data.get(consumed..end)?).into_owned();
                consumed = end;
                s
            } else {
                String::new()
            }
        } else {
            String::new()
        };
        Some((
            Self {
                size_pt: raw_size.checked_div(10000).unwrap_or(0),
                bold: weight >= 700,
                weight,
                name,
            },
            consumed,
        ))
    }
}

impl fmt::Display for FontDescriptor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let b = if self.bold { " Bold" } else { "" };
        write!(f, "({}pt{b}, \"{}\")", self.size_pt, self.name)
    }
}

/// Embedded picture/icon data from a form binary property stream.
///
/// Pictures are serialized with a 4-byte size prefix. The size field
/// includes all overhead (OLE type header + inner data). A sentinel
/// value of `0xFFFFFFFF` indicates the default picture.
///
/// # Binary Layout
///
/// ```text
/// +0x00  u32  size   Total picture data size (or 0xFFFFFFFF for default)
/// +0x04  [size bytes] Picture data (OLE header + BMP/ICO/etc.)
/// ```
#[derive(Debug, Clone)]
pub struct PictureData {
    /// Total picture data size in bytes.
    pub size: u32,
    /// Whether the picture contains a BMP ("BM" magic at offset +8).
    pub is_bmp: bool,
    /// Whether this is the default picture sentinel (0xFFFFFFFF).
    pub is_default: bool,
    /// Embedded picture bytes, excluding the 4-byte size prefix.
    ///
    /// Empty when [`is_default`](Self::is_default) is `true`.
    pub bytes: Vec<u8>,
}

impl PictureData {
    /// Parses picture data from raw bytes.
    /// Returns the parsed value and bytes consumed, or `None` if too short.
    pub fn parse(data: &[u8]) -> Option<(Self, usize)> {
        if data.len() < 4 {
            return None;
        }
        let size = read_u32_le(data, 0).ok()?;
        if size == 0xFFFFFFFF {
            return Some((
                Self {
                    size: 0,
                    is_bmp: false,
                    is_default: true,
                    bytes: Vec::new(),
                },
                4,
            ));
        }
        let total = size as usize;
        let consumed = 4usize.checked_add(total)?;
        if consumed > data.len() {
            return None;
        }
        // OLE header is 8 bytes, BMP magic at data start (offset 12 from blob start)
        let bmp_off: usize = 12;
        let is_bmp =
            data.get(bmp_off) == Some(&b'B') && data.get(bmp_off.checked_add(1)?) == Some(&b'M');
        Some((
            Self {
                size,
                is_bmp,
                is_default: false,
                bytes: data.get(4..consumed)?.to_vec(),
            },
            consumed,
        ))
    }

    /// Returns the embedded BMP/ICO bytes, excluding the size prefix.
    ///
    /// Returns `None` for the default-picture sentinel.
    pub fn bytes(&self) -> Option<&[u8]> {
        if self.is_default {
            None
        } else {
            Some(&self.bytes)
        }
    }
}

impl fmt::Display for PictureData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_default {
            write!(f, "default")
        } else if self.is_bmp {
            write!(f, "(BMP, {}B)", self.size)
        } else {
            write!(f, "({}B)", self.size)
        }
    }
}

impl StdDataFormat {
    /// Parses a StdDataFormat IPersistStream blob from raw bytes.
    /// Returns the parsed value and bytes consumed, or `None` if invalid.
    pub fn parse(data: &[u8]) -> Option<(Self, usize)> {
        if data.len() < 0x28 {
            return None;
        }
        if read_u32_le(data, 0).ok()? != STD_DATA_FORMAT_MAGIC {
            return None;
        }
        let version = read_u32_le(data, 4).ok()?;
        let format_type = read_u32_le(data, 8).ok()?;
        let fmt_str_len = read_u32_le(data, 0x14).ok()? as usize;
        let has_custom = read_u32_le(data, 0x18).ok()?;
        let true_val_len = read_u32_le(data, 0x1C).ok()? as usize;
        let false_val_len = read_u32_le(data, 0x20).ok()? as usize;
        let null_val_len = read_u32_le(data, 0x24).ok()? as usize;

        let mut off: usize = 0x28;

        // Format string (UTF-16LE)
        let fmt_byte_len = fmt_str_len.checked_mul(2)?;
        let fmt_end = off.checked_add(fmt_byte_len)?;
        let format = if fmt_str_len > 0 && fmt_end <= data.len() {
            let utf16: Vec<u16> = (0..fmt_str_len)
                .map(|j| {
                    let pos = off.checked_add(j.checked_mul(2)?)?;
                    read_u16_le(data, pos).ok()
                })
                .collect::<Option<Vec<_>>>()?;
            off = fmt_end;
            String::from_utf16_lossy(&utf16)
        } else {
            off = fmt_end;
            String::new()
        };

        // Custom values (3x VARIANT + optional BSTRs)
        if has_custom != 0 {
            // TrueValue: 0x10 header + (len * 2) bytes
            off = off
                .checked_add(0x10)?
                .checked_add(true_val_len.checked_mul(2)?)?;
            off = off
                .checked_add(0x10)?
                .checked_add(false_val_len.checked_mul(2)?)?;
            off = off
                .checked_add(0x10)?
                .checked_add(null_val_len.checked_mul(2)?)?;
        }

        // Trailer (version-dependent)
        let first_day_of_week =
            if version >= 0x60001 && off.checked_add(4).map(|e| e <= data.len()).unwrap_or(false) {
                let v = read_u32_le(data, off).ok()?;
                off = off.checked_add(4)?;
                Some(v)
            } else {
                None
            };
        let first_week_of_year =
            if version >= 0x60002 && off.checked_add(4).map(|e| e <= data.len()).unwrap_or(false) {
                let v = read_u32_le(data, off).ok()?;
                off = off.checked_add(4)?;
                Some(v)
            } else {
                None
            };

        Some((
            Self {
                version,
                format_type: DataFormatType::from_u32(format_type)
                    .unwrap_or(DataFormatType::General),
                format,
                has_custom_values: has_custom != 0,
                first_day_of_week,
                first_week_of_year,
                blob_size: off as u32,
            },
            off,
        ))
    }
}

impl fmt::Display for StdDataFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.format.is_empty() {
            write!(f, "({}, {}B)", self.format_type, self.blob_size)
        } else {
            write!(
                f,
                "({}, \"{}\", {}B)",
                self.format_type, self.format, self.blob_size
            )
        }
    }
}

/// Parses a VB6 ASCII string from a property stream.
///
/// Format: `[u16_le byte_length][string_bytes][null_terminator]`.
/// Used by ser_types 1, 18, 26, and 33 (Name, String, DataMember).
fn parse_ascii_str(data: &[u8]) -> Option<(String, usize)> {
    if data.len() < 2 {
        return None;
    }
    let len = read_u16_le(data, 0).ok()? as usize;
    let end = 2usize.checked_add(len)?;
    if end >= data.len() {
        return None;
    }
    let s = String::from_utf8_lossy(data.get(2..end)?).into_owned();
    Some((s, end.checked_add(1)?)) // +1 null terminator
}

/// Parses a VB6 UTF-16LE string from a property stream.
///
/// Format: `[u16_le char_count][char_count * 2 bytes UTF-16LE]`.
/// No null terminator.
/// Used by ser_type 13 (Tag, Connect, DatabaseName, RecordSource).
fn parse_utf16_str(data: &[u8]) -> Option<(String, usize)> {
    if data.len() < 2 {
        return None;
    }
    let char_count = read_u16_le(data, 0).ok()? as usize;
    let byte_len = char_count.checked_mul(2)?;
    let total = 2usize.checked_add(byte_len)?;
    if total > data.len() {
        return None;
    }
    let utf16: Vec<u16> = (0..char_count)
        .map(|j| {
            let pos = 2usize.checked_add(j.checked_mul(2)?)?;
            read_u16_le(data, pos).ok()
        })
        .collect::<Option<Vec<_>>>()?;
    let s = String::from_utf16_lossy(&utf16);
    Some((s, total))
}

/// A decoded property value from a form binary property stream.
#[derive(Debug, Clone)]
pub enum PropertyValue {
    /// Flag-only - opcode emitted with no value data.
    Flag,
    /// Boolean/enum byte (1 byte).
    Byte(u8),
    /// 16-bit signed integer.
    Int16(i16),
    /// 32-bit signed integer (a Long, or a coordinate in twips).
    Long(i32),
    /// Single-precision float (serialization type 7: `FontSize`,
    /// `ScaleLeft`, `CurrentX`, a UserDocument's `HScrollSmallChange`).
    Single(f32),
    /// OLE color value.
    Color(u32),
    /// ASCII string.
    Str(String),
    /// UTF-16 string (Tag, Connect, DatabaseName, etc.).
    TagStr(String),
    /// Position pair: Left + Top from callback.
    Bounds(ControlBounds),
    /// Client rectangle from callback.
    ClientRect(ClientRect),
    /// `ScaleMode` with the scale and drawing state stored with it.
    Scale(ScaleState),
    /// Font descriptor.
    Font(FontDescriptor),
    /// Embedded picture/icon data.
    Picture(PictureData),
    /// StdDataFormat COM object (from MSSTDFMT.DLL).
    DataFormat(StdDataFormat),
}

impl PropertyValue {
    /// Returns the stable persistence string for this value's discriminator.
    ///
    /// These strings are part of the public API contract and are suitable
    /// for database storage: `"Flag"`, `"Byte"`, `"Int16"`, `"Long"`,
    /// `"Single"`, `"Color"`, `"Str"`, `"TagStr"`, `"Bounds"`,
    /// `"ClientRect"`, `"Font"`, `"Picture"`, and `"DataFormat"`.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Flag => "Flag",
            Self::Byte(_) => "Byte",
            Self::Int16(_) => "Int16",
            Self::Long(_) => "Long",
            Self::Single(_) => "Single",
            Self::Color(_) => "Color",
            Self::Str(_) => "Str",
            Self::TagStr(_) => "TagStr",
            Self::Bounds(_) => "Bounds",
            Self::ClientRect(_) => "ClientRect",
            Self::Scale(_) => "Scale",
            Self::Font(_) => "Font",
            Self::Picture(_) => "Picture",
            Self::DataFormat(_) => "DataFormat",
        }
    }

    /// Returns the embedded BMP/ICO bytes for [`PropertyValue::Picture`].
    ///
    /// Returns `None` for non-picture values and for the default-picture
    /// sentinel.
    pub fn picture_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Picture(pic) => pic.bytes(),
            _ => None,
        }
    }

    /// Formats this value with [`Display`](fmt::Display), truncated to a
    /// maximum number of Unicode scalar values.
    ///
    /// Truncation happens only at character boundaries. No suffix is added,
    /// so the returned string length is at most `limit` characters.
    pub fn display_truncated(&self, limit: usize) -> String {
        let rendered = self.to_string();
        if rendered.chars().count() <= limit {
            return rendered;
        }
        rendered.chars().take(limit).collect()
    }
}

impl fmt::Display for PropertyValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Flag => Ok(()),
            Self::Byte(v) => write!(f, "{v}"),
            Self::Int16(v) => write!(f, "{v}"),
            Self::Long(v) => write!(f, "{v}"),
            Self::Single(v) => write!(f, "{v}"),
            Self::Color(v) => write!(f, "#{v:06X}"),
            Self::Str(s) | Self::TagStr(s) => {
                write!(f, "\"")?;
                for ch in s.chars() {
                    if ch.is_control() {
                        write!(f, "\\x{:02X}", ch as u32)?;
                    } else {
                        write!(f, "{ch}")?;
                    }
                }
                write!(f, "\"")
            }
            Self::Bounds(p) => write!(f, "{p}"),
            Self::ClientRect(r) => write!(f, "{r}"),
            Self::Scale(scale) => write!(f, "{scale}"),
            Self::Font(font) => write!(f, "{font}"),
            Self::Picture(pic) => write!(f, "{pic}"),
            Self::DataFormat(df) => write!(f, "{df}"),
        }
    }
}

/// A single decoded property from a form binary stream.
#[derive(Debug, Clone)]
pub struct Property {
    /// Property name (e.g., "Caption", "BackColor"); `"?"` for an index
    /// the control's table does not describe.
    pub name: &'static str,
    /// The property's index in its control type's table: the byte that
    /// names it in the stream.
    pub index: u8,
    /// Decoded value.
    pub value: PropertyValue,
    /// Byte offset of this property's value within the property stream.
    pub offset: usize,
}

/// Iterator over decoded properties in a form binary property stream.
///
/// Created by [`FormControlRecord::properties`](crate::vb::formdata::FormControlRecord::properties) or
/// [`FormDataParser::form_properties_decoded`](crate::vb::formdata::FormDataParser::form_properties_decoded).
#[must_use = "iterators are lazy and do nothing unless consumed"]
pub struct PropertyIter<'a> {
    data: &'a [u8],
    pos: usize,
    ctype: FormControlType,
}

impl<'a> PropertyIter<'a> {
    /// Creates a new property iterator over the given raw property stream.
    pub fn new(data: &'a [u8], ctype: FormControlType) -> Self {
        Self {
            data,
            pos: 0,
            ctype,
        }
    }

    /// Returns the offset in the stream of the next byte to decode: once
    /// the iterator has ended, that of the `0xFF` terminator when it ended
    /// there.
    pub fn position(&self) -> usize {
        self.pos
    }

    /// Returns `true` if the iterator stopped at the stream's `0xFF`
    /// terminator rather than at a property it could not decode.
    pub fn at_terminator(&self) -> bool {
        self.data.get(self.pos) == Some(&0xFF)
    }

    /// Decodes a single property value based on ser_type and callback_bytes.
    /// Returns `None` if the stream is truncated.
    fn decode_value(&mut self, ser_type: u8, callback_bytes: i8) -> Option<PropertyValue> {
        let d = self.data;
        let p = self.pos;
        let rest = d.get(p..)?;
        match ser_type {
            0 => Some(PropertyValue::Flag),

            // ASCII string (Name, String, DataMember)
            1 | 18 | 26 | 33 => {
                let (s, consumed) = parse_ascii_str(rest)?;
                self.pos = self.pos.checked_add(consumed)?;
                Some(PropertyValue::Str(s))
            }

            // Int16
            2 | 17 => {
                let v = read_i16_le(d, p).ok()?;
                self.pos = self.pos.checked_add(2)?;
                Some(PropertyValue::Int16(v))
            }

            // Long
            3 => {
                let v = read_i32_le(d, p).ok()?;
                self.pos = self.pos.checked_add(4)?;
                Some(PropertyValue::Long(v))
            }

            // Byte
            4 => {
                let v = *rest.first()?;
                self.pos = self.pos.checked_add(1)?;
                Some(PropertyValue::Byte(v))
            }

            // OLE_COLOR
            5 => {
                let v = read_u32_le(d, p).ok()?;
                self.pos = self.pos.checked_add(4)?;
                Some(PropertyValue::Color(v))
            }

            // ScaleMode and its scale state
            6 if callback_bytes == 3 => {
                let (scale, consumed) = ScaleState::parse(rest)?;
                self.pos = self.pos.checked_add(consumed)?;
                Some(PropertyValue::Scale(scale))
            }

            // Enum
            6 => {
                let v = *rest.first()?;
                self.pos = self.pos.checked_add(1)?;
                Some(PropertyValue::Byte(v))
            }

            // Single (`docs` Page: `HScrollSmallChange` 225 is 0x43610000)
            7 => {
                let v = f32::from_bits(read_u32_le(d, p).ok()?);
                self.pos = self.pos.checked_add(4)?;
                Some(PropertyValue::Single(v))
            }

            // Twips Y/H, a 4-byte integer (`geometry`: a Line's Y1 240.25 is
            // stored as 240, a Timer's Top 3480)
            10 | 11 => {
                let v = read_i32_le(d, p).ok()?;
                self.pos = self.pos.checked_add(4)?;
                Some(PropertyValue::Long(v))
            }

            // Twips X/W, a 4-byte integer, or with a callback a control's
            // bounds / a form's client rectangle
            8 | 9 => match callback_bytes {
                4 => {
                    let (bounds, consumed) = ControlBounds::parse(rest)?;
                    self.pos = self.pos.checked_add(consumed)?;
                    Some(PropertyValue::Bounds(bounds))
                }
                12 => {
                    let (rect, consumed) = ClientRect::parse(rest)?;
                    self.pos = self.pos.checked_add(consumed)?;
                    Some(PropertyValue::ClientRect(rect))
                }
                _ => {
                    let v = read_i32_le(d, p).ok()?;
                    self.pos = self.pos.checked_add(4)?;
                    Some(PropertyValue::Long(v))
                }
            },

            // UTF-16LE string (Tag, Connect, DatabaseName, RecordSource)
            13 => {
                let (s, consumed) = parse_utf16_str(rest)?;
                self.pos = self.pos.checked_add(consumed)?;
                Some(PropertyValue::TagStr(s))
            }

            // Font descriptor (11B header + variable name callback)
            20 => {
                let (font, consumed) = FontDescriptor::parse(rest)?;
                self.pos = self.pos.checked_add(consumed)?;
                Some(PropertyValue::Font(font))
            }

            // Picture / Icon
            21 => {
                let (pic, consumed) = PictureData::parse(rest)?;
                self.pos = self.pos.checked_add(consumed)?;
                Some(PropertyValue::Picture(pic))
            }

            // StdDataFormat IPersistStream blob
            22 => {
                let (df, consumed) = StdDataFormat::parse(rest)?;
                self.pos = self.pos.checked_add(consumed)?;
                Some(PropertyValue::DataFormat(df))
            }

            // Unknown ser_type - treat as flag
            _ => Some(PropertyValue::Flag),
        }
    }
}

impl<'a> Iterator for PropertyIter<'a> {
    type Item = Property;

    fn next(&mut self) -> Option<Property> {
        let opcode = *self.data.get(self.pos)?;
        if opcode == 0xFF {
            return None;
        }

        let opcode_offset = self.pos;

        // Known property - decode via ser_type dispatch
        if let Some(desc) = property_descriptor(self.ctype, opcode) {
            self.pos = self.pos.checked_add(1)?; // consume opcode byte
            let value_offset = self.pos;

            if desc.prop_type == PropType::Flag {
                return Some(Property {
                    name: desc.name,
                    index: opcode,
                    value: PropertyValue::Flag,
                    offset: opcode_offset,
                });
            }

            let value = self.decode_value(desc.ser_type, desc.callback_bytes)?;
            return Some(Property {
                name: desc.name,
                index: opcode,
                value,
                offset: value_offset,
            });
        }

        // Unknown opcode - try lookahead to skip flag-like unknowns
        self.pos = self.pos.checked_add(1)?;
        if let Some(&next) = self.data.get(self.pos)
            && (next == 0xFF || property_info(self.ctype, next).is_some())
        {
            return Some(Property {
                name: "?",
                index: opcode,
                value: PropertyValue::Flag,
                offset: opcode_offset,
            });
        }
        None
    }
}

/// Build-time generated property lookup tables.
/// Source: `data/vb6_control_properties.csv`.
pub(crate) mod generated {
    include!(concat!(env!("OUT_DIR"), "/property_generated.rs"));
}
