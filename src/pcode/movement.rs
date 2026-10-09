//! What a load, store or literal instruction moves, read from its handler.
//!
//! The mnemonics name a width at most (`MemLdStr` and `FStR4` move a Long;
//! `ILdRf` pushes the four bytes of a frame slot, not its address; opcodes
//! that share a handler behave the same), so each opcode's movement is
//! stated in the opcode table ([`OpcodeInfo::movement`](super::opcode::OpcodeInfo::movement)),
//! from its handler in `MSVBVM60.DLL` 6.00.8176 (6.00.9848's handlers are
//! the same code). [`Movement::of`] reads it for an instruction, with the
//! instruction's operands resolved: where the value comes from or goes to
//! ([`Place`]), how many bytes, and whether the move has side effects beyond
//! the bytes ([`Effect`]: freeing or releasing the old value, `AddRef`,
//! copies, Variant conversions).

use crate::pcode::{decoder::Instruction, operand::Operand};

/// A memory place a move reads or writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Place {
    /// The frame slot `[ebp + offset]`.
    Frame {
        /// Offset from ebp.
        offset: i16,
    },
    /// What the pointer in frame slot `[ebp + offset]` points to.
    FrameIndirect {
        /// Offset from ebp of the slot holding the pointer.
        offset: i16,
    },
    /// `[[ebp + frame] + offset]`: a member of the object or block whose
    /// pointer is in a frame slot (error 91 when the pointer is 0).
    FrameMember {
        /// Offset from ebp of the slot holding the pointer.
        frame: i16,
        /// Byte offset of the member (zero-extended).
        offset: u16,
    },
    /// `[Pr + offset]`: a member of the object in Pr (Pr not checked).
    PrMember {
        /// Byte offset of the member (zero-extended).
        offset: u16,
    },
    /// The variable whose address constant pool entry `index` holds (another
    /// module's variable).
    Global {
        /// The pool entry's index.
        index: u16,
    },
    /// `[[ebp+0x10] + offset]` (what `[ebp+0x10]` holds is not
    /// established).
    WithSlot {
        /// Byte offset (sign-extended).
        offset: i16,
    },
    /// What the pointer stored at `[[ebp+0x10] + offset]` points to.
    WithSlotIndirect {
        /// Byte offset (sign-extended).
        offset: i16,
    },
    /// The evaluation-stack slot `[esp + offset]`, esp before the push.
    StackSlot {
        /// Byte offset from esp.
        offset: i16,
    },
    /// The address popped first from the evaluation stack.
    Popped,
    /// The element of a one-dimensional SAFEARRAY: the array pointer is
    /// popped first, then the index (error 9 out of range or when the array
    /// is not one-dimensional).
    ArrayElement,
    /// The element of a SAFEARRAY of `dims` dimensions: the array pointer is
    /// popped first, then `dims` indices.
    ArrayElementN {
        /// The number of dimensions (and indices).
        dims: u16,
    },
    /// The element of the fixed array that pool entry `descriptor` describes
    /// inside the record whose address is popped first, then `dims` indices.
    RecordArrayElement {
        /// Pool index of the array's descriptor.
        descriptor: u16,
        /// The number of dimensions (and indices).
        dims: u16,
    },
    /// The object of the Variant whose address is popped: `vt` 9 or 13, by
    /// value (`LdPrUnkVar`).
    PoppedVariantObject,
    /// The object of the Variant whose address is popped, one `VT_BYREF`
    /// level followed: `vt` 9, else error 424 (`LdPrVar`).
    PoppedVariantDispatch,
}

/// What a 1- or 2-byte load leaves in the rest of its 4-byte slot (or a
/// literal in its slot).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Fill {
    /// The value fills its slots (4, 8 or 16 bytes).
    Full,
    /// Zeros.
    Zero,
    /// The sign.
    Sign,
    /// Not defined by the load (`mov ax, [..]; push eax` leaves eax's high
    /// half): only the low bytes are the value. `BranchF` / `BranchT` test
    /// the low 16 bits only.
    Undefined,
}

/// The data a Variant literal is built with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VariantData {
    /// Only the `vt` is written.
    None,
    /// A fixed value (`LitVar_TRUE`: -1; `LitVar_Missing`: `0x80020004`,
    /// `DISP_E_PARAMNOTFOUND`).
    Constant(i64),
    /// The instruction's literal operand.
    Operand,
    /// The constant pool entry (a BSTR, shared, not copied).
    PoolEntry,
}

/// A move whose side effects go beyond its bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Effect {
    /// Pops an object pointer, `AddRef`s it, stores it at the place and
    /// `Release`s the old one (helper 0x66102004, flag 0).
    SetObject,
    /// As [`SetObject`](Self::SetObject) without the `AddRef`: the popped
    /// reference (a function's result, `New`) is the one stored.
    SetObjectOwned,
    /// `Set` of a Variant to an object (`vt` 9, `VT_DISPATCH`): the Variant
    /// is freed unless it already holds one, then [`SetObject`](Self::SetObject)
    /// on its data.
    SetVariantDispatch,
    /// [`SetVariantDispatch`](Self::SetVariantDispatch) without the `AddRef`.
    SetVariantDispatchOwned,
    /// `Set` of a Variant to an `IUnknown` (`vt` 13).
    SetVariantUnknown,
    /// [`SetVariantUnknown`](Self::SetVariantUnknown) without the `AddRef`.
    SetVariantUnknownOwned,
    /// `Set` through a by-reference Variant argument (`VT_BYREF` links
    /// followed; a by-reference object slot is set in place; another
    /// by-reference type raises error 13), `vt` 9.
    SetVargDispatch,
    /// [`SetVargDispatch`](Self::SetVargDispatch) without the `AddRef`.
    SetVargDispatchOwned,
    /// [`SetVargDispatch`](Self::SetVargDispatch) for `vt` 13 (an
    /// `IDispatch` target is queried first).
    SetVargUnknown,
    /// [`SetVargUnknown`](Self::SetVargUnknown) without the `AddRef`.
    SetVargUnknownOwned,
    /// `Set` of a Variant from a Variant: pops the destination, then the
    /// source (each followed once through `VT_BYREF|VT_VARIANT`); the source
    /// must hold an object (else error 13); the destination is freed and
    /// takes it, `AddRef`ed.
    SetVariantVariant,
    /// [`SetVariantVariant`](Self::SetVariantVariant) without the `AddRef`,
    /// emptying the source.
    SetVariantVariantMove,
    /// Pops a BSTR and stores it, `SysFreeString`ing the old value: the
    /// string's ownership moves.
    BstrMove,
    /// Stores a copy (`SysAllocStringByteLen`) of a BSTR, freeing the old
    /// value; the source is popped, or with two places it is the second
    /// place's string (`FDupStr`). Error 14 when the copy fails.
    BstrCopy,
    /// Pops a fixed-length string's address and pushes a new BSTR of the
    /// instruction's operand characters.
    BstrFromFixed,
    /// `LSet` into a fixed-length string (pops the destination, then the
    /// source String; the operand is the length in characters).
    FixedStringLset,
    /// [`FixedStringLset`](Self::FixedStringLset), freeing the source after.
    FixedStringLsetFree,
    /// `RSet` into a fixed-length string.
    FixedStringRset,
    /// [`FixedStringRset`](Self::FixedStringRset), freeing the source after.
    FixedStringRsetFree,
    /// Variant `Let`: pops the source's address and frees the place's
    /// Variant; a `VT_DISPATCH` source has its default member stored, a
    /// `VT_UNKNOWN` one raises error 13; else the 16 bytes are moved and the
    /// source emptied.
    VariantLet,
    /// Pops the source's address; a reference type is copied with
    /// `__vbaVarCopy` (a BSTR duplicated, an object `AddRef`ed, an array
    /// copied), the place freed first; the source is unchanged.
    VariantCopy,
    /// As [`VariantCopy`](Self::VariantCopy) with `__vbaVarDup`: an object is
    /// `AddRef`ed and copied without freeing the place. With two places, the
    /// second place's Variant into the first (`FDupVar`).
    VariantDup,
    /// Pops the source's address; frees the place, moves the 16 bytes and
    /// empties the source (no default-member fetch).
    VariantMove,
    /// Stores the popped Variant through a by-reference argument, converted
    /// to the referenced type (else `__vbaVarMove`), then frees the source.
    VargLet,
    /// As [`VargLet`](Self::VargLet), copying (`__vbaVarCopy`).
    VargCopy,
    /// Pushes the 4 bytes at the place, then stores 0 there: the reference
    /// moves to the stack (`FLdZeroAd`).
    ZeroSource,
    /// Pushes 16 bytes: the Variant at the place when it is by reference,
    /// else a `VT_BYREF|VT_VARIANT` reference to it.
    LoadVarg,
    /// Pushes the address of the place's Variant (or of the Variant it
    /// references); a by-reference value of another type is copied
    /// (shallow) into the second place, a frame temp, whose address is
    /// pushed.
    AddressVarg,
    /// Pushes the place's address, or the referenced Variant's address when
    /// it is `VT_BYREF|VT_VARIANT`.
    AddressUnreferenced,
    /// Pops the destination array variable's address, then the source's;
    /// `__vbaAryMove`.
    ArrayMove,
    /// As [`ArrayMove`](Self::ArrayMove) with `__vbaAryCopy`.
    ArrayCopy,
    /// [`ArrayMove`](Self::ArrayMove) of record arrays (`__vbaAryRecMove`).
    RecordArrayMove,
    /// [`ArrayCopy`](Self::ArrayCopy) of record arrays (`__vbaAryRecCopy`).
    RecordArrayCopy,
    /// Pops a Variant's address and pushes the record it holds
    /// (`__vbaUdtVar`).
    RecordFromVariant,
    /// Pops a Variant's address and pushes the SAFEARRAY it holds
    /// (`__vbaAryVar`, its type checked).
    ArrayFromVariant,
    /// Pushes `__vbaLdZeroAry(second place, place)`.
    LoadZeroArray,
    /// Pops the Variant (an array, or an object whose default member is
    /// used), then the index Variants; copies the element into the frame
    /// temp and pushes the temp's address.
    VariantIndexCopy,
    /// The same pops; pushes the element's address.
    VariantIndexAddress,
    /// Pops the Variant, the index Variants and the value Variant; stores
    /// the value into the element (`DISPATCH_PROPERTYPUT`).
    VariantIndexLet,
    /// As [`VariantIndexLet`](Self::VariantIndexLet) with
    /// `DISPATCH_PROPERTYPUTREF`.
    VariantIndexSet,
    /// A late-bound get of the instruction's DISPID on the object at the
    /// place into the second place (a Variant), whose object becomes Pr
    /// (errors 424, 91).
    LateGetPr,
}

impl Effect {
    /// Returns the effect the table names `text` (without a `_keep` suffix).
    fn from_name(text: &str) -> Option<Self> {
        Some(match text {
            "set_obj" => Self::SetObject,
            "set_obj_owned" => Self::SetObjectOwned,
            "set_var9" => Self::SetVariantDispatch,
            "set_var9_owned" => Self::SetVariantDispatchOwned,
            "set_var13" => Self::SetVariantUnknown,
            "set_var13_owned" => Self::SetVariantUnknownOwned,
            "set_varg9" => Self::SetVargDispatch,
            "set_varg9_owned" => Self::SetVargDispatchOwned,
            "set_varg13" => Self::SetVargUnknown,
            "set_varg13_owned" => Self::SetVargUnknownOwned,
            "set_var_var" => Self::SetVariantVariant,
            "set_var_var_move" => Self::SetVariantVariantMove,
            "bstr_move" => Self::BstrMove,
            "bstr_copy" => Self::BstrCopy,
            "bstr_from_fixed" => Self::BstrFromFixed,
            "fixstr_lset" => Self::FixedStringLset,
            "fixstr_lset_free" => Self::FixedStringLsetFree,
            "fixstr_rset" => Self::FixedStringRset,
            "fixstr_rset_free" => Self::FixedStringRsetFree,
            "var_let" => Self::VariantLet,
            "var_copy" => Self::VariantCopy,
            "var_dup" => Self::VariantDup,
            "var_move" => Self::VariantMove,
            "varg_let" => Self::VargLet,
            "varg_copy" => Self::VargCopy,
            "zero_source" => Self::ZeroSource,
            "ld_varg" => Self::LoadVarg,
            "ad_varg" => Self::AddressVarg,
            "ad_unref" => Self::AddressUnreferenced,
            "ary_move" => Self::ArrayMove,
            "ary_copy" => Self::ArrayCopy,
            "ary_rec_move" => Self::RecordArrayMove,
            "ary_rec_copy" => Self::RecordArrayCopy,
            "udt_from_var" => Self::RecordFromVariant,
            "ary_from_var" => Self::ArrayFromVariant,
            "ld_zero_ary" => Self::LoadZeroArray,
            "var_index_copy" => Self::VariantIndexCopy,
            "var_index_ad" => Self::VariantIndexAddress,
            "var_index_let" => Self::VariantIndexLet,
            "var_index_set" => Self::VariantIndexSet,
            "late_get_pr" => Self::LateGetPr,
            _ => return None,
        })
    }
}

/// One move of an instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Move {
    /// Pushes the `bytes` at `place` bitwise (nothing `AddRef`ed or
    /// copied); 16 bytes are a Variant by value (four slots).
    Load {
        /// Where from.
        place: Place,
        /// How many bytes.
        bytes: u8,
        /// The rest of a 1- or 2-byte value's slot.
        fill: Fill,
    },
    /// Loads the float at `place` onto the x87 stack (`fld`).
    LoadFloat {
        /// Where from.
        place: Place,
        /// 4 (Single) or 8 (Double, Date).
        bytes: u8,
    },
    /// Pushes the address of `place`.
    Address {
        /// The place.
        place: Place,
    },
    /// Pops and stores the low `bytes` at `place`; the old value is
    /// overwritten, nothing freed or released.
    Store {
        /// Where to.
        place: Place,
        /// How many bytes.
        bytes: u8,
    },
    /// Stores ST0 at `place` (`fstp`); an x87 exception raises error 6, 11
    /// or 16.
    StoreFloat {
        /// Where to.
        place: Place,
        /// 4 or 8.
        bytes: u8,
    },
    /// Stores `bytes` zero bytes at `place`, nothing popped or freed.
    Zero {
        /// Where to.
        place: Place,
        /// How many bytes.
        bytes: u8,
    },
    /// Pushes the instruction's `bytes`-byte literal operand.
    Literal {
        /// The operand's size.
        bytes: u8,
        /// How it fills its slot(s).
        fill: Fill,
        /// The bytes pushed: 4, or 8 for a value sign-extended to two slots.
        pushed: u8,
    },
    /// Loads the instruction's float operand onto the x87 stack.
    LiteralFloat {
        /// The operand's size.
        bytes: u8,
        /// `true` for an integer operand converted by `fild`.
        integer: bool,
    },
    /// Pushes a constant.
    Constant {
        /// The value.
        value: i32,
    },
    /// Pushes constant pool entry `index` itself (a BSTR literal's address;
    /// nothing copied).
    PoolEntry {
        /// The pool entry's index.
        index: u16,
    },
    /// Builds a Variant literal at `place` (its `vt` and data only; the old
    /// contents are not freed) and pushes its address.
    VariantLiteral {
        /// The Variant type.
        vt: u16,
        /// The data written.
        data: VariantData,
        /// The Variant (a frame temp).
        place: Place,
    },
    /// Loads Pr with the 4 bytes at `place`; `checked`: error 91 when they
    /// are 0.
    LoadPr {
        /// Where from.
        place: Place,
        /// Whether a null object raises error 91.
        checked: bool,
    },
    /// Loads Pr with the address of `place`, not its contents.
    LoadPrAddress {
        /// The place.
        place: Place,
    },
    /// A move with side effects ([`Effect`]).
    Effect {
        /// What it does.
        effect: Effect,
        /// The place it works on.
        place: Place,
        /// A second place, for the effects that name one.
        second: Option<Place>,
        /// `true` if the handler first duplicates the top slot, so the
        /// popped value also stays on the stack.
        keep: bool,
    },
}

/// What one instruction moves: its [`Move`]s, in order.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Movement {
    /// The moves, in the order the handler makes them.
    pub moves: Vec<Move>,
}

impl Movement {
    /// Returns what `instruction` moves, with its operands resolved; `None`
    /// for an instruction that is no load, store or literal.
    pub fn of(instruction: &Instruction) -> Option<Self> {
        let format = instruction.info.operand_format;
        Self::parse(instruction.info.movement, &|spec, occurrence| {
            let index = format
                .split_whitespace()
                .enumerate()
                .filter(|(_, token)| token.trim_start_matches('%').starts_with(spec))
                .nth(occurrence)?
                .0;
            match instruction.operands.get(index).copied().flatten()? {
                Operand::StackVar(value) | Operand::Int16(value) => Some(i64::from(value)),
                Operand::ConstPoolIndex(value) => Some(i64::from(value)),
                Operand::Int32(value) => Some(i64::from(value)),
                Operand::Byte(value) => Some(i64::from(value)),
                _ => None,
            }
        })
    }

    /// Parses the opcode table's notation (see the `movement` column of
    /// `data/opcodes.csv`), reading operand references (`%a`, `%a#2`, `%2`,
    /// `%c`, `%s`) through `operand(spec, occurrence)`, where `occurrence` is
    /// 0 for the first operand of that kind. Returns `None` for an empty or
    /// unreadable text.
    pub fn parse(text: &str, operand: &dyn Fn(char, usize) -> Option<i64>) -> Option<Self> {
        if text.is_empty() {
            return None;
        }
        let moves = text
            .split('+')
            .map(|step| Self::parse_move(step, operand))
            .collect::<Option<Vec<Move>>>()?;
        Some(Self { moves })
    }

    /// Parses one move.
    fn parse_move(step: &str, operand: &dyn Fn(char, usize) -> Option<i64>) -> Option<Move> {
        let (kind, rest) = step.split_once(':')?;
        let place = |text: &str| Self::parse_place(text, operand);
        // `<place>:<width>`.
        let place_width = |text: &'_ str| -> Option<(Place, String)> {
            let (place_text, width) = text.rsplit_once(':')?;
            Some((place(place_text)?, width.to_string()))
        };
        Some(match kind {
            "ld" => {
                let (place, width) = place_width(rest)?;
                let (bytes, fill) = Self::parse_width(&width)?;
                Move::Load { place, bytes, fill }
            }
            "ldfp" => {
                let (place, width) = place_width(rest)?;
                Move::LoadFloat {
                    place,
                    bytes: width.parse().ok()?,
                }
            }
            "ad" => Move::Address {
                place: place(rest)?,
            },
            "st" | "stfp" | "zero" => {
                let (place, width) = place_width(rest)?;
                let bytes = width.parse().ok()?;
                match kind {
                    "st" => Move::Store { place, bytes },
                    "stfp" => Move::StoreFloat { place, bytes },
                    _ => Move::Zero { place, bytes },
                }
            }
            "lit" => {
                let (bytes, suffix) = rest.split_at(rest.find('s').unwrap_or(rest.len()));
                let bytes: u8 = bytes.parse().ok()?;
                let (fill, pushed) = match suffix {
                    "" if bytes >= 4 => (Fill::Full, bytes),
                    "" => (Fill::Zero, 4),
                    "s" => (Fill::Sign, 4),
                    "s8" => (Fill::Sign, 8),
                    _ => return None,
                };
                Move::Literal {
                    bytes,
                    fill,
                    pushed,
                }
            }
            "litfp" => {
                let integer = rest.ends_with('i');
                Move::LiteralFloat {
                    bytes: rest.trim_end_matches('i').parse().ok()?,
                    integer,
                }
            }
            "const" => Move::Constant {
                value: rest.parse().ok()?,
            },
            "pool" => Move::PoolEntry {
                index: u16::try_from(Self::operand_ref(rest, operand)?).ok()?,
            },
            "var" => {
                // `<vt>[=<data>]:<place>`
                let (head, place_text) = rest.split_once(':')?;
                let (vt, data) = match head.split_once('=') {
                    Some((vt, "lit")) => (vt, VariantData::Operand),
                    Some((vt, "pool")) => (vt, VariantData::PoolEntry),
                    Some((vt, value)) => (vt, VariantData::Constant(Self::parse_int(value)?)),
                    None => (head, VariantData::None),
                };
                Move::VariantLiteral {
                    vt: vt.parse().ok()?,
                    data,
                    place: place(place_text)?,
                }
            }
            "prn" | "pr" => Move::LoadPr {
                place: place(rest)?,
                checked: kind == "prn",
            },
            "prad" => Move::LoadPrAddress {
                place: place(rest)?,
            },
            "x" => {
                // `<effect>:<place>[:<place>]`
                let (name, places) = rest.split_once(':')?;
                let (name, keep) = match name.strip_suffix("_keep") {
                    Some(name) => (name, true),
                    None => (name, false),
                };
                let mut parts = Self::split_places(places).into_iter();
                let first = place(parts.next()?)?;
                let second = match parts.next() {
                    Some(text) => Some(place(text)?),
                    None => None,
                };
                Move::Effect {
                    effect: Effect::from_name(name)?,
                    place: first,
                    second,
                    keep,
                }
            }
            _ => return None,
        })
    }

    /// Splits `a:b` at the colons outside parentheses.
    fn split_places(text: &str) -> Vec<&str> {
        let mut parts = Vec::new();
        let mut depth = 0u32;
        let mut start = 0;
        for (at, c) in text.char_indices() {
            match c {
                '(' => depth = depth.saturating_add(1),
                ')' => depth = depth.saturating_sub(1),
                ':' if depth == 0 => {
                    parts.push(text.get(start..at).unwrap_or_default());
                    start = at.saturating_add(1);
                }
                _ => {}
            }
        }
        parts.push(text.get(start..).unwrap_or_default());
        parts
    }

    /// Parses a width: `4`, `8`, `16`, `2u`, `1z`, `2s`.
    fn parse_width(text: &str) -> Option<(u8, Fill)> {
        let (digits, suffix) = text.split_at(
            text.find(|c: char| c.is_ascii_alphabetic())
                .unwrap_or(text.len()),
        );
        let bytes = digits.parse().ok()?;
        let fill = match suffix {
            "" => Fill::Full,
            "z" => Fill::Zero,
            "s" => Fill::Sign,
            "u" => Fill::Undefined,
            _ => return None,
        };
        Some((bytes, fill))
    }

    /// Parses a decimal or `0x` hexadecimal, possibly negative, integer.
    fn parse_int(text: &str) -> Option<i64> {
        let (negative, digits) = match text.strip_prefix('-') {
            Some(digits) => (true, digits),
            None => (false, text),
        };
        let value = match digits.strip_prefix("0x") {
            Some(hex) => i64::from_str_radix(hex, 16).ok()?,
            None => digits.parse().ok()?,
        };
        Some(if negative {
            value.checked_neg()?
        } else {
            value
        })
    }

    /// Reads an operand reference (`%a`, `%a#2`, `%2`, `%c`, `%s`) or a
    /// number.
    fn operand_ref(text: &str, operand: &dyn Fn(char, usize) -> Option<i64>) -> Option<i64> {
        let Some(reference) = text.strip_prefix('%') else {
            return Self::parse_int(text);
        };
        let (spec, occurrence) = match reference.split_once('#') {
            Some((spec, n)) => (spec, n.parse::<usize>().ok()?.checked_sub(1)?),
            None => (reference, 0),
        };
        let mut chars = spec.chars();
        let spec = chars.next()?;
        if chars.next().is_some() {
            return None;
        }
        operand(spec, occurrence)
    }

    /// Parses a place.
    fn parse_place(text: &str, operand: &dyn Fn(char, usize) -> Option<i64>) -> Option<Place> {
        let read = |arg: &str| Self::operand_ref(arg.trim(), operand);
        let i16_of = |arg: &str| i16::try_from(read(arg)?).ok();
        let u16_of = |arg: &str| {
            let value = read(arg)?;
            u16::try_from(value)
                .ok()
                .or_else(|| i16::try_from(value).ok().map(i16::cast_unsigned))
        };
        Some(match text {
            "pop" => Place::Popped,
            "a1" => Place::ArrayElement,
            "vobj(pop)" => Place::PoppedVariantObject,
            "vdisp(pop)" => Place::PoppedVariantDispatch,
            _ => {
                let (name, args) = text.strip_suffix(')')?.split_once('(')?;
                let mut args = args.split(',');
                let first = args.next()?;
                let second = args.next();
                match (name, second) {
                    ("f", None) => Place::Frame {
                        offset: i16_of(first)?,
                    },
                    ("fi", None) => Place::FrameIndirect {
                        offset: i16_of(first)?,
                    },
                    ("m", Some(member)) => Place::FrameMember {
                        frame: i16_of(first)?,
                        offset: u16_of(member)?,
                    },
                    ("pr", None) => Place::PrMember {
                        offset: u16_of(first)?,
                    },
                    ("g", None) => Place::Global {
                        index: u16_of(first)?,
                    },
                    ("w", None) => Place::WithSlot {
                        offset: i16_of(first)?,
                    },
                    ("wi", None) => Place::WithSlotIndirect {
                        offset: i16_of(first)?,
                    },
                    ("s", None) => Place::StackSlot {
                        offset: i16_of(first)?,
                    },
                    ("an", None) => Place::ArrayElementN {
                        dims: u16_of(first)?,
                    },
                    ("ar", Some(dims)) => Place::RecordArrayElement {
                        descriptor: u16_of(first)?,
                        dims: u16_of(dims)?,
                    },
                    _ => return None,
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pcode::opcode::{DispatchTable, table_by_index};

    /// Every movement in the opcode table parses.
    #[test]
    fn every_movement_parses() {
        let mut parsed = 0;
        for table in [
            DispatchTable::Primary,
            DispatchTable::Lead0,
            DispatchTable::Lead1,
            DispatchTable::Lead2,
            DispatchTable::Lead3,
            DispatchTable::Lead4,
        ] {
            for info in table_by_index(table) {
                if info.movement.is_empty() {
                    continue;
                }
                let movement = Movement::parse(info.movement, &|_, _| Some(2));
                assert!(
                    movement.is_some(),
                    "{:?} 0x{:02X} {}: {:?}",
                    table,
                    info.index,
                    info.mnemonic,
                    info.movement
                );
                parsed += 1;
            }
        }
        assert!(parsed > 400, "{parsed}");
    }

    #[test]
    fn test_parse_forms() {
        let operand = |spec: char, occurrence: usize| match (spec, occurrence) {
            ('a', 0) => Some(-0x88),
            ('a', 1) => Some(-0x90),
            ('2', 0) => Some(0x10),
            _ => None,
        };
        assert_eq!(
            Movement::parse("ld:f(%a):2u", &operand).unwrap().moves,
            vec![Move::Load {
                place: Place::Frame { offset: -0x88 },
                bytes: 2,
                fill: Fill::Undefined
            }]
        );
        assert_eq!(
            Movement::parse("x:bstr_copy:m(%a,%2)", &operand)
                .unwrap()
                .moves,
            vec![Move::Effect {
                effect: Effect::BstrCopy,
                place: Place::FrameMember {
                    frame: -0x88,
                    offset: 0x10
                },
                second: None,
                keep: false
            }]
        );
        assert_eq!(
            Movement::parse("x:var_dup:f(%a#2):f(%a)", &operand)
                .unwrap()
                .moves,
            vec![Move::Effect {
                effect: Effect::VariantDup,
                place: Place::Frame { offset: -0x90 },
                second: Some(Place::Frame { offset: -0x88 }),
                keep: false
            }]
        );
        assert_eq!(
            Movement::parse("st:f(%a):4+ad:f(%a)", &operand)
                .unwrap()
                .moves
                .len(),
            2
        );
        assert_eq!(
            Movement::parse("var:11=-1:f(%a)", &operand).unwrap().moves,
            vec![Move::VariantLiteral {
                vt: 11,
                data: VariantData::Constant(-1),
                place: Place::Frame { offset: -0x88 }
            }]
        );
        assert_eq!(
            Movement::parse("lit:4s8", &operand).unwrap().moves,
            vec![Move::Literal {
                bytes: 4,
                fill: Fill::Sign,
                pushed: 8
            }]
        );
        assert!(Movement::parse("", &operand).is_none());
        assert!(Movement::parse("ld:q(1):4", &operand).is_none());
    }
}
