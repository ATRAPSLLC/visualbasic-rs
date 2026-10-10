//! MSVBVM60.DLL export function signature database.
//!
//! Provides lookup of calling conventions, parameter types, and return types
//! for the exported functions of the VB6 runtime DLL.
//!
//! The table has one entry per export of MSVBVM60.DLL: 635 named exports,
//! ordinals 100 to 2010, the same names and ordinals in 6.00.8176 and
//! 6.00.9848. 31 entries have [`CallingConv::Unknown`] (named, not
//! described). It is generated at build time from
//! `data/msvbvm60_exports.csv`; the signatures describe 6.00.9848.

mod generated {
    include!(concat!(env!("OUT_DIR"), "/msvbvm60_exports_generated.rs"));
}

/// Calling convention for an MSVBVM60 export.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallingConv {
    /// `__fastcall`: first two integer/pointer args in ECX, EDX; callee cleans stack.
    Fastcall,
    /// `__stdcall`: all args on stack; callee cleans stack.
    Stdcall,
    /// `__cdecl`: all args on stack; caller cleans stack.
    Cdecl,
    /// Special: an FDIV/FPU workaround (`_adj_*`), the x87 intrinsics `_CIexp`
    /// and `_CItan`, `__vbaChkstk`, or the P-Code engines (`ProcCallEngine`,
    /// `MethCallEngine`). No standard parameter passing - skip during
    /// prototype application. The other `_CI*` intrinsics, which take and
    /// return their value on the x87 stack the same way, are listed as
    /// [`Cdecl`](CallingConv::Cdecl) with no parameters.
    Special,
    /// Not analyzed: the export is named, its signature is not described.
    Unknown,
}

/// Parameter or return type for an MSVBVM60 export.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VbParamType {
    /// `void` - no value.
    Void,
    /// `int16_t` - 16-bit signed integer (VB6 `Integer`).
    Int16,
    /// `uint16_t` - 16-bit unsigned integer.
    UInt16,
    /// `int32_t` - 32-bit signed integer (VB6 `Long`).
    Int32,
    /// `uint32_t` - 32-bit unsigned integer.
    UInt32,
    /// `int64_t` - 64-bit signed integer (VB6 `Currency` raw).
    Int64,
    /// `uint8_t` - 8-bit unsigned (VB6 `Byte`).
    UInt8,
    /// `float` - 32-bit IEEE 754 (VB6 `Single`).
    Float,
    /// `double` - 64-bit IEEE 754 (VB6 `Double`).
    Double,
    /// `BOOL` - Win32 boolean (32-bit).
    Bool,
    /// `BSTR` - pointer to `SysAllocString`'d wide string.
    Bstr,
    /// `BSTR*` - pointer to BSTR location.
    BstrPtr,
    /// `VARIANT*` - pointer to 16-byte COM VARIANT.
    VariantPtr,
    /// `SAFEARRAY*` - pointer to COM safe array.
    SafeArrayPtr,
    /// `SAFEARRAY**` - pointer to SAFEARRAY pointer.
    SafeArrayPtrPtr,
    /// `IUnknown*` - COM interface pointer.
    IUnknownPtr,
    /// `IUnknown**` - pointer to COM interface pointer.
    IUnknownPtrPtr,
    /// `IDispatch*` - COM dispatch interface pointer.
    IDispatchPtr,
    /// `IDispatch**` - pointer to COM dispatch pointer.
    IDispatchPtrPtr,
    /// `HRESULT` - 32-bit COM result code.
    Hresult,
    /// `GUID*` - pointer to 16-byte COM GUID.
    GuidPtr,
    /// `void*` - opaque pointer.
    VoidPtr,
    /// `int32_t*` - pointer to 32-bit integer.
    Int32Ptr,
    /// `int16_t*` - pointer to 16-bit integer.
    Int16Ptr,
    /// `uint8_t*` - pointer to byte.
    UInt8Ptr,
    /// `int64_t*` - pointer to 64-bit integer.
    Int64Ptr,
    /// Not analyzed.
    Unknown,
}

/// A single parameter in an export function signature.
#[derive(Debug, Clone, Copy)]
pub struct ExportParam {
    /// Parameter type.
    pub ty: VbParamType,
    /// Parameter name (e.g., `"pbstr"`, `"pvar"`).
    pub name: &'static str,
}

/// Complete function signature for an MSVBVM60 export.
#[derive(Debug, Clone, Copy)]
pub struct ExportSignature {
    /// Export name (e.g., `"__vbaFreeStr"` or `"rtcDoEvents"`).
    pub name: &'static str,
    /// Export ordinal in MSVBVM60.DLL (100 to 2010). Every entry has one.
    pub ordinal: u16,
    /// Calling convention.
    pub calling_convention: CallingConv,
    /// Return type.
    pub return_type: VbParamType,
    /// Whether this function accepts variable arguments after fixed params.
    pub variadic: bool,
    /// Whether this function never returns to its caller: it raises a VB
    /// error on every path (`__vbaErrorOverflow`, `__vbaGenerateBoundsError`,
    /// `__vbaError`, `__vbaFPException`, `__vbaFailedFriend`,
    /// `rtcAppleScript`), ends the program (`__vbaEnd`, `__vbaStopExe`, both
    /// through the error the runtime unwinds on) or the process
    /// (`ThunRTMain`). The error raiser ends in a noncontinuable
    /// `RaiseException` (MSVBVM60 6.00.8176 `0x66026D91`).
    pub noreturn: bool,
    /// Fixed parameter list (in calling order).
    pub params: &'static [ExportParam],
    /// Functional category (e.g., `"free"`, `"string"`, `"variant"`).
    pub category: &'static str,
}

/// Look up an MSVBVM60 export signature by name.
///
/// The `name` is the export name (e.g., `"__vbaFreeStr"`, `"ThunRTMain"`,
/// `"_CIcos"`). A VB6 executable imports the `__vba*` functions, the
/// engines, `EVENT_SINK_*`, `DllFunctionCall` and the `_CI*`/`_adj_*`
/// intrinsics by name, but `ThunRTMain`, the `rtc*` functions and the
/// `GetMem*`/`PutMem*`/`SetMem*` accessors by ordinal: look those up with
/// [`lookup_export_by_ordinal`].
///
/// Returns `None` if the name is not in the database.
pub fn lookup_export(name: &str) -> Option<&'static ExportSignature> {
    generated::lookup_export_by_name(name)
}

/// Look up an MSVBVM60 export signature by ordinal number.
///
/// Used for imports by ordinal: an import table entry for ordinal 598
/// (which disassemblers show as `MSVBVM60.598` or `Ordinal_MSVBVM60_598`)
/// maps to `rtcDoEvents`.
///
/// Returns `None` if the ordinal is not in the database.
pub fn lookup_export_by_ordinal(ordinal: u16) -> Option<&'static ExportSignature> {
    generated::lookup_export_by_ordinal(ordinal)
}

/// Returns the full export signature table.
///
/// Sorted by name (byte order) for binary search. Useful for iteration.
pub fn all_exports() -> &'static [ExportSignature] {
    generated::EXPORTS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_vba_free_str() {
        let sig = lookup_export("__vbaFreeStr").expect("__vbaFreeStr not found");
        assert_eq!(sig.calling_convention, CallingConv::Fastcall);
        assert_eq!(sig.return_type, VbParamType::Void);
        assert_eq!(sig.params.len(), 1);
        assert_eq!(sig.params[0].ty, VbParamType::BstrPtr);
        assert!(!sig.variadic);
    }

    #[test]
    fn lookup_vba_free_str_list() {
        let sig = lookup_export("__vbaFreeStrList").expect("not found");
        assert_eq!(sig.calling_convention, CallingConv::Cdecl);
        assert!(sig.variadic);
    }

    #[test]
    fn lookup_by_ordinal() {
        let sig = lookup_export_by_ordinal(598).expect("ordinal 598 not found");
        assert_eq!(sig.name, "rtcDoEvents");
        assert_eq!(sig.calling_convention, CallingConv::Stdcall);
    }

    #[test]
    fn lookup_missing_returns_none() {
        assert!(lookup_export("nonexistent_function").is_none());
        assert!(lookup_export_by_ordinal(9999).is_none());
    }

    #[test]
    fn table_is_sorted_by_name() {
        let exports = all_exports();
        for w in exports.windows(2) {
            assert!(
                w[0].name <= w[1].name,
                "table not sorted: {:?} > {:?}",
                w[0].name,
                w[1].name
            );
        }
    }

    #[test]
    fn ordinals_are_unique() {
        let exports = all_exports();
        let ordinals: Vec<u16> = exports
            .iter()
            .filter(|e| e.ordinal > 0)
            .map(|e| e.ordinal)
            .collect();
        let mut deduped = ordinals.clone();
        deduped.sort();
        deduped.dedup();
        assert_eq!(ordinals.len(), deduped.len(), "duplicate ordinals found");
    }

    #[test]
    fn all_exports_nonempty() {
        assert!(
            all_exports().len() > 100,
            "expected >100 exports, got {}",
            all_exports().len()
        );
    }
}
