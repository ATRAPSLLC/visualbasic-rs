//! The method link table: the entry points of an object's COM vtable.
//!
//! See [`MethodLink`] for its layout and entry formats.

use crate::{addressmap::AddressMap, error::Error, util::read_u32_le};

/// One entry of the method link table: a vtable slot's code address.
///
/// The table at
/// [`OptionalObjectInfo::method_link_table_va`](crate::vb::object::OptionalObjectInfo::method_link_table_va)
/// (+0x30) is a flat `u32[method_link_count]` (count at +0x28) array of code
/// addresses. Entry `k` is the vtable slot at byte offset `0x1C + 4 * k`, the
/// first slot after `IDispatch`'s seven: in `calls`, `Square.Init` is entry 3
/// (vtable 0x28) and `Counter`'s private `Bump` entry 13 (vtable 0x50), the
/// offsets its P-Code `VCallHresult`s use.
///
/// The table is in vtable order, not method table order (`forms` `Board`:
/// entries 0-3 are methods 3-6, its public members, then methods 0-2 and
/// 7-10). In a class (`tests/fixtures/members`, `events`, `data`):
///
/// 1. the accessors of its plain `Public` variables, in declaration order
///    (`Get` then `Let`; `Get`, `Let`, `Set` for a Variant or object);
/// 2. three null entries per `Implements`;
/// 3. the accessors of its `Public WithEvents` variables (`Get`, `Let`,
///    `Set`);
/// 4. its public procedures, in declaration order;
/// 5. its other methods (`Friend`, `Private`, event handlers,
///    `Class_Initialize`) in method-table order;
/// 6. the accessors of its `Private WithEvents` variables.
///
/// `members` `Mixed` has every group: `Value` and `Note`, the `IFirst`
/// gap, `Shown`, `First` and `Last`, `Inner` and the handlers, `Hidden`.
///
/// # Entry Formats
///
/// In the P-Code fixtures an entry is one of:
///
/// ```text
/// 33 C0 BA <ProcDscInfo> 68 <jmp [MethCallEngine]> C3
///     xor eax, eax; mov edx, <ProcDscInfo>; push <thunk>; ret
///     A method (the method entry of the 20-byte stub `crate::vb::events` describes).
/// 81 44 24 04 <offset> B9 <jmp [GetMemStr]> FF E1
///     add [esp+4], <offset>; mov ecx, <thunk>; jmp ecx
///     A public variable's accessor (`data` `Item.Name`, offset 0x34: one
///     entry to GetMemStr, one to PutMemStr).
/// 58 81 04 24 <offset> 68 00000000 68 <va> 50 B9 <jmp [GetMemEvent]> FF E1
///     pop eax; add [esp], <offset>; push 0; push <va>; push eax; mov ecx, <thunk>; jmp ecx
///     A `WithEvents` variable's accessor (`events` `Listener.m_Source`,
///     offset 0x38: GetMemEvent, PutMemEvent, SetMemEvent).
/// 00000000
///     A reserved slot of a class that implements an interface.
/// ```
///
/// In a native build (`events-native`, `events` compiled to native code)
/// a procedure's entry is a 5-byte `E9 rel32` jump into its native code;
/// the order, the null entries and the variable accessors are the P-Code
/// build's. The bytes after the jump belong to the next stub.
///
/// [`MethodLinkIterator`] decodes each entry into a [`MethodLinkKind`] and
/// follows an `E9 rel32` entry to its target ([`MethodLink::code_va`]).
#[derive(Debug, Clone, Copy)]
pub struct MethodLink {
    /// The entry: VA of the slot's code, 0 for an
    /// [`Empty`](MethodLinkKind::Empty) entry.
    pub thunk_va: u32,
    /// Target of the entry's `E9 rel32` jump (a native procedure), or
    /// [`thunk_va`](Self::thunk_va) itself when the entry does not start
    /// with `E9` (a P-Code stub, a variable accessor).
    pub code_va: u32,
    /// What the entry's code is.
    pub kind: MethodLinkKind,
}

/// What a [`MethodLink`] entry's code is (see the entry formats on
/// [`MethodLink`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MethodLinkKind {
    /// A null entry: one of the three reserved slots per interface a class
    /// implements (`calls` `Square`, `events` `Ring`; six in `members`
    /// `Two`, nine in `Three`).
    Empty,
    /// A `mov edx, <ProcDscInfo>` stub (`33 C0 BA imm32`, or `BA imm32`)
    /// that enters one of the object's P-Code procedures through
    /// `MethCallEngine`.
    Procedure {
        /// VA of the procedure's `ProcDscInfo`, the value of the object's
        /// method table slot for that procedure.
        proc_dsc_va: u32,
    },
    /// A member variable's accessor: it adds `offset` to `this` and jumps
    /// through `thunk_va` (a `jmp [IAT slot]` import thunk) to the runtime's
    /// `GetMem*`, `PutMem*` or `SetMem*` function.
    Variable {
        /// Byte offset of the variable in the object instance (`data`
        /// `Item.Name`: 0x34; `events` `Listener.m_Source`: 0x38).
        offset: u32,
        /// VA of the import thunk the accessor jumps through.
        thunk_va: u32,
        /// The two values a `New` or `WithEvents` variable's accessor pushes
        /// for the runtime, in push order; `None` for the other variables'
        /// accessors, which push nothing. For a `New` variable
        /// (`GetMemNewObj`, `PutMemNewObj`, `SetMemNewObj`) they are the
        /// constant pool index of what to create and the pool's VA
        /// (`members` `Holder.Auto As New Collection`: 0 and 0x00401F6C).
        pushed: Option<[u32; 2]>,
    },
    /// An `E9 rel32` jump to [`MethodLink::code_va`].
    Jump,
    /// None of the above.
    Other,
}

impl MethodLinkKind {
    /// Decodes the kind of the entry whose code starts with `code`.
    fn decode(code: &[u8]) -> Self {
        match code {
            [0x33, 0xC0, 0xBA, a, b, c, d, ..] | [0xBA, a, b, c, d, ..] => Self::Procedure {
                proc_dsc_va: u32::from_le_bytes([*a, *b, *c, *d]),
            },
            [
                0x58,
                0x81,
                0x04,
                0x24,
                o0,
                o1,
                o2,
                o3,
                0x68,
                a0,
                a1,
                a2,
                a3,
                0x68,
                b0,
                b1,
                b2,
                b3,
                0x50,
                0xB9,
                t0,
                t1,
                t2,
                t3,
                0xFF,
                0xE1,
                ..,
            ] => Self::Variable {
                offset: u32::from_le_bytes([*o0, *o1, *o2, *o3]),
                thunk_va: u32::from_le_bytes([*t0, *t1, *t2, *t3]),
                pushed: Some([
                    u32::from_le_bytes([*a0, *a1, *a2, *a3]),
                    u32::from_le_bytes([*b0, *b1, *b2, *b3]),
                ]),
            },
            [
                0x81,
                0x44,
                0x24,
                0x04,
                o0,
                o1,
                o2,
                o3,
                0xB9,
                t0,
                t1,
                t2,
                t3,
                0xFF,
                0xE1,
                ..,
            ] => Self::Variable {
                offset: u32::from_le_bytes([*o0, *o1, *o2, *o3]),
                thunk_va: u32::from_le_bytes([*t0, *t1, *t2, *t3]),
                pushed: None,
            },
            [0xE9, ..] => Self::Jump,
            _ => Self::Other,
        }
    }
}

/// Iterator over the method link table of a VB6 object.
///
/// Walks the table at
/// [`OptionalObjectInfo::method_link_table_va`](crate::vb::object::OptionalObjectInfo::method_link_table_va),
/// reading each 4-byte entry and decoding it into a [`MethodLink`]. A null
/// entry (the reserved slots of a class that implements an interface)
/// yields a link of kind [`MethodLinkKind::Empty`]. An entry that cannot be
/// read yields an `Err`.
#[must_use = "iterators are lazy and do nothing unless consumed"]
pub struct MethodLinkIterator<'a, 'p> {
    /// Address map for VA resolution.
    map: &'p AddressMap<'a>,
    /// Base VA of the method link table.
    table_va: u32,
    /// Current zero-based position in the table.
    index: u32,
    /// Number of entries to read.
    total: u32,
}

impl<'a, 'p> MethodLinkIterator<'a, 'p> {
    /// Creates an iterator over the `total` entries of the table at
    /// `table_va`.
    ///
    /// `total` is capped at the number of 4-byte entries the file holds from
    /// `table_va` on, so a corrupt count cannot make the walk outlast the
    /// data.
    pub fn new(map: &'p AddressMap<'a>, table_va: u32, total: u32) -> Self {
        let available = map.slice_from_va(table_va, 0).map_or(0, <[u8]>::len) / 4;
        Self {
            map,
            table_va,
            index: 0,
            total: total.min(u32::try_from(available).unwrap_or(u32::MAX)),
        }
    }

    /// Reads and decodes the entry at `ptr_va`.
    fn read(&self, ptr_va: u32) -> Result<MethodLink, Error> {
        let thunk_va = read_u32_le(self.map.slice_from_va(ptr_va, 4)?, 0)?;
        if thunk_va == 0 {
            return Ok(MethodLink {
                thunk_va,
                code_va: 0,
                kind: MethodLinkKind::Empty,
            });
        }
        let code = self.map.slice_from_va(thunk_va, 5)?;
        let kind = MethodLinkKind::decode(code);
        let code_va = match code {
            [0xE9, r0, r1, r2, r3, ..] => {
                let rel32 = i32::from_le_bytes([*r0, *r1, *r2, *r3]);
                thunk_va.wrapping_add(5).wrapping_add_signed(rel32)
            }
            _ => thunk_va,
        };
        Ok(MethodLink {
            thunk_va,
            code_va,
            kind,
        })
    }
}

impl<'a, 'p> Iterator for MethodLinkIterator<'a, 'p> {
    type Item = Result<MethodLink, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.total || self.table_va == 0 {
            return None;
        }
        let ptr_va = self.table_va.wrapping_add(self.index.saturating_mul(4));
        self.index = self.index.saturating_add(1);
        Some(self.read(ptr_va))
    }
}
