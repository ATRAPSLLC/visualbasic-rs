//! Function type descriptor (FuncTypDesc) parser.
//!
//! Describes the prototype of a public VB6 function, including its parameter
//! and return types, property kind, and vtable offset. These descriptors are
//! found via the [`PrivateObjectDescriptor`](super::privateobj::PrivateObjectDescriptor)'s
//! `lpFuncTypDescs` pointer array, which runs parallel to the object's method
//! table: entry `i` describes method `i`, and is null for a method with no
//! public prototype (a `Private` or `Friend` procedure).
//!
//! # Layout
//!
//! | Offset | Size | Field |
//! |--------|------|-------|
//! | 0x00 | 1 | `bEntries` - bits 2-7: type-list entries (parameters plus the return value); bits 0-1: property kind |
//! | 0x01 | 1 | `bFlags` - bit 0: has a return value; bits 2-7: 0x3F when the last parameter is a `ParamArray` |
//! | 0x02 | 2 | `wVTableOffset` - COM vtable offset; bit 0 repeats `bFlags` bit 0 (mask off) |
//! | 0x04 | 2 | `iObjectIndex` - signed; -1 (0xFFFF) = no COM object type reference |
//! | 0x06 | 2 | Reserved (always 0) |
//! | 0x08 | 4 | `lpOptionalDefaults` - VA to optional param default values header (see below) |
//! | 0x0C | 2 | `wDispId` - the member's DISPID |
//! | 0x0E | 1 | Always 3 in compiled projects (not the return type) |
//! | 0x0F | 1 | `bFuncFlags` - 0x60 for regular Sub/Function, 0x68 for Property |
//! | 0x10 | 4 | `lpParamNames` - VA to parameter name string pointer array |
//! | 0x20 | n | Type list: `this` (0x1E), each parameter, then the return value |
//!
//! # Property Kind Encoding
//!
//! The low 2 bits of `bEntries`:
//!
//! | Value | Meaning |
//! |-------|---------|
//! | 0 | Regular Sub or Function |
//! | 1 | Property Get |
//! | 2 | Property Let |
//! | 3 | Property Set |
//!
//! The entry count is `bEntries >> 2`: a `Function F(a, b)` has 3 (two
//! parameters and the return value), a `Property Let P(i, v)` has 2, a `Sub`
//! with no parameters 0. Verified against compiled fixtures of known source
//! (`tests/fixtures/calls`, `tests/fixtures/types`).
//!
//! # References
//!
//! - [Gen Digital: Recovery of function prototypes in VB6 executables](https://www.gendigital.com/blog/insights/research/recovery-of-function-prototypes-in-visual-basic-6-executables)
//! - Reverse-engineered from pe\_x86\_vb\_loader sample via BinaryNinja

use std::fmt;

use crate::{
    addressmap::AddressMap,
    error::Error,
    util::{read_cstr, read_u16_le, read_u32_le},
    vb::external::VarType,
};

/// Property type encoded in the lowest 3 bits of `arg_size`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropertyKind {
    /// Not a property (regular Sub/Function).
    None,
    /// Property Get procedure.
    Get,
    /// Property Let procedure.
    Let,
    /// Property Set procedure.
    Set,
    /// Unknown property bits.
    Unknown(u8),
}

impl fmt::Display for PropertyKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => Ok(()),
            Self::Get => write!(f, "Get "),
            Self::Let => write!(f, "Let "),
            Self::Set => write!(f, "Set "),
            Self::Unknown(v) => write!(f, "Prop{v} "),
        }
    }
}

/// View over a function type descriptor (FuncTypDesc).
///
/// Describes a single public function, sub, or property procedure in a
/// VB6 class or form: the fixed header (0x20 bytes) and the type list that
/// follows it.
///
/// # Example
///
/// For a `Property Get ZipName() As Long`:
/// - `arg_count() == 0`
/// - `property_kind() == PropertyKind::Get`
/// - `has_return_type() == true`
/// - `return_type() == Some(ArgType::new(0x28))` (the `[out, retval]` Long)
#[derive(Clone, Copy, Debug)]
pub struct FuncTypDesc<'a> {
    bytes: &'a [u8],
}

impl<'a> FuncTypDesc<'a> {
    /// Minimum size needed to parse the descriptor's header fields.
    pub const MIN_SIZE: usize = 0x14;

    /// Offset of the type list (its first entry is `this`).
    const TYPE_LIST: usize = 0x20;

    /// Parses a FuncTypDesc from the given byte slice.
    ///
    /// Requires at least 20 bytes (`MIN_SIZE`) for the header. The type list
    /// at 0x20 is read from the rest of the slice, so pass the bytes to the
    /// end of the section for [`arg_types`](Self::arg_types) and
    /// [`return_type`](Self::return_type).
    ///
    /// # Errors
    ///
    /// Returns [`Error::TooShort`] if the slice is too short.
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        if data.len() < Self::MIN_SIZE {
            return Err(Error::TooShort {
                expected: Self::MIN_SIZE,
                actual: data.len(),
                context: "FuncTypDesc",
            });
        }
        Ok(Self { bytes: data })
    }

    /// Raw `bEntries` byte at offset 0x00.
    ///
    /// Encodes the type-list entry count (bits 2-7) and the property kind
    /// (bits 0-1).
    #[inline]
    pub fn raw_arg_size(&self) -> u8 {
        self.bytes.first().copied().unwrap_or(0)
    }

    /// Number of type-list entries: the parameters plus the return value.
    #[inline]
    pub fn entry_count(&self) -> u8 {
        self.raw_arg_size() >> 2
    }

    /// Number of explicit parameters.
    ///
    /// Does not include the return value or the `this` pointer.
    #[inline]
    pub fn arg_count(&self) -> u8 {
        self.entry_count()
            .saturating_sub(u8::from(self.has_return_type()))
    }

    /// Property kind encoded in the low 2 bits of `bEntries`.
    pub fn property_kind(&self) -> PropertyKind {
        match self.raw_arg_size() & 0x03 {
            0 => PropertyKind::None,
            1 => PropertyKind::Get,
            2 => PropertyKind::Let,
            _ => PropertyKind::Set,
        }
    }

    /// Returns `true` if this is a Property (Get/Let/Set) rather than Sub/Function.
    #[inline]
    pub fn is_property(&self) -> bool {
        self.raw_arg_size() & 0x03 != 0
    }

    /// Raw flags byte at offset 0x01.
    ///
    /// | Bits | Mask | Meaning |
    /// |------|------|---------|
    /// | 0 | 0x01 | Has a return value |
    /// | 2-7 | 0xFC | 0x3F when the last parameter is a `ParamArray`; otherwise a small count (1 for a procedure with an `Optional ... As Variant` without default, 0 seen elsewhere) |
    #[inline]
    pub fn flags(&self) -> u8 {
        self.bytes.get(1).copied().unwrap_or(0)
    }

    /// Returns `true` if this function has a return type.
    ///
    /// When true, [`return_type`](Self::return_type) provides the type.
    /// Functions (not Subs) and Property Get procedures have return types.
    #[inline]
    pub fn has_return_type(&self) -> bool {
        self.flags() & 0x01 != 0
    }

    /// Returns `true` if the last parameter is a `ParamArray`.
    ///
    /// `bFlags` bits 2-7 hold 0x3F then; the parameter's type is a ByRef
    /// Variant array, passed as one SAFEARRAY pointer.
    #[inline]
    pub fn has_param_array(&self) -> bool {
        self.flags() >> 2 == 0x3F
    }

    /// VTable offset at offset 0x02 (2 bytes, little-endian).
    ///
    /// This is the byte offset into the COM vtable for this method.
    /// Bit 0 is masked off - it indicates "has return type" redundantly
    /// (same as `bFlags` bit 0). Confirmed in `ResolveDispatchToFuncTypDesc`
    /// which reads `*(ftd+2) & 1` to check this flag. The first user method
    /// typically starts at offset 0x1C (after IUnknown + IDispatch = 7 methods).
    #[inline]
    pub fn vtable_offset(&self) -> Result<u16, Error> {
        Ok(read_u16_le(self.bytes, 0x02)? & 0xFFFE)
    }

    /// Object index at offset 0x04 (signed 16-bit).
    ///
    /// -1 (0xFFFF) indicates no COM object type reference. When >= 0, indexes
    /// into the object table for typed object parameters (e.g., a function
    /// returning a specific class type). Always -1 in all tested samples
    /// (104 binaries, 709 objects).
    ///
    /// The runtime copies this field as part of the first 12 bytes during
    /// `ResolveDispatchToFuncTypDesc`, but no consumer was found that
    /// explicitly branches on its value. It may be used by the IDE/debugger
    /// for type resolution or by `ITypeInfo` implementations not in the
    /// runtime hot path.
    #[inline]
    pub fn object_index(&self) -> Result<i16, Error> {
        Ok(read_u16_le(self.bytes, 0x04)? as i16)
    }

    /// VA of optional parameter default values header at offset 0x08.
    ///
    /// Points to an 8-byte header structure used by the runtime's
    /// `OptionalDefaultsNext` (0x660F5FCA) for looking up default values
    /// of optional parameters. **Not** the arg type data (those are inline
    /// at +0x20 - see [`arg_types`](Self::arg_types)).
    ///
    /// # Header Layout (at this VA)
    ///
    /// | Offset | Size | Field |
    /// |--------|------|-------|
    /// | 0x00 | 4 | `dwTotalSize` - bytes in the defaults data area |
    /// | 0x04 | 4 | `lpDefaults` - VA of first default value entry |
    ///
    /// Each default value entry is:
    /// - `u16` VarType code (2=Integer, 3=Long, 8=BSTR, etc.)
    /// - Type-dependent data (BSTR: u16 length + UTF-16LE; others: fixed-size)
    ///
    /// Use [`optional_defaults`](Self::optional_defaults) to parse these.
    #[inline]
    pub fn optional_defaults_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x08)
    }

    /// Method DISPID at offset 0x0C.
    ///
    /// Used by `ResolveDispatchToFuncTypDesc` (0x6600EFC3) in the runtime
    /// for `IDispatch::GetIDsOfNames` resolution. Matches the DISPID that
    /// COM clients use to invoke this method.
    #[inline]
    pub fn dispid(&self) -> Result<u16, Error> {
        read_u16_le(self.bytes, 0x0C)
    }

    /// Return type: the last type-list entry, when the procedure has one.
    ///
    /// The entry is the `[out, retval]` parameter, so it carries the ByRef
    /// bit; [`ArgType::base_type`] gives the value's type. Returns `None` for
    /// a `Sub`, `Property Let` or `Property Set`, and when the type list is
    /// not in the parsed slice.
    pub fn return_type(&self) -> Option<ArgType> {
        if !self.has_return_type() {
            return None;
        }
        self.type_list().last().copied()
    }

    /// Secondary function flags at offset 0x0F.
    ///
    /// **Not read by the runtime.** Exhaustive search of MSVBVM60.DLL
    /// FuncTypDesc consumers (`ResolveDispatchToFuncTypDesc`, `IDispatchInvoke`,
    /// `MarshalDispParamsToNative`, `BuildFuncTypDescHashTable`,
    /// `LookupFuncTypDescByName`) confirmed none access byte +0x0F. The runtime
    /// only copies the first 12 bytes (3 dwords: +0x00..+0x0B) from FuncTypDesc
    /// into dispatch resolution structures.
    ///
    /// Compiler metadata with the following bit layout:
    ///
    /// | Bit | Mask | Meaning |
    /// |-----|------|---------|
    /// | 3 | 0x08 | Property procedure (Get/Let/Set) |
    /// | 5 | 0x20 | Always set |
    /// | 6 | 0x40 | Always set |
    ///
    /// Observed values: `0x60` for regular Sub/Function, `0x68` for Property.
    #[inline]
    pub fn func_flags(&self) -> u8 {
        self.bytes.get(0x0F).copied().unwrap_or(0)
    }

    /// Returns `true` if `func_flags` bit 3 indicates a property procedure.
    ///
    /// Equivalent to [`is_property`](Self::is_property) but derived from the
    /// secondary flags byte rather than the `bArgSize` encoding.
    #[inline]
    pub fn func_flags_is_property(&self) -> bool {
        self.func_flags() & 0x08 != 0
    }

    /// VA of the parameter name string pointer array at offset 0x10.
    ///
    /// Points to an array of VAs, one per parameter. Each VA points to
    /// a null-terminated ANSI parameter name string.
    #[inline]
    pub fn param_names_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x10)
    }

    /// Resolves parameter names from the param names VA array.
    ///
    /// Returns a `Vec` of parameter name byte slices, one per argument.
    /// Names that cannot be resolved (null VA or outside PE) are returned
    /// as empty slices.
    ///
    /// # Arguments
    ///
    /// * `map` - Address map for VA-to-offset resolution.
    pub fn param_names<'b>(&self, map: &AddressMap<'b>) -> Vec<&'b [u8]> {
        let Ok(base) = self.param_names_va() else {
            return Vec::new();
        };
        if base == 0 || self.arg_count() == 0 {
            return Vec::new();
        }
        let count = self.arg_count() as usize;
        let mut names = Vec::with_capacity(count);
        for i in 0..count {
            let ptr_va = base.wrapping_add((i as u32).wrapping_mul(4));
            let name: &'b [u8] = map
                .slice_from_va(ptr_va, 4)
                .ok()
                .and_then(|d| {
                    let name_va = read_u32_le(d, 0).ok()?;
                    if name_va == 0 {
                        return None;
                    }
                    let off = map.va_to_offset(name_va).ok()?;
                    read_cstr(map.file(), off).ok()
                })
                .unwrap_or(b"");
            names.push(name);
        }
        names
    }

    /// Returns the procedure kind keyword for display.
    ///
    /// - `"Sub"` - no return type, not a property
    /// - `"Function"` - has return type, not a property
    /// - `"Property Get"` / `"Property Let"` / `"Property Set"` - property procedures
    pub fn kind_keyword(&self) -> &'static str {
        if self.is_property() {
            match self.property_kind() {
                PropertyKind::Get => "Property Get",
                PropertyKind::Let => "Property Let",
                PropertyKind::Set => "Property Set",
                _ => "Property",
            }
        } else if self.has_return_type() {
            "Function"
        } else {
            "Sub"
        }
    }

    /// Returns the parameter types (not the return value).
    ///
    /// Read from the type list at offset 0x20: its first entry is `this`
    /// (0x1E), then one per parameter, then the return value. Each entry is
    /// one [`ArgType`] byte; an array type is followed by padding to a 4-byte
    /// boundary, and a typed object, record or interface by padding and a
    /// 4-byte descriptor VA (see [`ArgType::object_va`]).
    ///
    /// Empty when the slice the descriptor was parsed from ends before the
    /// type list.
    pub fn arg_types(&self) -> Vec<ArgType> {
        let mut list = self.type_list();
        if self.has_return_type() {
            list.pop();
        }
        list
    }

    /// Returns the evaluation-stack slots a call passes: each parameter's
    /// width ([`ArgType::slots`]) plus one for the return value's pointer.
    ///
    /// The receiver is not counted (the call opcodes push it themselves).
    /// `None` when the type list is not in the parsed slice.
    pub fn arg_slots(&self) -> Option<u16> {
        let list = self.type_list();
        if list.len() != usize::from(self.entry_count()) {
            return None;
        }
        Some(list.iter().map(|t| u16::from(t.slots())).sum())
    }

    /// Returns the frame offset (from ebp) of each parameter inside the
    /// called procedure, and of the return value's pointer last.
    ///
    /// Arguments follow `this` at `ebp+8`, each [`ArgType::slots`] wide.
    pub fn param_offsets(&self) -> Vec<i16> {
        let mut offset: i16 = 0x0C;
        self.type_list()
            .iter()
            .map(|t| {
                let at = offset;
                offset = offset.saturating_add(i16::from(t.slots()).saturating_mul(4));
                at
            })
            .collect()
    }

    /// Parses the type list's entries after `this`: the parameters, then the
    /// return value.
    fn type_list(&self) -> Vec<ArgType> {
        let count = usize::from(self.entry_count());
        let mut types = Vec::with_capacity(count);
        // Entry 0 is `this`; positions are relative to the descriptor.
        let mut pos = Self::TYPE_LIST.saturating_add(1);
        for _ in 0..count {
            let Some(&code) = self.bytes.get(pos) else {
                break;
            };
            let mut t = ArgType::new(code);
            let next = pos.saturating_add(1);
            pos = if t.has_descriptor() {
                let aligned = next.saturating_add(3) & !3;
                t = t.with_descriptor(self.bytes.get(aligned..aligned.saturating_add(4)));
                aligned.saturating_add(4)
            } else if t.is_array() {
                next.saturating_add(3) & !3
            } else {
                next
            };
            types.push(t);
        }
        types
    }

    /// Parses optional parameter default values from the defaults area.
    ///
    /// The header at [`optional_defaults_va`](Self::optional_defaults_va)
    /// holds the defaults area's byte size and VA. Each entry is a u16
    /// VARTYPE then its value: a BSTR as a u16 character count and the
    /// UTF-16LE characters, other types at their natural width. An `Optional`
    /// Variant without a default has no entry. Verified on the `types`
    /// fixture (`Opts(Optional a As Long = 5, Optional st As String = "d",
    /// Optional db As Double = 1.5, Optional bo As Boolean = True, Optional va)`).
    ///
    /// Returns a `Vec` of [`OptionalDefault`] entries in parameter order.
    /// Returns empty if `optional_defaults_va` is 0 or if parsing fails.
    pub fn optional_defaults(&self, map: &AddressMap<'_>) -> Vec<OptionalDefault> {
        let Ok(header_va) = self.optional_defaults_va() else {
            return Vec::new();
        };
        if header_va == 0 {
            return Vec::new();
        }
        let Some((total_size, defaults_va)) = map
            .slice_from_va(header_va, 8)
            .ok()
            .and_then(|hdr| Some((read_u32_le(hdr, 0).ok()?, read_u32_le(hdr, 4).ok()?)))
        else {
            return Vec::new();
        };
        let total_size = total_size as usize;
        if defaults_va == 0 || total_size == 0 {
            return Vec::new();
        }
        let Some(data) = map
            .slice_from_va(defaults_va, total_size)
            .ok()
            .and_then(|d| d.get(..total_size))
        else {
            return Vec::new();
        };

        let mut defaults = Vec::new();
        let mut pos: usize = 0;
        while let Ok(vt_raw) = read_u16_le(data, pos) {
            let vt = VarType::from_raw(vt_raw).unwrap_or(VarType::Empty);
            let value_start = pos.saturating_add(2);
            let (value, len) = if vt == VarType::Bstr {
                let Ok(chars) = read_u16_le(data, value_start) else {
                    break;
                };
                let start = value_start.saturating_add(2);
                let end = start.saturating_add(usize::from(chars).saturating_mul(2));
                let Some(bytes) = data.get(start..end) else {
                    break;
                };
                let units: Vec<u16> = bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|&c| u16::from_le_bytes(c))
                    .collect();
                (
                    DefaultValue::String(String::from_utf16_lossy(&units)),
                    end.saturating_sub(value_start),
                )
            } else {
                let size = vt.data_size();
                let Some(bytes) = data.get(value_start..value_start.saturating_add(size)) else {
                    break;
                };
                (DefaultValue::decode(vt, bytes), size)
            };
            defaults.push(OptionalDefault { vt, vt_raw, value });
            pos = value_start.saturating_add(len);
        }
        defaults
    }
}

/// A parsed optional parameter default value.
#[derive(Debug, Clone)]
pub struct OptionalDefault {
    /// VARIANT type code.
    pub vt: VarType,
    /// Raw VarType code (preserved for unknown types).
    pub vt_raw: u16,
    /// The default value.
    pub value: DefaultValue,
}

/// The actual default value data.
#[derive(Debug, Clone)]
pub enum DefaultValue {
    /// No value (VT_EMPTY, VT_NULL, VT_VARIANT).
    Empty,
    /// Integer value (VT_I2, VT_I4, VT_BOOL, etc.).
    Integer(i64),
    /// Floating-point value (VT_R4, VT_R8, VT_DATE as its serial number).
    Float(f64),
    /// Currency value, scaled by 10000 (VT_CY).
    Currency(i64),
    /// String value (VT_BSTR).
    String(String),
    /// Raw bytes for types we don't decode inline.
    Raw(Vec<u8>),
}

impl DefaultValue {
    /// Decodes a fixed-width value of type `vt` from its bytes.
    fn decode(vt: VarType, bytes: &[u8]) -> Self {
        let raw = || Self::Raw(bytes.to_vec());
        match vt {
            VarType::Empty | VarType::Null | VarType::Variant => Self::Empty,
            VarType::I2 | VarType::Bool | VarType::I1 | VarType::Ui1 | VarType::Ui2 => {
                read_u16_le(bytes, 0).map_or_else(|_| raw(), |v| Self::Integer(i64::from(v as i16)))
            }
            VarType::I4 | VarType::Int | VarType::Uint | VarType::Error => {
                read_u32_le(bytes, 0).map_or_else(|_| raw(), |v| Self::Integer(i64::from(v as i32)))
            }
            VarType::R4 => read_u32_le(bytes, 0)
                .map_or_else(|_| raw(), |v| Self::Float(f64::from(f32::from_bits(v)))),
            VarType::R8 | VarType::Date => bytes
                .get(..8)
                .and_then(|b| <[u8; 8]>::try_from(b).ok())
                .map_or_else(raw, |b| Self::Float(f64::from_le_bytes(b))),
            VarType::Cy => bytes
                .get(..8)
                .and_then(|b| <[u8; 8]>::try_from(b).ok())
                .map_or_else(raw, |b| Self::Currency(i64::from_le_bytes(b))),
            _ => raw(),
        }
    }
}

impl fmt::Display for OptionalDefault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.value {
            DefaultValue::Empty => write!(f, "Empty"),
            DefaultValue::Integer(v) => {
                if self.vt == VarType::Bool {
                    write!(f, "{}", if *v != 0 { "True" } else { "False" })
                } else {
                    write!(f, "{v}")
                }
            }
            DefaultValue::Float(v) => write!(f, "{v}"),
            DefaultValue::Currency(v) => write!(f, "{}@", *v as f64 / 10000.0),
            DefaultValue::String(s) => write!(f, "\"{s}\""),
            DefaultValue::Raw(b) => {
                write!(
                    f,
                    "0x{}",
                    b.iter().map(|x| format!("{x:02X}")).collect::<String>()
                )
            }
        }
    }
}

/// One entry of a FuncTypDesc's type list (offset 0x20): a parameter's or
/// the return value's type.
///
/// **Uses a different numbering than [`VbType`](crate::vb::external::VbType).** Codes verified against
/// compiled fixtures of known source (`tests/fixtures/types`): Boolean 0x03,
/// Byte 0x05, Integer 0x06, Long 0x08, Single 0x0A, Double 0x0B, Date 0x0C,
/// Currency 0x0D, Variant 0x0F, String 0x10, a class 0x13 (with the class's
/// ObjectInfo VA), Object 0x1B.
///
/// # Encoding
///
/// ```text
/// bits 0-4: base type code (see type_name())
/// bit 5 (0x20): ByRef modifier (the `[out, retval]` entry has it too)
/// bit 6 (0x40): Array modifier
/// bit 7 (0x80): Optional parameter
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArgType {
    /// The type byte.
    code: u8,
    /// The descriptor VA that follows a typed object, record or interface
    /// entry; 0 for other types.
    descriptor: u32,
}

impl ArgType {
    /// Creates a type from its type-list byte, without a descriptor.
    #[must_use]
    pub const fn new(code: u8) -> Self {
        Self {
            code,
            descriptor: 0,
        }
    }

    /// Returns the raw type-list byte.
    #[inline]
    pub fn code(self) -> u8 {
        self.code
    }

    /// Returns the VA of the class's ObjectInfo (typed object, 0x13), record
    /// descriptor (0x11, 0x14) or interface (0x1C, 0x1D) that follows the
    /// entry in the type list.
    #[inline]
    pub fn object_va(self) -> Option<u32> {
        (self.descriptor != 0).then_some(self.descriptor)
    }

    /// Returns the evaluation-stack slots (4 bytes each) a call passes for
    /// this parameter.
    ///
    /// By reference and arrays: one pointer. By value: two for Double, Date
    /// and Currency, four for Variant (and Decimal, which only a Variant
    /// holds), one for everything else.
    pub fn slots(self) -> u8 {
        if self.is_byref() || self.is_array() {
            return 1;
        }
        match self.base_type() {
            Self::DOUBLE | Self::DATE | Self::CURRENCY => 2,
            Self::VARIANT | Self::DECIMAL => 4,
            _ => 1,
        }
    }

    /// Returns the type without the ByRef bit: the value type of an
    /// `[out, retval]` entry.
    #[must_use]
    pub fn value_type(self) -> Self {
        Self {
            code: self.code & !Self::BYREF,
            ..self
        }
    }

    /// Returns `true` if a descriptor VA follows the entry.
    fn has_descriptor(self) -> bool {
        matches!(self.base_type(), 0x11 | 0x13 | 0x14 | 0x1C | 0x1D)
    }

    /// Returns the type with the descriptor VA read from `bytes`.
    fn with_descriptor(self, bytes: Option<&[u8]>) -> Self {
        let descriptor = bytes.and_then(|b| read_u32_le(b, 0).ok()).unwrap_or(0);
        Self { descriptor, ..self }
    }

    /// Void / Empty (0x00). Maps to VT_NULL.
    pub const VOID: u8 = 0x00;
    /// Boolean (0x03). Maps to VT_BOOL.
    pub const BOOLEAN: u8 = 0x03;
    /// Signed byte (0x04). Maps to VT_I1.
    pub const SBYTE: u8 = 0x04;
    /// Unsigned byte (0x05). Maps to VT_UI1.
    pub const BYTE: u8 = 0x05;
    /// 16-bit integer (0x06). Maps to VT_I2.
    pub const INTEGER: u8 = 0x06;
    /// Unsigned 16-bit (0x07). Maps to VT_UI2.
    pub const USHORT: u8 = 0x07;
    /// 32-bit integer (0x08). Maps to VT_I4.
    pub const LONG: u8 = 0x08;
    /// Unsigned 32-bit (0x09). Maps to VT_UI4.
    pub const ULONG: u8 = 0x09;
    /// Single-precision float (0x0A). Maps to VT_R4.
    pub const SINGLE: u8 = 0x0A;
    /// Double-precision float (0x0B). Maps to VT_R8.
    pub const DOUBLE: u8 = 0x0B;
    /// Date (0x0C). Maps to VT_DATE.
    pub const DATE: u8 = 0x0C;
    /// Currency (0x0D). Maps to VT_CY.
    pub const CURRENCY: u8 = 0x0D;
    /// Decimal (0x0E). Maps to VT_DECIMAL.
    pub const DECIMAL: u8 = 0x0E;
    /// Variant (0x0F). Maps to VT_VARIANT.
    pub const VARIANT: u8 = 0x0F;
    /// String / BSTR (0x10). Maps to VT_BSTR. **Not 0x08 like VbType!**
    pub const STRING: u8 = 0x10;
    /// User Defined Type (0x11). Followed by extra data.
    pub const UDT: u8 = 0x11;
    /// A class of the project (0x13), followed by its ObjectInfo VA. Maps to
    /// VT_DISPATCH.
    pub const OBJECT: u8 = 0x13;
    /// Record (0x14). Maps to VT_RECORD.
    pub const RECORD: u8 = 0x14;
    /// Dispatch pointer (0x1E). Internal VB type for ByVal object/string refs.
    pub const DISPATCH_PTR: u8 = 0x1E;

    /// ByRef modifier (bit 5). Parameter passed by reference.
    pub const BYREF: u8 = 0x20;
    /// Array modifier (bit 6). Parameter is an array.
    pub const ARRAY: u8 = 0x40;
    /// Optional modifier (bit 7). Parameter has a default value.
    pub const OPTIONAL: u8 = 0x80;

    /// Returns the base type code (bits 0-4).
    #[inline]
    pub fn base_type(self) -> u8 {
        self.code & 0x1F
    }

    /// Returns `true` if this is a ByRef parameter (bit 5).
    #[inline]
    pub fn is_byref(self) -> bool {
        self.code & Self::BYREF != 0
    }

    /// Returns `true` if this is an array type (bit 6).
    #[inline]
    pub fn is_array(self) -> bool {
        self.code & Self::ARRAY != 0
    }

    /// Returns `true` if this is an optional parameter (bit 7).
    #[inline]
    pub fn is_optional(self) -> bool {
        self.code & Self::OPTIONAL != 0
    }

    /// Returns the VB6 type name for the base type.
    pub fn type_name(self) -> &'static str {
        match self.base_type() {
            0x00 | 0x01 => "Void",
            0x02 => "void",
            Self::BOOLEAN => "Boolean",
            Self::SBYTE => "SByte",
            Self::BYTE => "Byte",
            Self::INTEGER => "Integer",
            Self::USHORT => "UShort",
            Self::LONG | 0x1A => "Long",
            Self::ULONG => "ULong",
            Self::SINGLE => "Single",
            Self::DOUBLE => "Double",
            Self::DATE => "Date",
            Self::CURRENCY => "Currency",
            Self::DECIMAL => "Decimal",
            Self::VARIANT => "Variant",
            Self::STRING => "String",
            Self::UDT => "UDT",
            Self::OBJECT => "Class",
            0x1B | 0x1D => "Object",
            Self::RECORD => "Record",
            0x16 => "IDispatch",
            0x1C => "IUnknown",
            Self::DISPATCH_PTR => "DispPtr",
            _ => "Unknown",
        }
    }
}

impl fmt::Display for ArgType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_optional() {
            write!(f, "Optional ")?;
        }
        if self.is_byref() {
            write!(f, "ByRef ")?;
        }
        write!(f, "{}", self.type_name())?;
        if self.is_array() {
            write!(f, "()")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a descriptor: the 0x20-byte header from its first bytes, then
    /// the type list (`this` first).
    fn ftd(header: &[u8], types: &[u8]) -> Vec<u8> {
        let mut data = vec![0u8; 0x20];
        data[..header.len()].copy_from_slice(header);
        data.extend_from_slice(types);
        data
    }

    // tests/fixtures/calls Counter.AddLong(ByVal a As Long, ByVal b As Long) As Long
    const ADD_LONG: [u8; 0x14] = [
        0x0C, 0x01, 0x29, 0x00, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x03,
        0x60, 0xC8, 0x1C, 0x40, 0x00,
    ];

    #[test]
    fn function_with_two_args() {
        let data = ftd(&ADD_LONG, &[0x1E, 0x08, 0x08, 0x28]);
        let f = FuncTypDesc::parse(&data).unwrap();
        assert_eq!(f.entry_count(), 3);
        assert_eq!(f.arg_count(), 2);
        assert_eq!(f.property_kind(), PropertyKind::None);
        assert!(f.has_return_type());
        assert_eq!(f.return_type(), Some(ArgType::new(0x28)));
        assert_eq!(f.arg_types(), vec![ArgType::new(0x08), ArgType::new(0x08)]);
        assert_eq!(f.arg_slots(), Some(3));
        assert_eq!(f.param_offsets(), vec![0x0C, 0x10, 0x14]);
        assert_eq!(f.vtable_offset().unwrap(), 0x0028);
        assert_eq!(f.dispid().unwrap(), 2);
        assert_eq!(f.kind_keyword(), "Function");
    }

    #[test]
    fn property_let_with_index() {
        // tests/fixtures/types Kinds.Item(ByVal row As Long, ByVal col As Long, ByVal value As String)
        let data = ftd(&[0x0E, 0x00, 0x61, 0x00], &[0x1E, 0x08, 0x08, 0x10]);
        let f = FuncTypDesc::parse(&data).unwrap();
        assert_eq!(f.property_kind(), PropertyKind::Let);
        assert_eq!(f.arg_count(), 3);
        assert_eq!(f.return_type(), None);
        assert_eq!(f.kind_keyword(), "Property Let");
        assert_eq!(f.arg_slots(), Some(3));
    }

    #[test]
    fn wide_and_by_value_variant_parameters() {
        // Describe(ByVal v As Variant, ByRef text As String) As Variant, then
        // ScaleBy-like Double and Currency by value.
        let data = ftd(&[0x0C, 0x01], &[0x1E, 0x0F, 0x30, 0x2F]);
        let f = FuncTypDesc::parse(&data).unwrap();
        assert_eq!(f.arg_slots(), Some(4 + 1 + 1));
        assert_eq!(f.param_offsets(), vec![0x0C, 0x1C, 0x20]);
        assert_eq!(ArgType::new(0x0B).slots(), 2);
        assert_eq!(ArgType::new(0x0D).slots(), 2);
        assert_eq!(ArgType::new(0x0C).slots(), 2);
        assert_eq!(ArgType::new(0x2B).slots(), 1);
    }

    #[test]
    fn arrays_align_and_classes_carry_their_object() {
        // Arrays(ByRef al() As Long, ByRef sa() As String, ByRef va() As Variant) As Long()
        let data = ftd(
            &[0x10, 0x01],
            &[0x1E, 0x68, 0, 0, 0x70, 0, 0, 0, 0x6F, 0, 0, 0, 0x68],
        );
        let f = FuncTypDesc::parse(&data).unwrap();
        assert_eq!(
            f.arg_types(),
            vec![ArgType::new(0x68), ArgType::new(0x70), ArgType::new(0x6F)]
        );
        assert_eq!(f.return_type(), Some(ArgType::new(0x68)));
        // K(ByVal x As Kinds) As Kinds: each class entry is followed by the
        // class's ObjectInfo VA at the next 4-byte boundary.
        let data = ftd(
            &[0x08, 0x01],
            &[
                0x1E, 0x13, 0, 0, 0xF4, 0x14, 0x40, 0, 0x33, 0, 0, 0, 0xF4, 0x14, 0x40, 0,
            ],
        );
        let f = FuncTypDesc::parse(&data).unwrap();
        let args = f.arg_types();
        assert_eq!(args.len(), 1);
        assert_eq!(args[0].object_va(), Some(0x004014F4));
        assert_eq!(args[0].type_name(), "Class");
        assert_eq!(
            f.return_type().and_then(|t| t.object_va()),
            Some(0x004014F4)
        );
    }

    #[test]
    fn param_array_flag() {
        // Total(ParamArray values() As Variant) As Long
        let data = ftd(&[0x08, 0xFD], &[0x1E, 0x6F, 0, 0, 0x28]);
        let f = FuncTypDesc::parse(&data).unwrap();
        assert!(f.has_param_array());
        assert!(f.has_return_type());
        assert_eq!(f.arg_types(), vec![ArgType::new(0x6F)]);
        assert_eq!(f.arg_slots(), Some(2));
    }

    #[test]
    fn sub_without_parameters() {
        let data = ftd(&[0x00, 0x00, 0x69, 0x00], &[0x1E]);
        let f = FuncTypDesc::parse(&data).unwrap();
        assert_eq!(f.arg_count(), 0);
        assert_eq!(f.return_type(), None);
        assert_eq!(f.kind_keyword(), "Sub");
        assert_eq!(f.arg_slots(), Some(0));
    }

    #[test]
    fn type_list_outside_the_slice() {
        let f = FuncTypDesc::parse(&ADD_LONG).unwrap();
        assert!(f.arg_types().is_empty());
        assert_eq!(f.arg_slots(), None);
    }

    #[test]
    fn test_parse_too_short() {
        let short = [0u8; 0x13];
        assert!(FuncTypDesc::parse(&short).is_err());
    }

    #[test]
    fn test_arg_type_names() {
        // Arg type encoding is DIFFERENT from VbType
        assert_eq!(ArgType::new(0x10).type_name(), "String");
        assert_eq!(ArgType::new(0x08).type_name(), "Long");
        assert_eq!(ArgType::new(0x06).type_name(), "Integer");
        assert_eq!(ArgType::new(0x03).type_name(), "Boolean");
        assert_eq!(ArgType::new(0x05).type_name(), "Byte");
        assert_eq!(ArgType::new(0x0A).type_name(), "Single");
        assert_eq!(ArgType::new(0x0B).type_name(), "Double");
        assert_eq!(ArgType::new(0x0C).type_name(), "Date");
        assert_eq!(ArgType::new(0x0D).type_name(), "Currency");
        assert_eq!(ArgType::new(0x0F).type_name(), "Variant");
        assert_eq!(ArgType::new(0x1B).type_name(), "Object");
        assert_eq!(ArgType::new(0x13).type_name(), "Class");
    }

    #[test]
    fn test_arg_type_display() {
        assert_eq!(format!("{}", ArgType::new(0x10)), "String");
        assert_eq!(format!("{}", ArgType::new(0x30)), "ByRef String");
        assert_eq!(format!("{}", ArgType::new(0x50)), "String()");
        assert_eq!(format!("{}", ArgType::new(0x70)), "ByRef String()");
        assert_eq!(format!("{}", ArgType::new(0x90)), "Optional String");
    }

    #[test]
    fn test_arg_type_modifiers() {
        let t = ArgType::new(0x70); // ByRef + Array + String
        assert!(t.is_byref());
        assert!(t.is_array());
        assert!(!t.is_optional());
        assert_eq!(t.base_type(), ArgType::STRING);

        let t = ArgType::new(0x90); // Optional + String
        assert!(t.is_optional());
        assert!(!t.is_byref());
        assert!(!t.is_array());
    }
}
