//! External table and import descriptor structures.
//!
//! VB6 executables resolve `Declare`d DLL functions at runtime through
//! `DllFunctionCall` (export of MSVBVM60.DLL) rather than through the
//! conventional PE import table. Two tables describe external references:
//!
//! ```text
//! ProjectData +0x234 (table VA), +0x238 (count)
//!   └── ExternalTableEntry[]   (8 bytes each)
//!         ├── type 7: ExternalDeclareInfo  (the DllFunctionCall descriptor)
//!         │     ├── lpLibraryName  -> DLL name string
//!         │     └── lpFunctionName -> API function name string
//!         └── type 6: ExternalTypelibInfo  (lpGuid -> 16-byte GUID)
//!
//! VbHeader +0x50 (table VA), +0x46 (count)
//!   └── ExternalComponentEntry   (variable length, self-relative offsets)
//! ```
//!
//! # API Call Mechanism
//!
//! When P-Code calls a `Declare`d function with an `ImpAdCall*` opcode:
//! 1. The opcode references a constant pool entry
//! 2. The pool entry is the VA of a native stub that jumps to the address
//!    cached by an earlier call, or pushes the descriptor and calls
//!    `DllFunctionCall` (see [`resolve_api_stub`])
//! 3. The descriptor ([`CallApiStub`], the same bytes as the external
//!    table's [`ExternalDeclareInfo`]) points to the DLL name and function name
//! 4. `DllFunctionCall` resolves via `LoadLibraryA`/`GetProcAddress` and
//!    caches the module handle and address
//!
//! `ImpAdCall*` pool entries also point to other native entry points, such
//! as a module procedure's thunk (`mov edx, <ProcDscInfo>; mov ecx,
//! <ProcCallEngine>; jmp ecx`, `tests/fixtures/hello`).

use std::{borrow::Cow, fmt, str};

use crate::{
    addressmap::AddressMap,
    error::Error,
    util::{read_cstr, read_i32_le, read_u16_le, read_u32_le},
    vb::control::Guid,
};

/// View over a CallAPI stub structure (the `DllFunctionCall` descriptor).
///
/// This is the structure the compiler-emitted native stub of a `Declare`d
/// function pushes before calling `DllFunctionCall`; the external table's
/// `Declare` entry points to the same bytes (in the fixtures the
/// [`ExternalTableEntry::external_object_va`] of each `Declare` equals the
/// VA its stub pushes; see [`ExternalDeclareInfo`]). The VB6 runtime reads it
/// in the `DllFunctionCall` worker to resolve the import lazily via
/// `LoadLibraryA` + `GetProcAddress`.
///
/// # Layout
///
/// Verified against the `DllFunctionCall` worker (MSVBVM60 6.00.8176
/// 0x6602675a, 6.00.9848 0x660315de): it dereferences `+0x00` for
/// `LoadLibraryA`, `+0x04` for `GetProcAddress` by name, `+0x08` for
/// `GetProcAddress` by ordinal (when the by-ordinal flag is set), and `+0x0C`
/// as a writable resolve cache.
///
/// | Offset | Size | Field | Runtime use |
/// |--------|------|-------|-------------|
/// | 0x00 | 4 | `lpLibraryName` (VA to null-terminated DLL name) | `LoadLibraryA` |
/// | 0x04 | 4 | `lpFunctionName` (VA to null-terminated API name) | `GetProcAddress` by name |
/// | 0x08 | 2 | `wOrdinal` (import ordinal) | `GetProcAddress` by ordinal |
/// | 0x0A | 2 | `wFlags` - **bit 1 (`0x02`) = resolve by ordinal** | path selector |
/// | 0x0C | 4 | `lpResolveCache` (`+0x04` HMODULE, `+0x08` proc addr) | populated at first call |
///
/// Only the first 8 bytes ([`SIZE`](Self::SIZE)) are required; the ordinal and
/// flag fields ([`FULL_SIZE`](Self::FULL_SIZE)) are read opportunistically by
/// [`resolve_api_stub`] and degrade to errors if the backing data is shorter.
#[derive(Clone, Copy, Debug)]
pub struct CallApiStub<'a> {
    /// Raw backing bytes borrowed from the PE file buffer.
    bytes: &'a [u8],
}

impl<'a> CallApiStub<'a> {
    /// Minimum size to resolve the DLL and function name pointers (8 bytes).
    pub const SIZE: usize = 8;

    /// Full descriptor size including the ordinal, flags, and resolve cache.
    ///
    /// [`resolve_api_stub`] reads this many bytes when available so the
    /// [`ordinal`](Self::ordinal) and [`is_by_ordinal`](Self::is_by_ordinal)
    /// accessors have data to work with.
    pub const FULL_SIZE: usize = 0x10;

    /// Bit in [`flags`](Self::flags) indicating the import resolves by ordinal.
    ///
    /// When set, the runtime ignores [`function_name_va`](Self::function_name_va)
    /// and calls `GetProcAddress` with [`ordinal`](Self::ordinal) instead - the
    /// API name is absent from the binary, a common API-hiding technique.
    pub const FLAG_BY_ORDINAL: u16 = 0x0002;

    /// Parses a CallApiStub from the given byte slice.
    ///
    /// Retains up to [`FULL_SIZE`](Self::FULL_SIZE) bytes when available so the
    /// ordinal and flag accessors have backing data; only the first
    /// [`SIZE`](Self::SIZE) bytes are required.
    ///
    /// # Errors
    ///
    /// Returns [`Error::TooShort`] if `data.len() < 8`.
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        if data.len() < Self::SIZE {
            return Err(Error::TooShort {
                expected: Self::SIZE,
                actual: data.len(),
                context: "CallApiStub",
            });
        }
        let end = data.len().min(Self::FULL_SIZE);
        let bytes = data.get(..end).ok_or(Error::TooShort {
            expected: Self::SIZE,
            actual: data.len(),
            context: "CallApiStub",
        })?;
        Ok(Self { bytes })
    }

    /// Virtual address of the DLL name string at offset 0x00.
    #[inline]
    pub fn library_name_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x00)
    }

    /// Virtual address of the API function name string at offset 0x04.
    #[inline]
    pub fn function_name_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x04)
    }

    /// Import ordinal at offset 0x08.
    ///
    /// Only meaningful when [`is_by_ordinal`](Self::is_by_ordinal) is `true`;
    /// otherwise the import resolves by name and this field is typically `0`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::TooShort`] if the descriptor was resolved from only
    /// the minimum [`SIZE`](Self::SIZE) bytes (no ordinal/flags available).
    #[inline]
    pub fn ordinal(&self) -> Result<u16, Error> {
        read_u16_le(self.bytes, 0x08)
    }

    /// Resolution flags at offset 0x0A.
    ///
    /// See [`FLAG_BY_ORDINAL`](Self::FLAG_BY_ORDINAL), the only bit the
    /// `DllFunctionCall` worker tests. Every `Declare` of the fixtures (by
    /// name) has 0x0004 here; that bit's meaning is unknown.
    ///
    /// # Errors
    ///
    /// Returns [`Error::TooShort`] if the descriptor was resolved from only
    /// the minimum [`SIZE`](Self::SIZE) bytes.
    #[inline]
    pub fn flags(&self) -> Result<u16, Error> {
        read_u16_le(self.bytes, 0x0A)
    }

    /// Returns `true` if this import resolves by ordinal rather than by name.
    ///
    /// By-ordinal imports omit the API name from the binary (the runtime calls
    /// `GetProcAddress` with the raw [`ordinal`](Self::ordinal)), which is a
    /// common name-hiding technique worth surfacing during triage. Returns
    /// `false` if the flags field could not be read (descriptor too short).
    #[inline]
    pub fn is_by_ordinal(&self) -> bool {
        self.flags().is_ok_and(|f| f & Self::FLAG_BY_ORDINAL != 0)
    }

    /// Resolves the DLL library name as a lossy UTF-8 string.
    ///
    /// Use [`library_name_bytes`](Self::library_name_bytes) for raw bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if the VA cannot be resolved.
    pub fn library_name(&self, map: &AddressMap<'a>) -> Result<Cow<'a, str>, Error> {
        Ok(String::from_utf8_lossy(self.library_name_bytes(map)?))
    }

    /// Resolves the DLL library name as raw bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if the VA cannot be resolved.
    pub fn library_name_bytes(&self, map: &AddressMap<'a>) -> Result<&'a [u8], Error> {
        let va = self.library_name_va()?;
        if va == 0 {
            return Ok(b"");
        }
        let offset = map.va_to_offset(va)?;
        read_cstr(map.file(), offset)
    }

    /// Resolves the API function name as a lossy UTF-8 string.
    ///
    /// Use [`function_name_bytes`](Self::function_name_bytes) for raw bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if the VA cannot be resolved.
    pub fn function_name(&self, map: &AddressMap<'a>) -> Result<Cow<'a, str>, Error> {
        Ok(String::from_utf8_lossy(self.function_name_bytes(map)?))
    }

    /// Resolves the API function name as raw bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if the VA cannot be resolved.
    pub fn function_name_bytes(&self, map: &AddressMap<'a>) -> Result<&'a [u8], Error> {
        let va = self.function_name_va()?;
        if va == 0 {
            return Ok(b"");
        }
        let offset = map.va_to_offset(va)?;
        read_cstr(map.file(), offset)
    }
}

/// Resolves an API call stub from the constant pool.
///
/// Two stub shapes are accepted. The first is a `push` of the descriptor
/// followed by a jump (none of the fixtures has this form):
///
/// ```x86asm
/// push offset CallApiStruct    ; 0x68 <imm32>
/// jmp  DllFunctionCall          ; 0xE9 <rel32>  (or 0xFF 0x25 for indirect)
/// ```
///
/// The stubs of the P-Code fixtures (`exprs`, `types`, `vtable`) first try
/// the address an earlier call cached at the descriptor's cache + 8, then
/// push the descriptor and call the `jmp [__imp_DllFunctionCall]` thunk:
///
/// ```x86asm
/// mov  eax, [cache]             ; 0xA1 <imm32>
/// or   eax, eax                 ; 0x0B 0xC0
/// jz   +2                       ; 0x74 0x02
/// jmp  eax                      ; 0xFF 0xE0
/// push offset CallApiStruct    ; 0x68 <imm32>
/// mov  eax, <thunk>             ; 0xB8 <imm32>
/// call eax                      ; 0xFF 0xD0
/// ```
///
/// This function reads the stub at the given VA, extracts the
/// [`CallApiStub`] address from the `push` instruction, and returns it.
///
/// # Arguments
///
/// * `map` - Address map for VA-to-offset translation.
/// * `stub_va` - Virtual address of the native call stub in the constant pool.
///
/// # Errors
///
/// Returns an error if the VA cannot be resolved or the stub is neither form.
pub fn resolve_api_stub<'a>(map: &AddressMap<'a>, stub_va: u32) -> Result<CallApiStub<'a>, Error> {
    /// The cache check before the `push`, without the cache's address.
    const CACHED_TAIL: [u8; 6] = [0x0B, 0xC0, 0x74, 0x02, 0xFF, 0xE0];

    let stub_data = map.slice_from_va(stub_va, 5)?;
    let push_at = match (stub_data.first(), stub_data.get(5..11)) {
        (Some(0xA1), Some(tail)) if tail == CACHED_TAIL => 11,
        _ => 0,
    };
    let first = *stub_data.get(push_at).ok_or(Error::TooShort {
        expected: push_at.saturating_add(5),
        actual: stub_data.len(),
        context: "resolve_api_stub",
    })?;
    if first != 0x68 {
        return Err(Error::EntryPointNotPush { byte: first });
    }

    let call_api_va = read_u32_le(stub_data, push_at.saturating_add(1))?;
    // Prefer the full descriptor (name VAs + ordinal + flags + cache), but fall
    // back to the 8-byte minimum when the struct sits at the end of a section
    // and the trailing fields aren't backed by file data.
    let call_api_data = map
        .slice_from_va(call_api_va, CallApiStub::FULL_SIZE)
        .or_else(|_| map.slice_from_va(call_api_va, CallApiStub::SIZE))?;
    CallApiStub::parse(call_api_data)
}

/// A VB6 type byte in the encoding of [`VbBaseType`], with modifier bits
/// (`ByRef`, `Array`, `Optional`) OR'd with the base type.
///
/// This is not the encoding of a FuncTypDesc's type list, which has its own
/// codes ([`ArgType`](crate::vb::functype::ArgType)).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VbType(
    /// Raw type byte, possibly OR'd with modifier flags (ByRef, Array, Optional).
    pub u8,
);

impl VbType {
    /// Empty/void type.
    pub const EMPTY: u8 = 0x00;
    /// Null type.
    pub const NULL: u8 = 0x01;
    /// Integer (16-bit).
    pub const INTEGER: u8 = 0x02;
    /// Long (32-bit).
    pub const LONG: u8 = 0x03;
    /// Single-precision float.
    pub const SINGLE: u8 = 0x04;
    /// Double-precision float.
    pub const DOUBLE: u8 = 0x05;
    /// Currency (64-bit fixed-point).
    pub const CURRENCY: u8 = 0x06;
    /// Date (stored as Double).
    pub const DATE: u8 = 0x07;
    /// String (BSTR).
    pub const STRING: u8 = 0x08;
    /// Object reference.
    pub const OBJECT: u8 = 0x0A;
    /// Error type.
    pub const ERROR: u8 = 0x0B;
    /// Boolean.
    pub const BOOLEAN: u8 = 0x0C;
    /// Variant.
    pub const VARIANT: u8 = 0x0D;
    /// Decimal.
    pub const DECIMAL: u8 = 0x0E;
    /// Byte (unsigned 8-bit).
    pub const BYTE: u8 = 0x10;
    /// User Defined Type (UDT). In lpArgTypes, followed by 4-byte aligned extra data.
    pub const UDT: u8 = 0x11;
    /// Typed object reference. In lpArgTypes, followed by 4-byte aligned extra data.
    pub const TYPED_OBJECT: u8 = 0x13;
    /// Typed array. In lpArgTypes, followed by 4-byte aligned extra data.
    pub const TYPED_ARRAY: u8 = 0x14;
    /// Long pointer / handle.
    pub const LONG_PTR: u8 = 0x1B;
    /// Extended decimal. In lpArgTypes, followed by 4-byte aligned extra data.
    pub const EXTENDED_DECIMAL: u8 = 0x1C;
    /// External COM object (followed by 32-bit offset).
    pub const EXTERNAL_COM: u8 = 0x1D;
    /// IDispatch pointer to an internal VB object.
    ///
    /// Not observed in the fixtures. (The runtime's type-byte decoder maps
    /// code 0x1E to `VT_HRESULT` (0x19), but it decodes the
    /// [`ArgType`](crate::vb::functype::ArgType) encoding, where 0x1E is the
    /// `this` entry, not this one.)
    pub const DISPATCH_PTR: u8 = 0x1E;

    /// Array modifier (OR'd with base type, bit 5).
    ///
    /// Unverified. The runtime's type-byte decoder (MSVBVM60 6.00.8176
    /// 0x66018ed8, 6.00.9848 0x6600fbff) maps bit 0x20 to `VT_BYREF` and bit
    /// 0x40 to `VT_ARRAY`, but it decodes the
    /// [`ArgType`](crate::vb::functype::ArgType) encoding, not this one.
    pub const ARRAY: u8 = 0x20;
    /// ByRef modifier (OR'd with base type, bit 6).
    ///
    /// Unverified: see [`ARRAY`](Self::ARRAY).
    pub const BYREF: u8 = 0x40;
    /// Optional parameter modifier (OR'd with base type, bit 7).
    pub const OPTIONAL: u8 = 0x80;

    /// Returns the raw base type code without any modifiers (5-bit).
    #[inline]
    pub fn base_type(self) -> u8 {
        self.0 & 0x1F
    }

    /// Returns the base type as a [`VbBaseType`] enum for typed matching.
    #[inline]
    pub fn base_type_enum(self) -> VbBaseType {
        VbBaseType::from_raw(self.base_type())
    }

    /// Returns `true` if the ByRef modifier is set.
    #[inline]
    pub fn is_byref(self) -> bool {
        self.0 & Self::BYREF != 0
    }

    /// Returns `true` if the Array modifier is set.
    #[inline]
    pub fn is_array(self) -> bool {
        self.0 & Self::ARRAY != 0
    }

    /// Returns `true` if the Optional modifier is set.
    #[inline]
    pub fn is_optional(self) -> bool {
        self.0 & Self::OPTIONAL != 0
    }

    /// Returns a human-readable name for the base type.
    pub fn type_name(self) -> &'static str {
        match self.base_type() {
            Self::EMPTY => "Void",
            Self::NULL => "Null",
            Self::INTEGER => "Integer",
            Self::LONG => "Long",
            Self::SINGLE => "Single",
            Self::DOUBLE => "Double",
            Self::CURRENCY => "Currency",
            Self::DATE => "Date",
            Self::STRING => "String",
            Self::OBJECT => "Object",
            Self::ERROR => "Error",
            Self::BOOLEAN => "Boolean",
            Self::VARIANT => "Variant",
            Self::DECIMAL => "Decimal",
            Self::BYTE => "Byte",
            Self::UDT => "UDT",
            Self::TYPED_OBJECT => "TypedObject",
            Self::TYPED_ARRAY => "TypedArray",
            Self::LONG_PTR => "LongPtr",
            Self::EXTENDED_DECIMAL => "ExtDecimal",
            Self::EXTERNAL_COM => "ExternalCOM",
            Self::DISPATCH_PTR => "DispatchPtr",
            _ => "Unknown",
        }
    }
}

impl fmt::Display for VbType {
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

/// Base type enumeration for VB6 type descriptors (5-bit type code).
///
/// Extracted from [`VbType`] via [`VbType::base_type_enum`]. The 3 high bits
/// of VbType carry modifiers (Array, ByRef, Optional); this enum represents
/// only the base type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VbBaseType {
    /// Void / empty (0x00).
    Void,
    /// Null (0x01).
    Null,
    /// Integer, 16-bit signed (0x02).
    Integer,
    /// Long, 32-bit signed (0x03).
    Long,
    /// Single-precision float (0x04).
    Single,
    /// Double-precision float (0x05).
    Double,
    /// Currency, 64-bit fixed-point (0x06).
    Currency,
    /// Date, stored as Double (0x07).
    Date,
    /// String / BSTR (0x08).
    String,
    /// Object reference (0x0A).
    Object,
    /// Error type (0x0B).
    Error,
    /// Boolean (0x0C).
    Boolean,
    /// Variant (0x0D).
    Variant,
    /// Decimal (0x0E).
    Decimal,
    /// Byte, unsigned 8-bit (0x10).
    Byte,
    /// User Defined Type (0x11).
    Udt,
    /// Typed object reference (0x13).
    TypedObject,
    /// Typed array (0x14).
    TypedArray,
    /// Long pointer / handle (0x1B).
    LongPtr,
    /// Extended decimal (0x1C).
    ExtDecimal,
    /// External COM object (0x1D).
    ExternalCom,
    /// IDispatch pointer to internal VB object (0x1E).
    DispatchPtr,
    /// Unknown or undocumented type code.
    Unknown(u8),
}

impl VbBaseType {
    /// Converts a raw 5-bit type code to a `VbBaseType`.
    pub fn from_raw(raw: u8) -> Self {
        match raw & 0x1F {
            0x00 => Self::Void,
            0x01 => Self::Null,
            0x02 => Self::Integer,
            0x03 => Self::Long,
            0x04 => Self::Single,
            0x05 => Self::Double,
            0x06 => Self::Currency,
            0x07 => Self::Date,
            0x08 => Self::String,
            0x0A => Self::Object,
            0x0B => Self::Error,
            0x0C => Self::Boolean,
            0x0D => Self::Variant,
            0x0E => Self::Decimal,
            0x10 => Self::Byte,
            0x11 => Self::Udt,
            0x13 => Self::TypedObject,
            0x14 => Self::TypedArray,
            0x1B => Self::LongPtr,
            0x1C => Self::ExtDecimal,
            0x1D => Self::ExternalCom,
            0x1E => Self::DispatchPtr,
            n => Self::Unknown(n),
        }
    }

    /// Returns a human-readable name for this base type.
    pub fn name(self) -> &'static str {
        match self {
            Self::Void => "Void",
            Self::Null => "Null",
            Self::Integer => "Integer",
            Self::Long => "Long",
            Self::Single => "Single",
            Self::Double => "Double",
            Self::Currency => "Currency",
            Self::Date => "Date",
            Self::String => "String",
            Self::Object => "Object",
            Self::Error => "Error",
            Self::Boolean => "Boolean",
            Self::Variant => "Variant",
            Self::Decimal => "Decimal",
            Self::Byte => "Byte",
            Self::Udt => "UDT",
            Self::TypedObject => "TypedObject",
            Self::TypedArray => "TypedArray",
            Self::LongPtr => "LongPtr",
            Self::ExtDecimal => "ExtDecimal",
            Self::ExternalCom => "ExternalCOM",
            Self::DispatchPtr => "DispatchPtr",
            Self::Unknown(_) => "Unknown",
        }
    }
}

impl fmt::Display for VbBaseType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// COM VARIANT type code.
///
/// Used in optional parameter default value entries (at the VA pointed to
/// by `FuncTypDesc.optional_defaults_va`). Mirrors the `VARENUM` values
/// from the Windows SDK.
///
/// Size mapping verified against `VarTypeToSize` (MSVBVM60 6.00.8176
/// 0x660f2747, 6.00.9848 0x660F5FF0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum VarType {
    /// VT_EMPTY (0) - no value.
    Empty = 0,
    /// VT_NULL (1) - SQL-style null.
    Null = 1,
    /// VT_I2 (2) - 16-bit signed integer. Data: 2 bytes.
    I2 = 2,
    /// VT_I4 (3) - 32-bit signed integer. Data: 4 bytes.
    I4 = 3,
    /// VT_R4 (4) - 32-bit float. Data: 4 bytes.
    R4 = 4,
    /// VT_R8 (5) - 64-bit float. Data: 8 bytes.
    R8 = 5,
    /// VT_CY (6) - Currency (64-bit fixed-point). Data: 8 bytes.
    Cy = 6,
    /// VT_DATE (7) - Date (as f64). Data: 8 bytes.
    Date = 7,
    /// VT_BSTR (8) - Unicode string. Data: u16 length + UTF-16LE bytes.
    Bstr = 8,
    /// VT_DISPATCH (9) - IDispatch pointer. Data: 4 bytes.
    Dispatch = 9,
    /// VT_ERROR (10) - SCODE. Data: 4 bytes.
    Error = 10,
    /// VT_BOOL (11) - Boolean (VARIANT_BOOL). Data: 2 bytes.
    Bool = 11,
    /// VT_VARIANT (12) - Variant (nested). Data: variable.
    Variant = 12,
    /// VT_UNKNOWN (13) - IUnknown pointer. Data: 4 bytes.
    Unknown = 13,
    /// VT_DECIMAL (14) - 96-bit decimal. Data: 16 bytes.
    Decimal = 14,
    /// VT_I1 (16) - 8-bit signed integer. Data: 2 bytes (word-aligned).
    I1 = 16,
    /// VT_UI1 (17) - 8-bit unsigned integer. Data: 2 bytes (word-aligned).
    Ui1 = 17,
    /// VT_UI2 (18) - 16-bit unsigned integer. Data: 2 bytes.
    Ui2 = 18,
    /// VT_RECORD (19) - UDT / record. Data: 4 bytes.
    Record = 19,
    /// VT_INT (22) - Machine-sized signed integer. Data: 4 bytes.
    Int = 22,
    /// VT_UINT (23) - Machine-sized unsigned integer. Data: 4 bytes.
    Uint = 23,
}

impl VarType {
    /// Converts a raw u16 to a VarType, returning None for unknown codes.
    pub fn from_raw(v: u16) -> Option<Self> {
        match v {
            0 => Some(Self::Empty),
            1 => Some(Self::Null),
            2 => Some(Self::I2),
            3 => Some(Self::I4),
            4 => Some(Self::R4),
            5 => Some(Self::R8),
            6 => Some(Self::Cy),
            7 => Some(Self::Date),
            8 => Some(Self::Bstr),
            9 => Some(Self::Dispatch),
            10 => Some(Self::Error),
            11 => Some(Self::Bool),
            12 => Some(Self::Variant),
            13 => Some(Self::Unknown),
            14 => Some(Self::Decimal),
            16 => Some(Self::I1),
            17 => Some(Self::Ui1),
            18 => Some(Self::Ui2),
            19 => Some(Self::Record),
            22 => Some(Self::Int),
            23 => Some(Self::Uint),
            _ => None,
        }
    }

    /// Returns the byte size of this type's data portion in a default value entry.
    ///
    /// Mirrors `VarTypeToSize` (MSVBVM60 6.00.8176 0x660f2747).
    /// Returns 0 for variable-size types (BSTR, Variant) and unknown types.
    pub fn data_size(self) -> usize {
        match self {
            Self::Empty | Self::Null => 0,
            Self::I2 => 2,
            Self::I4 | Self::R4 => 4,
            Self::R8 | Self::Cy | Self::Date => 8,
            Self::Bstr => 0, // Variable - handled separately
            Self::Dispatch | Self::Error => 4,
            Self::Bool => 2,
            Self::Variant => 0,
            Self::Unknown => 4,
            Self::Decimal => 16,
            Self::I1 | Self::Ui1 | Self::Ui2 => 2,
            Self::Record => 4,
            Self::Int | Self::Uint => 4,
        }
    }
}

impl fmt::Display for VarType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "Empty"),
            Self::Null => write!(f, "Null"),
            Self::I2 => write!(f, "Integer"),
            Self::I4 => write!(f, "Long"),
            Self::R4 => write!(f, "Single"),
            Self::R8 => write!(f, "Double"),
            Self::Cy => write!(f, "Currency"),
            Self::Date => write!(f, "Date"),
            Self::Bstr => write!(f, "String"),
            Self::Dispatch => write!(f, "Object"),
            Self::Error => write!(f, "Error"),
            Self::Bool => write!(f, "Boolean"),
            Self::Variant => write!(f, "Variant"),
            Self::Unknown => write!(f, "Unknown"),
            Self::Decimal => write!(f, "Decimal"),
            Self::I1 => write!(f, "SByte"),
            Self::Ui1 => write!(f, "Byte"),
            Self::Ui2 => write!(f, "UShort"),
            Self::Record => write!(f, "UDT"),
            Self::Int => write!(f, "Int"),
            Self::Uint => write!(f, "UInt"),
        }
    }
}

/// Classification of an external table entry.
///
/// The `fExternalType` field determines what kind of external reference
/// the entry represents and how `lpExternalObject` should be interpreted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalKind {
    /// GUID reference (`fExternalType == 0x06`).
    ///
    /// The `lpExternalObject` VA points to an [`ExternalTypelibInfo`] whose
    /// first dword is the VA of a 16-byte GUID. In the fixtures the two
    /// projects with a UserControl (`forms`, `dispid`) each have one such
    /// entry, with the GUID `{FCFB3D23-A0FA-1068-A738-08002B3371B5}`; no
    /// fixture references an OCX.
    TypeLib,

    /// Declare function import (`fExternalType == 0x07`).
    ///
    /// The `lpExternalObject` VA points to an [`ExternalDeclareInfo`] whose
    /// first two DWORDs are VAs to null-terminated strings: the DLL library
    /// name and the exported function name (e.g., `kernel32` +
    /// `GetTickCount`).
    DeclareFunction,

    /// Unknown or unrecognized external type.
    ///
    /// The raw `fExternalType` value is preserved for inspection.
    Unknown(u32),
}

impl ExternalKind {
    /// Classifies an external type value.
    pub fn from_raw(value: u32) -> Self {
        match value {
            0x06 => Self::TypeLib,
            0x07 => Self::DeclareFunction,
            other => Self::Unknown(other),
        }
    }

    /// Returns the stable persistence string for this external kind.
    ///
    /// These strings are part of the public API contract and are suitable
    /// for database storage: `"DeclareFunction"`, `"TypeLib"`, and
    /// `"Unknown"`.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::DeclareFunction => "DeclareFunction",
            Self::TypeLib => "TypeLib",
            Self::Unknown(_) => "Unknown",
        }
    }
}

impl fmt::Display for ExternalKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// View over an external component table entry (8 bytes).
///
/// The external table is referenced by `ProjectData.external_table_va()?`
/// with `ProjectData.external_count()?` entries. Each entry is a DLL
/// function a `Declare` imports or a GUID reference. `Declare`s of the same
/// entry point share one entry: `vtable` has 13 `Declare`s, 12 of them
/// aliases of `GetTickCount`, and 2 entries.
///
/// Use [`kind()`](Self::kind) to determine what the entry represents and
/// how to interpret `external_object_va()`.
///
/// # Layout
///
/// | Offset | Size | Field |
/// |--------|------|-------|
/// | 0x00 | 4 | `fExternalType` - see [`ExternalKind`] |
/// | 0x04 | 4 | `lpExternalObject` (VA to component descriptor) |
#[derive(Clone, Copy, Debug)]
pub struct ExternalTableEntry<'a> {
    /// Raw backing bytes borrowed from the PE file buffer.
    bytes: &'a [u8],
}

impl<'a> ExternalTableEntry<'a> {
    /// Size of one external table entry in bytes.
    pub const SIZE: usize = 8;

    /// Parses an external table entry from the given byte slice.
    ///
    /// # Errors
    ///
    /// Returns [`Error::TooShort`] if `data.len() < 8`.
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        let bytes = data.get(..Self::SIZE).ok_or(Error::TooShort {
            expected: Self::SIZE,
            actual: data.len(),
            context: "ExternalTableEntry",
        })?;
        Ok(Self { bytes })
    }

    /// Raw component type flags at offset 0x00.
    #[inline]
    pub fn external_type(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x00)
    }

    /// Classified external kind based on the type flags.
    ///
    /// Use this to determine how to interpret [`external_object_va`](Self::external_object_va):
    /// - [`ExternalKind::DeclareFunction`]: VA points to [`ExternalDeclareInfo`]
    /// - [`ExternalKind::TypeLib`]: VA points to [`ExternalTypelibInfo`]
    ///
    /// # Errors
    ///
    /// Returns [`Error::Truncated`] if the backing buffer is shorter than expected.
    #[inline]
    pub fn kind(&self) -> Result<ExternalKind, Error> {
        Ok(ExternalKind::from_raw(self.external_type()?))
    }

    /// VA of the external component descriptor at offset 0x04.
    #[inline]
    pub fn external_object_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x04)
    }

    /// Parses this entry as a `Declare` function import.
    ///
    /// Returns `None` if the type is not [`ExternalKind::DeclareFunction`]
    /// or the VA cannot be resolved.
    pub fn as_declare(&self, map: &AddressMap<'a>) -> Option<ExternalDeclareInfo<'a>> {
        if !matches!(self.kind().ok()?, ExternalKind::DeclareFunction) {
            return None;
        }
        let va = self.external_object_va().ok()?;
        let data = map.slice_from_va(va, ExternalDeclareInfo::SIZE).ok()?;
        ExternalDeclareInfo::parse(data).ok()
    }

    /// Parses this entry as a TypeLib reference.
    ///
    /// Returns `None` if the type is not [`ExternalKind::TypeLib`] (`exprs`
    /// entry 0, the `Declare` of `lstrlenA`) or the VA cannot be resolved.
    pub fn as_typelib(&self, map: &AddressMap<'a>) -> Option<ExternalTypelibInfo<'a>> {
        if !matches!(self.kind().ok()?, ExternalKind::TypeLib) {
            return None;
        }
        let va = self.external_object_va().ok()?;
        let data = map.slice_from_va(va, ExternalTypelibInfo::SIZE).ok()?;
        ExternalTypelibInfo::parse(data).ok()
    }
}

/// External Declare function descriptor (0x10 bytes).
///
/// Describes a `Declare Function`/`Declare Sub` import from a native DLL.
/// It is the `DllFunctionCall` descriptor itself: the same bytes the
/// function's native stub pushes (`types`: the `Sleep` entry's descriptor
/// is 0x00401990, and its stub at 0x004019A8 pushes 0x00401990), with the
/// [`CallApiStub`] layout; `CallApiStub::from` views it as one.
///
/// # Layout
///
/// | Offset | Size | Field |
/// |--------|------|-------|
/// | 0x00 | 4 | `lpLibraryName` (VA to DLL name string) |
/// | 0x04 | 4 | `lpFunctionName` (VA to API function name string) |
/// | 0x08 | 2 | [`ordinal`](Self::ordinal) (0 in every fixture `Declare`) |
/// | 0x0A | 2 | [`flags`](Self::flags) (0x0004 in every fixture `Declare`) |
/// | 0x0C | 4 | `lpResolveCache` (VA in the zero-filled `.data`: +0x04 module handle, +0x08 function address, written by `DllFunctionCall`) |
#[derive(Clone, Copy, Debug)]
pub struct ExternalDeclareInfo<'a> {
    bytes: &'a [u8],
}

impl<'a> From<ExternalDeclareInfo<'a>> for CallApiStub<'a> {
    /// Views a `Declare`'s descriptor as the [`CallApiStub`] it is.
    fn from(declare: ExternalDeclareInfo<'a>) -> Self {
        Self {
            bytes: declare.bytes,
        }
    }
}

impl<'a> ExternalDeclareInfo<'a> {
    /// Size of the structure in bytes.
    pub const SIZE: usize = 0x10;

    /// Bytes from the descriptor to the call stub the compiler places after
    /// it: the descriptor, then 8 bytes nothing reads (0 in every fixture).
    /// The stub (`mov eax, [cache + 8]; ...; push <descriptor>; call
    /// <DllFunctionCall>`) starts at the descriptor + 0x18 (VBA6 6.0.9782
    /// `0x0FB10155`), the address the module's constant pool names.
    pub const BLOCK_SIZE: usize = 0x18;

    /// Parses an external declare info from the given byte slice.
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        let bytes = data.get(..Self::SIZE).ok_or(Error::TooShort {
            expected: Self::SIZE,
            actual: data.len(),
            context: "ExternalDeclareInfo",
        })?;
        Ok(Self { bytes })
    }

    /// VA of the DLL library name string at offset 0x00.
    #[inline]
    pub fn library_name_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x00)
    }

    /// VA of the API function name string at offset 0x04.
    #[inline]
    pub fn function_name_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x04)
    }

    /// Import ordinal at offset 0x08 (see [`CallApiStub::ordinal`]).
    ///
    /// Meaningful when [`is_by_ordinal`](Self::is_by_ordinal) is `true`; 0
    /// for every `Declare` of the fixtures, all by name.
    #[inline]
    pub fn ordinal(&self) -> Result<u16, Error> {
        read_u16_le(self.bytes, 0x08)
    }

    /// Resolution flags at offset 0x0A (see [`CallApiStub::flags`]).
    ///
    /// 0x0004 for every `Declare` of the fixtures.
    #[inline]
    pub fn flags(&self) -> Result<u16, Error> {
        read_u16_le(self.bytes, 0x0A)
    }

    /// Returns `true` if the function resolves by ordinal
    /// ([`CallApiStub::FLAG_BY_ORDINAL`] set in [`flags`](Self::flags)),
    /// the API name then being absent from the binary.
    #[inline]
    pub fn is_by_ordinal(&self) -> bool {
        CallApiStub::from(*self).is_by_ordinal()
    }

    /// VA of the resolve cache at offset 0x0C.
    ///
    /// Not a stub: it points into the zero-filled `.data` section, where
    /// `DllFunctionCall` stores the module handle (+0x04, MSVBVM60 6.00.8176
    /// `0x66026791`) and the function address (+0x08, `0x660267C6`) on the
    /// first call. The function's stub jumps through cache + 8 once it is
    /// set (`types`: `Sleep`'s cache 0x0040432C, its stub reads
    /// `[0x00404334]`).
    #[inline]
    pub fn resolve_cache_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x0C)
    }

    /// Resolves the DLL library name string.
    pub fn library_name(&self, map: &AddressMap<'a>) -> Option<&'a str> {
        let va = self.library_name_va().ok()?;
        if va == 0 {
            return None;
        }
        let off = map.va_to_offset(va).ok()?;
        let name = read_cstr(map.file(), off).ok()?;
        str::from_utf8(name).ok()
    }

    /// Resolves the API function name string.
    pub fn function_name(&self, map: &AddressMap<'a>) -> Option<&'a str> {
        let va = self.function_name_va().ok()?;
        if va == 0 {
            return None;
        }
        let off = map.va_to_offset(va).ok()?;
        let name = read_cstr(map.file(), off).ok()?;
        str::from_utf8(name).ok()
    }
}

/// External TypeLib reference descriptor.
///
/// The target of an [`ExternalKind::TypeLib`] entry. The GUID is accessed
/// indirectly through a VA pointer; in the fixtures it is
/// `{FCFB3D23-A0FA-1068-A738-08002B3371B5}`, not an OCX's type library.
///
/// # Layout
///
/// | Offset | Size | Field |
/// |--------|------|-------|
/// | 0x00 | 4 | `lpTypelibGuid` (VA to 16-byte GUID) |
/// | 0x04 | 4 | `lpRuntimeData` (VA in the zero-filled `.data` section) |
#[derive(Clone, Copy, Debug)]
pub struct ExternalTypelibInfo<'a> {
    bytes: &'a [u8],
}

impl<'a> ExternalTypelibInfo<'a> {
    /// Minimum size of the structure in bytes.
    pub const SIZE: usize = 0x08;

    /// Parses an external typelib info from the given byte slice.
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        let bytes = data.get(..Self::SIZE).ok_or(Error::TooShort {
            expected: Self::SIZE,
            actual: data.len(),
            context: "ExternalTypelibInfo",
        })?;
        Ok(Self { bytes })
    }

    /// VA to the 16-byte typelib GUID at offset 0x00.
    #[inline]
    pub fn typelib_guid_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x00)
    }

    /// Resolves the typelib GUID by following the VA pointer.
    pub fn typelib_guid(&self, map: &AddressMap<'a>) -> Option<Guid> {
        let va = self.typelib_guid_va().ok()?;
        if va == 0 {
            return None;
        }
        let data = map.slice_from_va(va, 16).ok()?;
        Guid::from_bytes(data)
    }
}

/// View over a variable-length external component entry: one ActiveX
/// control class the project hosts on a form or UserControl.
///
/// Used by `VBHeader.external_table_va` (+0x50). Each entry uses
/// self-relative offsets like ComRegData. The runtime (MSVBVM60 6.00.8176
/// 0x6602db62, 6.00.9848 0x6603C89A) copies `dwEntrySize` bytes and turns
/// the offsets at +0x04..+0x30 into pointers.
///
/// Measured on `tests/fixtures/{activex,forms,dispid,ocx}`: an entry per
/// hosted class, a third-party control (`activex`: `MSWINSCK.OCX` Winsock,
/// `MSINET.OCX` Inet, `COMDLG32.OCX` CommonDialog) or one of the project's
/// own UserControls (`forms` Gauge, `dispid` Dial, `ocx` Knob, with an
/// empty OCX filename).
///
/// # Header Layout (0x34 bytes, 13 dwords)
///
/// | Offset | Size | Field |
/// |--------|------|-------|
/// | 0x00 | 4 | `dwEntrySize` - total entry size (self-relative advance to next) |
/// | 0x04 | 4 | `bComponentInfo` - self-rel offset to the component info block (0x38) |
/// | 0x08 | 4 | self-rel offset to the coordinate conversions of event parameters ([`event_conversions`](Self::event_conversions)), 0 when there are none |
/// | 0x0C | 4 | self-rel offset to one `u16` per event indexing those conversions, 0 when there are none |
/// | 0x10 | 4 | self-rel offset to a bitmap of one bit per event (`cFuncs / 8 + 1` bytes, which VB6.EXE allocates zeroed at 0x48ed5b), 0 when the control declares no event; every bit is 0 in every fixture, and zeroing it changes neither hosting nor event dispatch (a control with its DISPIDs corrupted stops calling its handlers); the bytes after it are allocation slack |
/// | 0x14 | 4 | self-rel offset to the DISPIDs of the control's events ([`event_dispids`](Self::event_dispids)), 0 when it declares none |
/// | 0x18 | 4 | self-rel offset to the bindable-property records ([`bindable_properties`](Self::bindable_properties)), 0 when it has none |
/// | 0x1C | 4 | `bLicenseKey` - self-rel offset to the control's licence key, UTF-16 ([`license_key`](Self::license_key)) |
/// | 0x20 | 4 | the licence key's length in bytes (0x48: 36 UTF-16 characters), -1 when there is none; the runtime makes the key a BSTR with `SysAllocStringByteLen` (MSVBVM60 6.00.8176, 0x6602dd71) |
/// | 0x24 | 4 | self-rel offset (0 in every fixture) |
/// | 0x28 | 4 | `bOcxFilename` - self-rel offset to the OCX filename (empty for the project's own UserControl) |
/// | 0x2C | 4 | `bProgId` - self-rel offset to the ProgID (e.g., `"MSWinsockLib.Winsock"`) |
/// | 0x30 | 4 | `bClassName` - self-rel offset to the class name (e.g., `"Winsock"`) |
///
/// # Component Info Block (at `bComponentInfo`)
///
/// | Offset | Size | Field |
/// |--------|------|-------|
/// | 0x00 | 16 | the control's CLSID ([`clsid`](Self::clsid)) |
/// | 0x10 | 16 | its events (source) interface IID ([`events_iid`](Self::events_iid)) |
/// | 0x20 | 16 | its default interface IID ([`default_iid`](Self::default_iid)) |
/// | 0x30 | 32 | two GUIDs the compiler generates per build |
/// | 0x60 | 16 | the events IID a hosted instance's ControlInfo names ([`instance_events_iid`](Self::instance_events_iid)) |
/// | 0x70 | 16 | the same for an instance in a control array ([`array_events_iid`](Self::array_events_iid)) |
/// | 0x84 | 2 | the control's extender capabilities, derived from its `MiscStatus` ([`extender_flags`](Self::extender_flags)) |
/// | 0x86 | 2 | more control flags ([`control_flags`](Self::control_flags)) |
/// | 0x8E | 2 | number of events the control declares ([`declared_event_count`](Self::declared_event_count)) |
/// | 0x92 | 2 | number of bindable-property records ([`bindable_count`](Self::bindable_count)) |
///
/// The instance events IIDs are generated per build: a hosted control's
/// event sink (its ControlInfo `+0x08`) is not its own events interface but
/// an extended one, whose first 9 slots are `VBControlExtenderEvents`
/// (`GotFocus` ... `Validate`) and whose slots from 9 on are the control's
/// own events in its events interface's order (`activex` Winsock: `Error` 9,
/// `DataArrival` 10, `Connect` 11, `ConnectionRequest` 12, `Close` 13; `ocx`
/// Knob: `Turned` 9, `Reset` 10, its declaration order), slot `9 + k`
/// being the event whose DISPID is [`event_dispids`](Self::event_dispids)`[k]`.
/// The entry holds no event names: those are in the control's type library.
///
/// # Bindable Property Records (at +0x18)
///
/// The runtime reads [`bindable_count`](Self::bindable_count) records of 0x18
/// bytes and points each one's `+0x10` at a name, the names following the
/// records one after another (MSVBVM60 6.00.8176, 0x6602dc2c); see
/// [`BindableProperty`]. A control has them when its type library marks
/// properties bindable (`activex` MaskEdBox: 5).
#[derive(Clone, Copy, Debug)]
pub struct ExternalComponentEntry<'a> {
    bytes: &'a [u8],
}

impl<'a> ExternalComponentEntry<'a> {
    /// Minimum header size in bytes.
    pub const HEADER_SIZE: usize = 0x34;

    /// Size of a [`BindableProperty`] record.
    pub const BINDABLE_RECORD_SIZE: usize = 0x18;

    /// Parses an external component entry from the given byte slice.
    ///
    /// The slice should start at the entry's `dwEntrySize` field.
    /// Only the header is validated; data blocks are accessed lazily.
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        if data.len() < Self::HEADER_SIZE {
            return Err(Error::TooShort {
                expected: Self::HEADER_SIZE,
                actual: data.len(),
                context: "ExternalComponentEntry",
            });
        }
        let size = read_u32_le(data, 0x00)? as usize;
        let bytes = data.get(..size).ok_or(Error::TooShort {
            expected: size,
            actual: data.len(),
            context: "ExternalComponentEntry (entry_size)",
        })?;
        if size < Self::HEADER_SIZE {
            return Err(Error::TooShort {
                expected: Self::HEADER_SIZE,
                actual: size,
                context: "ExternalComponentEntry (entry_size)",
            });
        }
        Ok(Self { bytes })
    }

    /// Total entry size at offset 0x00 (advance to next entry).
    #[inline]
    pub fn entry_size(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x00)
    }

    /// OCX/DLL filename as a lossy UTF-8 string (e.g., `"Tabctl32.ocx"`).
    ///
    /// Use [`ocx_filename_bytes`](Self::ocx_filename_bytes) for raw bytes.
    pub fn ocx_filename(&self) -> Cow<'a, str> {
        String::from_utf8_lossy(self.ocx_filename_bytes())
    }

    /// OCX/DLL filename as raw bytes from self-relative offset at +0x28.
    pub fn ocx_filename_bytes(&self) -> &'a [u8] {
        self.resolve_string(0x28)
    }

    /// ProgID-style name as a lossy UTF-8 string (e.g., `"TabDlg.SSTab"`).
    ///
    /// Use [`prog_id_bytes`](Self::prog_id_bytes) for raw bytes.
    pub fn prog_id(&self) -> Cow<'a, str> {
        String::from_utf8_lossy(self.prog_id_bytes())
    }

    /// ProgID-style name as raw bytes from self-relative offset at +0x2C.
    pub fn prog_id_bytes(&self) -> &'a [u8] {
        self.resolve_string(0x2C)
    }

    /// Short class name as a lossy UTF-8 string (e.g., `"SSTab"`).
    ///
    /// Use [`class_name_bytes`](Self::class_name_bytes) for raw bytes.
    pub fn class_name(&self) -> Cow<'a, str> {
        String::from_utf8_lossy(self.class_name_bytes())
    }

    /// Short class name as raw bytes from self-relative offset at +0x30.
    pub fn class_name_bytes(&self) -> &'a [u8] {
        self.resolve_string(0x30)
    }

    /// Returns the 16-byte GUID at `offset` in the component info block.
    fn info_guid(&self, offset: usize) -> Option<Guid> {
        let info = usize::try_from(read_u32_le(self.bytes, 0x04).ok()?).ok()?;
        if info == 0 {
            return None;
        }
        let start = info.checked_add(offset)?;
        Guid::from_bytes(self.bytes.get(start..)?)
    }

    /// Returns the control's CLSID (component info +0x00).
    pub fn clsid(&self) -> Option<Guid> {
        self.info_guid(0x00)
    }

    /// Returns the IID of the control's events (source) interface
    /// (component info +0x10): for a project's own UserControl, the
    /// object's events IID.
    pub fn events_iid(&self) -> Option<Guid> {
        self.info_guid(0x10)
    }

    /// Returns the IID of the control's default interface (component info
    /// +0x20).
    pub fn default_iid(&self) -> Option<Guid> {
        self.info_guid(0x20)
    }

    /// Returns the events IID a hosted instance of the control names in its
    /// ControlInfo (component info +0x60): the extended events interface
    /// whose slots 0-8 are `VBControlExtenderEvents` and whose slots from 9
    /// on are the control's own events.
    pub fn instance_events_iid(&self) -> Option<Guid> {
        self.info_guid(0x60)
    }

    /// Returns the events IID an instance in a control array names in its
    /// ControlInfo (component info +0x70; `activex` `Peers`, `ocx` `Knob2`).
    pub fn array_events_iid(&self) -> Option<Guid> {
        self.info_guid(0x70)
    }

    /// Returns the number of events the control declares (the u16 at
    /// component info +0x8E): `activex` Winsock 7, Inet 1, CommonDialog 0;
    /// `forms` Gauge 1, `ocx` Knob 2.
    pub fn declared_event_count(&self) -> u16 {
        self.info_u16(0x8E).unwrap_or(0)
    }

    /// Returns the licence key's length in bytes, the value at +0x20
    /// (`activex` Winsock: 0x48, 36 UTF-16 characters); `None` when it is
    /// -1 (a project's own UserControl).
    pub fn license_key_length(&self) -> Option<u32> {
        read_u32_le(self.bytes, 0x20)
            .ok()
            .filter(|&length| length != u32::MAX)
    }

    /// Returns the control's design-time licence key: UTF-16 with no
    /// terminator, [`license_key_length`](Self::license_key_length) bytes at
    /// the self-relative offset at +0x1C. The key of `HKCR\Licenses` the
    /// control's class factory checks (`activex` Winsock:
    /// `"2c49f800-c2dd-11cf-9ad6-0080c7e7b78d"`). `None` when there is none
    /// (a project's own UserControl).
    pub fn license_key(&self) -> Option<String> {
        let offset = self.offset_at(0x1C)?;
        let length = usize::try_from(self.license_key_length()?).ok()?;
        let end = offset.checked_add(length)?;
        let tail = self.bytes.get(offset..end)?;
        let units: Vec<u16> = tail
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&pair| u16::from_le_bytes(pair))
            .take_while(|&unit| unit != 0)
            .collect();
        (!units.is_empty()).then(|| String::from_utf16_lossy(&units))
    }

    /// Component info block byte at component_info+0x86 (the low byte of
    /// [`control_flags`](Self::control_flags)).
    pub fn component_flags(&self) -> Option<u8> {
        let off = read_u32_le(self.bytes, 0x04).ok()? as usize;
        let end = off.checked_add(0x87)?;
        if off == 0 || end > self.bytes.len() {
            return None;
        }
        let flags_off = off.checked_add(0x86)?;
        self.bytes.get(flags_off).copied()
    }

    /// Returns the u16 at `offset` in the component info block.
    fn info_u16(&self, offset: usize) -> Option<u16> {
        let info = usize::try_from(read_u32_le(self.bytes, 0x04).ok()?).ok()?;
        if info == 0 {
            return None;
        }
        read_u16_le(self.bytes, info.checked_add(offset)?).ok()
    }

    /// Returns the self-relative offset at header `field`, `None` when 0.
    fn offset_at(&self, field: usize) -> Option<usize> {
        let offset = usize::try_from(read_u32_le(self.bytes, field).ok()?).ok()?;
        (offset != 0).then_some(offset)
    }

    /// Returns the DISPIDs of the control's own events, in its events
    /// interface's order (the `i32` array at the self-relative offset at
    /// +0x14, [`declared_event_count`](Self::declared_event_count) entries).
    ///
    /// A hosted instance's event sink slot `9 + k` is the event with
    /// DISPID `event_dispids()[k]` (`activex` Winsock: 6, 0, 1, 2, 5, 3, 4,
    /// its `Error`, `DataArrival`, `Connect`, `ConnectionRequest`, `Close`,
    /// `SendProgress`, `SendComplete`; `ocx` Knob: 1, 2). Empty when the
    /// control declares no event.
    pub fn event_dispids(&self) -> Vec<i32> {
        let Some(start) = self.offset_at(0x14) else {
            return Vec::new();
        };
        (0..usize::from(self.declared_event_count()))
            .map_while(|k| {
                let offset = k.checked_mul(4)?.checked_add(start)?;
                read_i32_le(self.bytes, offset).ok()
            })
            .collect()
    }

    /// Returns, per declared event (in [`event_dispids`](Self::event_dispids)
    /// order), the parameters the runtime converts before calling the
    /// host's handler: the `u16` at index `k` of the array at +0x0C is a
    /// dword index into the list at +0x08, whose entries run to a 0 kind
    /// (MSVBVM60 6.00.8176, 0x66024f4e; VB6.EXE builds it at 0x48ec9e).
    /// `activex` ProgressBar: its first three events (MouseDown, MouseMove,
    /// MouseUp) convert parameter 2 (`x`, `OLE_XPOS_PIXELS`) and 3 (`y`,
    /// `OLE_YPOS_PIXELS`). Empty for a control without such parameters.
    /// See [`ConversionKind`].
    pub fn event_conversions(&self) -> Vec<Vec<ParamConversion>> {
        let (Some(list), Some(indices)) = (self.offset_at(0x08), self.offset_at(0x0C)) else {
            return Vec::new();
        };
        (0..usize::from(self.declared_event_count()))
            .map(|k| {
                let Some(first) = k
                    .checked_mul(2)
                    .and_then(|offset| offset.checked_add(indices))
                    .and_then(|offset| read_u16_le(self.bytes, offset).ok())
                    .filter(|&first| first != 0)
                else {
                    return Vec::new();
                };
                (usize::from(first)..)
                    .map_while(|entry| {
                        let at = entry.checked_mul(4)?.checked_add(list)?;
                        let raw = read_u32_le(self.bytes, at).ok()?;
                        let [low, high, by_ref, kind] = raw.to_le_bytes();
                        (kind != 0).then_some(ParamConversion {
                            param: u16::from_le_bytes([low, high]),
                            by_ref: by_ref != 0,
                            kind: ConversionKind::from_raw(kind),
                        })
                    })
                    .collect()
            })
            .collect()
    }

    /// Returns the control's extender capability flags, the `u16` at
    /// component info +0x84, which VB6 derives from the control's
    /// `MiscStatus` and from which the runtime decides the extender
    /// properties it offers (MSVBVM60 6.00.8176, 0x660b5f41).
    ///
    /// Measured by changing one UserControl property at a time
    /// (`tests/fixtures/extender`, base 0x1766 for `MiscStatus` 0x20191):
    /// `OLEMISC_INVISIBLEATRUNTIME` gives 0x0B60 (0x0800 set; 0x0002,
    /// 0x0004, 0x0400 and 0x1000 cleared), `CanGetFocus = False` clears
    /// 0x0404, `OLEMISC_ALIGNABLE` sets 0x0080, `OLEMISC_ACTSLIKEBUTTON` sets
    /// 0x2000, `OLEMISC_ACTSLIKELABEL` sets 0x4000 and clears 0x0400,
    /// `OLEMISC_SIMPLEFRAME` sets 0x8000; a control with bindable properties
    /// sets 0x0010 (`activex` MaskEdBox).
    pub fn extender_flags(&self) -> u16 {
        self.info_u16(0x84).unwrap_or(0)
    }

    /// Returns the control flags, the `u16` at component info +0x86: 0x2057
    /// for most controls (the project's UserControls, Winsock, Inet,
    /// CommonDialog, MSComm, SysInfo, MMControl), 0x3057 for MaskEdBox,
    /// RichTextBox, SSTab and the Common Controls, 0x2055 for ImageList
    /// (`tests/fixtures/{activex,extender}` and 16 more classes measured).
    ///
    /// Bits whose cause is measured in VB6.EXE: 0x0100, a property typed
    /// `OLE_OPTEXCLUSIVE` (0x4da547); 0x0800, a `DataSource` property (typed
    /// `DataSource` or `ICursor`, 0x48f9c7). Bits whose effect is measured
    /// in the runtime (MSVBVM60 6.00.8176): 0x0080 registers the class
    /// through a separate path (0x6602b84c) and keeps two extender members
    /// (0x660b5f8e); 0x1000 keeps one more extender member (0x660b5fd6).
    /// What sets 0x0080 and 0x1000 is not measured: 0x0080 is clear in
    /// every class measured, 0x1000 does not follow the type library.
    pub fn control_flags(&self) -> u16 {
        self.info_u16(0x86).unwrap_or(0)
    }

    /// Returns the number of [`bindable_properties`](Self::bindable_properties),
    /// the u16 at component info +0x92.
    pub fn bindable_count(&self) -> u16 {
        self.info_u16(0x92).unwrap_or(0)
    }

    /// Returns the control's data-bindable properties: the 0x18-byte
    /// records at the self-relative offset at +0x18, their names following
    /// them one after another (`activex` MaskEdBox: `Text`, `BackColor`,
    /// `ForeColor`, `Enabled`, `BorderStyle`; a control with none has no
    /// records).
    pub fn bindable_properties(&self) -> Vec<BindableProperty<'a>> {
        let Some(start) = self.offset_at(0x18) else {
            return Vec::new();
        };
        let count = usize::from(self.bindable_count());
        let Some(mut name_at) = count
            .checked_mul(Self::BINDABLE_RECORD_SIZE)
            .and_then(|size| size.checked_add(start))
        else {
            return Vec::new();
        };
        let mut properties = Vec::with_capacity(count);
        for k in 0..count {
            let Some(record) = k
                .checked_mul(Self::BINDABLE_RECORD_SIZE)
                .and_then(|offset| offset.checked_add(start))
            else {
                break;
            };
            let (Ok(dispid), Ok(flags), Ok(vartype), Ok(name)) = (
                read_i32_le(self.bytes, record),
                read_u32_le(self.bytes, record.saturating_add(0x04)),
                read_u16_le(self.bytes, record.saturating_add(0x0C)),
                read_cstr(self.bytes, name_at),
            ) else {
                break;
            };
            properties.push(BindableProperty {
                dispid,
                flags,
                vartype,
                name: str::from_utf8(name).unwrap_or("?"),
            });
            name_at = name_at.saturating_add(name.len()).saturating_add(1);
        }
        properties
    }

    /// Resolves a self-relative offset to a null-terminated string.
    fn resolve_string(&self, header_offset: usize) -> &'a [u8] {
        let Ok(off_raw) = read_u32_le(self.bytes, header_offset) else {
            return &[];
        };
        let off = off_raw as usize;
        if off == 0 || off >= self.bytes.len() {
            return &[];
        }
        read_cstr(self.bytes, off).unwrap_or(&[])
    }
}

impl fmt::Display for ExternalComponentEntry<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let filename = self.ocx_filename();
        let class = self.class_name();
        write!(f, "{filename}!{class}")?;
        let events = self.declared_event_count();
        if events > 0 {
            write!(f, " ({events} events)")?;
        }
        Ok(())
    }
}

/// A parameter of a hosted control's event that the runtime converts before
/// calling the host's handler ([`ExternalComponentEntry::event_conversions`]):
/// a dword of the list at the component's +0x08 (`u16` parameter index, a
/// by-reference byte, a [`ConversionKind`] byte).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParamConversion {
    /// The parameter's index, 0 for the first.
    pub param: u16,
    /// `true` if the parameter is passed by reference.
    pub by_ref: bool,
    /// What the runtime does with it.
    pub kind: ConversionKind,
}

/// The conversion of a [`ParamConversion`], the compiler's kind byte
/// (VB6.EXE's table at 0x59eb64; `tests/fixtures/extender` `Coords`, one
/// event per type).
///
/// A by-value `Variant` parameter (kind 1) reaches the handler as the
/// VARIANT itself. A by-value stdole coordinate (kinds 2-9) is converted to
/// a Single in the container's scale, and the host's handler declares it
/// `Single`; `OLE_*_CONTAINER` types and by-reference parameters are not
/// converted and keep their declared types.
///
/// The runtime (MSVBVM60 6.00.8176 0x66024f83, 6.00.9848 alike) tests kind
/// 1 first, then calls entry `kind` of its converter table, whose
/// converters fill entries 1-8 and leave 9 null: each kind runs the
/// converter one place before its own. Measured with a host on a 15 twips
/// per pixel display: 100 HIMETRIC becomes 56.69 for kinds 2-4 but 1500 for
/// `OLE_YSIZE_HIMETRIC` (a pixel converter), 100 pixels becomes 1500 for
/// kinds 6-8, and an event with a by-value `OLE_YSIZE_PIXELS` parameter
/// crashes the host. No converter offsets a position by the container's
/// ScaleLeft / ScaleTop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversionKind {
    /// 1: a by-value `Variant`, passed as the VARIANT.
    Variant,
    /// 2: `OLE_XPOS_HIMETRIC`.
    XPosHimetric,
    /// 3: `OLE_YPOS_HIMETRIC`.
    YPosHimetric,
    /// 4: `OLE_XSIZE_HIMETRIC`.
    XSizeHimetric,
    /// 5: `OLE_YSIZE_HIMETRIC`.
    YSizeHimetric,
    /// 6: `OLE_XPOS_PIXELS`.
    XPosPixels,
    /// 7: `OLE_YPOS_PIXELS`.
    YPosPixels,
    /// 8: `OLE_XSIZE_PIXELS`.
    XSizePixels,
    /// 9: `OLE_YSIZE_PIXELS`.
    YSizePixels,
    /// Any other byte.
    Other(u8),
}

impl ConversionKind {
    /// Decodes the kind byte.
    pub fn from_raw(raw: u8) -> Self {
        match raw {
            1 => Self::Variant,
            2 => Self::XPosHimetric,
            3 => Self::YPosHimetric,
            4 => Self::XSizeHimetric,
            5 => Self::YSizeHimetric,
            6 => Self::XPosPixels,
            7 => Self::YPosPixels,
            8 => Self::XSizePixels,
            9 => Self::YSizePixels,
            other => Self::Other(other),
        }
    }

    /// Returns the stdole type the kind stands for (`"OLE_XPOS_PIXELS"`),
    /// `None` for [`Variant`](Self::Variant) and [`Other`](Self::Other).
    pub fn stdole_type(self) -> Option<&'static str> {
        match self {
            Self::XPosHimetric => Some("OLE_XPOS_HIMETRIC"),
            Self::YPosHimetric => Some("OLE_YPOS_HIMETRIC"),
            Self::XSizeHimetric => Some("OLE_XSIZE_HIMETRIC"),
            Self::YSizeHimetric => Some("OLE_YSIZE_HIMETRIC"),
            Self::XPosPixels => Some("OLE_XPOS_PIXELS"),
            Self::YPosPixels => Some("OLE_YPOS_PIXELS"),
            Self::XSizePixels => Some("OLE_XSIZE_PIXELS"),
            Self::YSizePixels => Some("OLE_YSIZE_PIXELS"),
            Self::Variant | Self::Other(_) => None,
        }
    }
}

/// A data-bindable property of a hosted control
/// ([`ExternalComponentEntry::bindable_properties`]): a 0x18-byte record and
/// its name.
///
/// ```text
/// +0x00  i32  dispid   the property's DISPID (MaskEdBox Text 22, BackColor -501)
/// +0x04  u32  flags    6 for the default-bound property, 0 for the others
/// +0x0C  u16  vartype  its VARTYPE (8 BSTR, 0x0B Boolean, 0x1D OLE_COLOR)
/// +0x10  u32  name     filled in by the runtime (MSVBVM60 6.00.8176, 0x6602dc47)
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BindableProperty<'a> {
    /// The property's DISPID.
    pub dispid: i32,
    /// The record's flags: 6 for the property bound by default.
    pub flags: u32,
    /// The property's VARTYPE.
    pub vartype: u16,
    /// The property's name.
    pub name: &'a str,
}

/// Iterator over variable-length external component entries.
///
/// Walks the external component table at `VBHeader.external_table_va`
/// with `VBHeader.external_count` entries, advancing by each entry's
/// self-relative size.
#[must_use = "iterators are lazy and do nothing unless consumed"]
pub struct ExternalComponentIter<'a> {
    data: &'a [u8],
    pos: usize,
    remaining: u16,
}

impl<'a> ExternalComponentIter<'a> {
    /// Creates a new iterator over external component entries.
    ///
    /// `data` should be a slice starting at the first entry. `count`
    /// is the number of entries to iterate.
    pub fn new(data: &'a [u8], count: u16) -> Self {
        Self {
            data,
            pos: 0,
            remaining: count,
        }
    }
}

impl<'a> Iterator for ExternalComponentIter<'a> {
    type Item = ExternalComponentEntry<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 || self.pos >= self.data.len() {
            return None;
        }
        self.remaining = self.remaining.saturating_sub(1);
        let rest = self.data.get(self.pos..)?;
        let entry = ExternalComponentEntry::parse(rest).ok()?;
        let size = entry.entry_size().ok()? as usize;
        if size == 0 {
            return None;
        }
        self.pos = self.pos.checked_add(size)?;
        Some(entry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::addressmap::SectionEntry;

    #[test]
    fn test_call_api_stub_parse() {
        let mut data = vec![0u8; CallApiStub::SIZE];
        data[0x00..0x04].copy_from_slice(&0x00401000u32.to_le_bytes());
        data[0x04..0x08].copy_from_slice(&0x00402000u32.to_le_bytes());
        let stub = CallApiStub::parse(&data).unwrap();
        assert_eq!(stub.library_name_va().unwrap(), 0x00401000);
        assert_eq!(stub.function_name_va().unwrap(), 0x00402000);
    }

    #[test]
    fn test_call_api_stub_too_short() {
        let data = vec![0u8; CallApiStub::SIZE - 1];
        assert!(matches!(
            CallApiStub::parse(&data),
            Err(Error::TooShort { .. })
        ));
    }

    #[test]
    fn test_call_api_stub_zero_va() {
        let data = vec![0u8; CallApiStub::SIZE];
        let stub = CallApiStub::parse(&data).unwrap();
        assert_eq!(stub.library_name_va().unwrap(), 0);
        assert_eq!(stub.function_name_va().unwrap(), 0);
    }

    #[test]
    fn test_call_api_stub_minimum_has_no_ordinal() {
        // An 8-byte (minimum) descriptor still resolves names, but the ordinal
        // and flag fields are absent and must degrade gracefully.
        let data = vec![0u8; CallApiStub::SIZE];
        let stub = CallApiStub::parse(&data).unwrap();
        assert!(stub.ordinal().is_err());
        assert!(stub.flags().is_err());
        assert!(!stub.is_by_ordinal());
    }

    #[test]
    fn test_call_api_stub_by_ordinal() {
        let mut data = vec![0u8; CallApiStub::FULL_SIZE];
        data[0x00..0x04].copy_from_slice(&0x00401000u32.to_le_bytes()); // lib name VA
        data[0x04..0x08].copy_from_slice(&0u32.to_le_bytes()); // no func name (by ordinal)
        data[0x08..0x0A].copy_from_slice(&0x007Bu16.to_le_bytes()); // ordinal 123
        data[0x0A..0x0C].copy_from_slice(&CallApiStub::FLAG_BY_ORDINAL.to_le_bytes());
        let stub = CallApiStub::parse(&data).unwrap();
        assert_eq!(stub.ordinal().unwrap(), 123);
        assert_eq!(stub.flags().unwrap(), CallApiStub::FLAG_BY_ORDINAL);
        assert!(stub.is_by_ordinal());
    }

    #[test]
    fn test_call_api_stub_by_name() {
        let mut data = vec![0u8; CallApiStub::FULL_SIZE];
        data[0x04..0x08].copy_from_slice(&0x00402000u32.to_le_bytes()); // func name VA
        // flags = 0 → by name
        let stub = CallApiStub::parse(&data).unwrap();
        assert_eq!(stub.ordinal().unwrap(), 0);
        assert_eq!(stub.flags().unwrap(), 0);
        assert!(!stub.is_by_ordinal());
    }

    #[test]
    fn test_resolve_api_stub_valid() {
        // Build a fake file with:
        // - At offset 0x200 (RVA 0x1000): push 0x00401100; jmp ...
        // - At offset 0x300 (RVA 0x1100): CallApiStub with lib_va and func_va
        // - At offset 0x400 (RVA 0x1200): "kernel32.dll\0"
        // - At offset 0x410 (RVA 0x1210): "GetTickCount\0"
        let mut file = vec![0u8; 0x500];

        // The push stub at RVA 0x1000 (offset 0x200)
        file[0x200] = 0x68; // push imm32
        file[0x201..0x205].copy_from_slice(&0x00401100u32.to_le_bytes()); // CallApiStub VA

        // CallApiStub at RVA 0x1100 (offset 0x300)
        file[0x300..0x304].copy_from_slice(&0x00401200u32.to_le_bytes()); // lib name VA
        file[0x304..0x308].copy_from_slice(&0x00401210u32.to_le_bytes()); // func name VA

        // Strings
        file[0x400..0x40C].copy_from_slice(b"kernel32.dll");
        file[0x410..0x41C].copy_from_slice(b"GetTickCount");

        let map = AddressMap::from_parts(
            &file,
            0x00400000,
            vec![SectionEntry {
                virtual_address: 0x1000,
                virtual_size: 0x1000,
                raw_data_offset: 0x200,
                raw_data_size: 0x1000,
            }],
        );

        let stub = resolve_api_stub(&map, 0x00401000).unwrap();
        assert_eq!(stub.library_name_bytes(&map).unwrap(), b"kernel32.dll");
        assert_eq!(stub.function_name_bytes(&map).unwrap(), b"GetTickCount");
        assert_eq!(stub.library_name(&map).unwrap(), "kernel32.dll");
        assert_eq!(stub.function_name(&map).unwrap(), "GetTickCount");
    }

    #[test]
    fn test_resolve_api_stub_not_push() {
        let mut file = vec![0u8; 0x500];
        file[0x200] = 0xCC; // int3 instead of push

        let map = AddressMap::from_parts(
            &file,
            0x00400000,
            vec![SectionEntry {
                virtual_address: 0x1000,
                virtual_size: 0x1000,
                raw_data_offset: 0x200,
                raw_data_size: 0x1000,
            }],
        );

        assert!(matches!(
            resolve_api_stub(&map, 0x00401000),
            Err(Error::EntryPointNotPush { byte: 0xCC })
        ));
    }

    #[test]
    fn test_vb_type_base() {
        let t = VbType(0x03);
        assert_eq!(t.base_type(), VbType::LONG);
        assert_eq!(t.type_name(), "Long");
        assert!(!t.is_byref());
        assert!(!t.is_array());
        assert!(!t.is_optional());
    }

    #[test]
    fn test_vb_type_byref_long() {
        // ByRef Long: BYREF(0x40) | LONG(0x03) = 0x43
        let t = VbType(0x43);
        assert_eq!(t.base_type(), VbType::LONG);
        assert_eq!(t.type_name(), "Long");
        assert!(t.is_byref());
        assert!(!t.is_array());
    }

    #[test]
    fn test_vb_type_optional_array_string() {
        // Optional Array String: OPTIONAL(0x80) | ARRAY(0x20) | STRING(0x08) = 0xA8
        let t = VbType(0xA8);
        assert_eq!(t.base_type(), VbType::STRING);
        assert!(t.is_optional());
        assert!(t.is_array());
        assert!(!t.is_byref());
    }

    #[test]
    fn test_vb_type_byref_array_byte() {
        // ByRef Array of Byte: BYREF(0x40) | ARRAY(0x20) | BYTE(0x10) = 0x70
        // Verified from pe_x86_vb_loader Cls_Zip Pack arg[2] = 0x70
        let t = VbType(0x70);
        assert_eq!(t.base_type(), VbType::BYTE);
        assert!(t.is_byref());
        assert!(t.is_array());
        assert!(!t.is_optional());
        assert_eq!(format!("{t}"), "ByRef Byte()");
    }

    #[test]
    fn test_vb_type_array_byte() {
        // Array of Byte: ARRAY(0x20) | BYTE(0x10) = 0x30
        // Verified from pe_x86_vb_loader Cls_Zip Pack arg[1] = 0x30
        let t = VbType(0x30);
        assert_eq!(t.base_type(), VbType::BYTE);
        assert!(t.is_array());
        assert!(!t.is_byref());
        assert_eq!(format!("{t}"), "Byte()");
    }

    #[test]
    fn test_vb_type_all_base_types() {
        assert_eq!(VbType(0x00).type_name(), "Void");
        assert_eq!(VbType(0x01).type_name(), "Null");
        assert_eq!(VbType(0x02).type_name(), "Integer");
        assert_eq!(VbType(0x04).type_name(), "Single");
        assert_eq!(VbType(0x05).type_name(), "Double");
        assert_eq!(VbType(0x06).type_name(), "Currency");
        assert_eq!(VbType(0x07).type_name(), "Date");
        assert_eq!(VbType(0x0A).type_name(), "Object");
        assert_eq!(VbType(0x0B).type_name(), "Error");
        assert_eq!(VbType(0x0C).type_name(), "Boolean");
        assert_eq!(VbType(0x0D).type_name(), "Variant");
        assert_eq!(VbType(0x0E).type_name(), "Decimal");
        assert_eq!(VbType(0x10).type_name(), "Byte");
        assert_eq!(VbType(0x1D).type_name(), "ExternalCOM");
        assert_eq!(VbType(0x1E).type_name(), "DispatchPtr");
        assert_eq!(VbType(0x1F).type_name(), "Unknown");
    }
}
