//! Method dispatch table entry classification.
//!
//! [`MethodEntry`] classifies each slot of an object's method table.

use crate::{
    addressmap::AddressMap,
    error::Error,
    project::PCodeMethod,
    util::{read_u16_le, read_u32_le},
};

/// Classification of a single entry in the method dispatch table.
///
/// An object's method table
/// ([`ObjectInfo::methods_va`](crate::vb::object::ObjectInfo::methods_va))
/// has one 4-byte slot per method. In every P-Code fixture
/// (`tests/fixtures`), a procedure's slot points straight at its
/// [`ProcDscInfo`](crate::vb::procedure::ProcDscInfo), whose first dword
/// points back at the object's `ObjectInfo`, in modules, classes, forms and
/// UserControls alike. A standard module also counts each `Declare` as a
/// method, ahead of its procedures, and its table holds a slot for each, but
/// the compiler writes nothing there: a `Declare` has no procedure, so its
/// slot keeps whatever the compiler's output buffer held (`vtable`: 13
/// `Declare`s, slots 0-12 hold values such as `0x00000409` and
/// `0x62617456`, ASCII `"Vtab"`). Those slots are
/// [`Declare`](Self::Declare).
#[derive(Debug)]
pub enum MethodEntry<'a> {
    /// The slot holds 0. No fixture has one outside a standard module's
    /// `Declare` slots, which are [`Declare`](Self::Declare).
    Null,
    /// A standard module's `Declare` statement: one of the slots before the
    /// module's first procedure. The slot holds no pointer and its value is
    /// not read (`exprs`: slots 0-1, `types`: 0-2, `vtable`: 0-12).
    Declare,
    /// A P-Code procedure: the slot points at its `ProcDscInfo` (every
    /// fixture), or at a `mov edx, <ProcDscInfo>` stub (`BA imm32` or
    /// `33 C0 BA imm32`).
    PCode(PCodeMethod<'a>),
    /// A slot that points inside the PE image at neither a P-Code stub nor a
    /// `ProcDscInfo` of the object: taken to be native code. No fixture
    /// yields one; the natively compiled `flow-native` module has no method
    /// table (its `ObjectInfo` method count is 0).
    Native {
        /// Virtual address of the native method body.
        va: u32,
    },
    /// A non-zero slot value outside the PE image, such as an address in
    /// MSVBVM60.DLL. No fixture has one outside a standard module's
    /// `Declare` slots.
    Runtime {
        /// The slot value.
        va: u32,
    },
}

impl<'a> MethodEntry<'a> {
    /// Reads and classifies a single method table entry.
    ///
    /// Used by [`MethodIterator`](super::MethodIterator) and
    /// [`PCodeMethodIterator`](super::PCodeMethodIterator). It does not know
    /// about `Declare` slots; the iterators decide those first (see
    /// [`is_procedure`](Self::is_procedure)).
    ///
    /// # Arguments
    ///
    /// * `map` - Address map for VA-to-offset resolution.
    /// * `methods_va` - Base VA of the method dispatch table.
    /// * `index` - Zero-based slot within the table.
    /// * `object_info_va` - VA of the owning object's `ObjectInfo`.
    ///
    /// # Returns
    ///
    /// A [`MethodEntry`] variant indicating the slot's type:
    /// - [`Null`](MethodEntry::Null) if the VA is zero.
    /// - [`Runtime`](MethodEntry::Runtime) if the VA falls outside the PE image.
    /// - [`PCode`](MethodEntry::PCode) if the target starts with `BA` or
    ///   `33 C0 BA` (a `mov edx, <ProcDscInfo>` stub), or is a `ProcDscInfo`
    ///   of the object: its first dword is `object_info_va` and its P-Code
    ///   size at +0x08 is not 0 (every fixture slot).
    /// - [`Native`](MethodEntry::Native) otherwise.
    ///
    /// # Errors
    ///
    /// Returns an error if the method table entry or the bytes at the
    /// target VA cannot be read, or if a stub's `ProcDscInfo` does not parse.
    pub(crate) fn classify(
        map: &AddressMap<'a>,
        methods_va: u32,
        index: u16,
        object_info_va: u32,
    ) -> Result<MethodEntry<'a>, Error> {
        let method_va = Self::slot(map, methods_va, index)?;
        if method_va == 0 {
            return Ok(MethodEntry::Null);
        }
        if !map.is_va_in_image(method_va) {
            return Ok(MethodEntry::Runtime { va: method_va });
        }
        let stub_data = map.slice_from_va(method_va, 12)?;
        if Self::stub_target(stub_data).is_some() {
            return Ok(MethodEntry::PCode(PCodeMethod::parse(
                map, methods_va, index,
            )?));
        }
        if Self::is_proc_dsc_of(map, method_va, object_info_va)
            && let Ok(pcode) = PCodeMethod::parse(map, methods_va, index)
        {
            return Ok(MethodEntry::PCode(pcode));
        }
        Ok(MethodEntry::Native { va: method_va })
    }

    /// Returns `true` if slot `index` of the method table names a procedure
    /// of the object whose `ObjectInfo` is at `object_info_va`: the slot
    /// points at that object's `ProcDscInfo`, directly or through a
    /// `mov edx, <ProcDscInfo>` stub.
    ///
    /// A standard module's `Declare` slots, which hold no pointer, give
    /// `false`; so does a slot that cannot be read.
    pub(crate) fn is_procedure(
        map: &AddressMap<'a>,
        methods_va: u32,
        index: u16,
        object_info_va: u32,
    ) -> bool {
        let Ok(method_va) = Self::slot(map, methods_va, index) else {
            return false;
        };
        let Ok(stub_data) = map.slice_from_va(method_va, 12) else {
            return false;
        };
        let proc_dsc_va = Self::stub_target(stub_data).unwrap_or(method_va);
        Self::is_proc_dsc_of(map, proc_dsc_va, object_info_va)
    }

    /// Reads slot `index` of the method table at `methods_va`.
    fn slot(map: &AddressMap<'a>, methods_va: u32, index: u16) -> Result<u32, Error> {
        let entry_va = methods_va.wrapping_add(u32::from(index).wrapping_mul(4));
        read_u32_le(map.slice_from_va(entry_va, 4)?, 0)
    }

    /// The `ProcDscInfo` VA a `mov edx, imm32` stub (`BA imm32`, or
    /// `33 C0 BA imm32`) loads, or `None` if `code` is neither form.
    fn stub_target(code: &[u8]) -> Option<u32> {
        match code {
            [0xBA, a, b, c, d, ..] | [0x33, 0xC0, 0xBA, a, b, c, d, ..] => {
                Some(u32::from_le_bytes([*a, *b, *c, *d]))
            }
            _ => None,
        }
    }

    /// Returns `true` if `va` holds a `ProcDscInfo` owned by the object
    /// whose `ObjectInfo` is at `object_info_va`: its first dword points
    /// there (`ProcCallEngine` loads the `ObjectInfo` from it, MSVBVM60
    /// 6.00.8176 `0x66104ADA`) and its P-Code size at +0x08 is not 0.
    fn is_proc_dsc_of(map: &AddressMap<'a>, va: u32, object_info_va: u32) -> bool {
        let Ok(data) = map.slice_from_va(va, 10) else {
            return false;
        };
        read_u32_le(data, 0).is_ok_and(|owner| owner == object_info_va)
            && read_u16_le(data, 8).is_ok_and(|size| size != 0)
    }
}
