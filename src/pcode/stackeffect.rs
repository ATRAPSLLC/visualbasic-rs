//! The evaluation-stack and x87 effect of one P-Code instruction.
//!
//! The P-Code interpreter keeps two stacks: the **evaluation stack**, which is
//! the native stack (`esp`) below the procedure's frame, in 4-byte slots, and
//! the **x87 stack**, which holds Single, Double and Date intermediates. An
//! instruction pops values off the evaluation stack, may push one value, and
//! may pop and push x87 values; some opcodes also read or load Pr, the object
//! register at `[ebp-0x4C]`. [`Instruction::stack_effect`] reports all of it
//! as a [`StackEffect`].
//!
//! # Widths
//!
//! A value on the evaluation stack is 1 slot (an address, an Integer widened
//! to 32 bits, a Long, a String or object pointer), 2 slots (a Currency, or a
//! Double kept off the x87 stack by `FLdR8` and kin) or 4 slots (a Variant
//! passed by value: late-bound arguments, `PopAdLdVar`). Most Variant opcodes
//! work on a Variant's **address**, one slot.
//!
//! # Order
//!
//! [`StackEffect::popped`] lists the values from both stacks in one order:
//! the last operand first. The compiler pushes operands in source order, so
//! on one stack that is top first - a binary operation's right operand, then
//! its left (`SubR8` computes ST1 - ST0) - and for the few operations that
//! read both stacks it is the order the handler takes them as operands:
//! `LtCyR8` is *Currency < Double*, so its list is the Double (x87), then the
//! Currency (2 slots).
//!
//! # What only the callee can say
//!
//! A `VCall*` / `ThisVCall*` releases as many slots as its callee takes, and
//! some calls leave a float result on the x87 stack only if the callee
//! returns one. Pass the [`CallSignature`](super::calltarget::CallSignature)
//! that [`CallResolver`](super::calltarget::CallResolver) found, if any; what
//! it cannot settle is [`Pop::Unknown`] / [`Push::MaybeX87`].
//!
//! The effects come from the handlers of MSVBVM60 6.00.8176 and 6.00.9848
//! (`data/opcodes.csv`).
//!
//! [`Instruction::stack_effect`]: super::decoder::Instruction::stack_effect

use crate::pcode::opcode::Receiver;

/// One popped item of a [`StackEffect`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Pop {
    /// A value on the evaluation stack, this many slots wide.
    Eval(u8),
    /// A value on the x87 stack.
    X87,
    /// Values on the evaluation stack occupying this many slots in all, each
    /// as wide as it was pushed: the arguments of a call whose parameter
    /// types are unknown (`ImpAdCall*` to a `Declare` function, `Print #`
    /// items).
    Slots(u16),
    /// The arguments of a call whose callee is unknown: neither their number
    /// nor their widths are known.
    Unknown,
}

/// The one value an instruction pushes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Push {
    /// A value on the evaluation stack, this many slots wide.
    Eval(u8),
    /// A value on the x87 stack.
    X87,
    /// A call's result on the x87 stack if its callee returns a Single,
    /// Double or Date, nothing otherwise; the callee's return type is not
    /// known.
    MaybeX87,
}

/// Where an instruction loads Pr, the object register (`[ebp-0x4C]`), from,
/// with its operands: [`PrLoad`](super::opcode::PrLoad) and the operands
/// that locate the object.
///
/// Returned by [`Instruction::pr_source`](super::decoder::Instruction::pr_source).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PrSource {
    /// The object in frame slot `ebp + offset` (`FLdPr`).
    Frame {
        /// Offset of the slot from ebp.
        offset: i16,
    },
    /// `Me` (`FLdPrThis`).
    Me,
    /// The value of constant pool entry `index` itself (`ImpAdLdPr`).
    Pool {
        /// The pool entry's index.
        index: u16,
    },
    /// The object the pointer in frame slot `ebp + offset` points to
    /// (`ILdPr`).
    FrameIndirect {
        /// Offset of the slot from ebp.
        offset: i16,
    },
    /// The object at `Pr + offset` (`MemLdPr`).
    PrMember {
        /// Byte offset of the member.
        offset: u16,
    },
    /// The object at `offset` in the object whose pointer is in frame slot
    /// `ebp + frame` (`FMemLdPr`): a member of `Me` (`frame` 8) in a class,
    /// a module variable (`frame` 8 is the module's data block), a member
    /// of a `With` object in a frame temp.
    FrameMember {
        /// Offset of the frame slot from ebp.
        frame: i16,
        /// Byte offset of the member.
        offset: u16,
    },
    /// The ADDRESS of the element of the popped array at the popped indices
    /// (`Ary1LdPr`, `AryLdPr`, `AryInRecLdPr`: Pr points into the array, and
    /// `Mem*` then reach the element's bytes: `AryLdPr; MemStFPR8 0` stores a
    /// Double element).
    ArrayElement,
    /// The object of the Variant whose address it pops (`LdPrVar`,
    /// `LdPrUnkVar`).
    Variant,
    /// The object variable whose address it pops, first set to a new
    /// instance of the class whose ObjectInfo is pool entry `class` if it is
    /// `Nothing` (`NewIfNullPr`).
    NewIfNull {
        /// The pool entry of the class.
        class: u16,
    },
    /// The object at `[[ebp + 0x10] + offset]` (`IWMemLdPr`).
    WithMember {
        /// Byte offset of the member.
        offset: i16,
    },
    /// The object a late-bound get of member `dispid` on the object in
    /// frame slot `ebp + object` returns, kept in the Variant temp at
    /// `ebp + temp` (`FLdLateIdUnkVar`).
    LateGet {
        /// The member's DISPID.
        dispid: i32,
        /// Offset of the frame slot holding the object.
        object: i16,
        /// Offset of the Variant temp from ebp.
        temp: i16,
    },
}

/// What an instruction does to the evaluation and x87 stacks and to Pr.
///
/// Returned by [`Instruction::stack_effect`](super::decoder::Instruction::stack_effect).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackEffect {
    /// The values popped from both stacks, last operand first (see the
    /// [module documentation](self)).
    ///
    /// For a stdcall call (`VCall*`, `ThisVCall*`, `ImpAdCall*`), its
    /// arguments in parameter order (the first parameter is pushed last, so
    /// it is on top), a function's return value pointer last. A late-bound
    /// call and `RaiseEvent` push their Variants in source order: the last
    /// argument is on top (see
    /// [`CallSignature::arg_widths`](super::calltarget::CallSignature::arg_widths)).
    pub popped: Vec<Pop>,
    /// The value pushed, if any. An opcode pushes at most one value.
    ///
    /// A `GoSub`'s push is the return position, on the paths into its
    /// target; its `Return` pops it.
    pub pushed: Option<Push>,
    /// Where the opcode's object, or the call's receiver, comes from.
    ///
    /// A [`Receiver::Popped`] receiver is the first entry of
    /// [`popped`](Self::popped): the Variant whose address is on top.
    pub receiver: Receiver,
    /// Where the opcode loads Pr, the object register, from; `None` if it
    /// does not load it.
    pub pr: Option<PrSource>,
}

impl StackEffect {
    /// Returns the evaluation-stack slots popped, or `None` while a
    /// [`Pop::Unknown`] leaves the count open.
    pub fn pop_slots(&self) -> Option<u16> {
        self.popped.iter().try_fold(0u16, |sum, pop| match *pop {
            Pop::Eval(width) => sum.checked_add(u16::from(width)),
            Pop::Slots(slots) => sum.checked_add(slots),
            Pop::X87 => Some(sum),
            Pop::Unknown => None,
        })
    }

    /// Returns the x87 values popped.
    pub fn fpu_pops(&self) -> u8 {
        let count = self.popped.iter().filter(|&&pop| pop == Pop::X87).count();
        u8::try_from(count).unwrap_or(u8::MAX)
    }

    /// Returns the width in slots of the value pushed onto the evaluation
    /// stack (0: none).
    pub fn pushes(&self) -> u8 {
        match self.pushed {
            Some(Push::Eval(width)) => width,
            _ => 0,
        }
    }

    /// Returns the x87 values pushed, or `None` for a
    /// [`Push::MaybeX87`].
    pub fn fpu_pushes(&self) -> Option<u8> {
        match self.pushed {
            Some(Push::X87) => Some(1),
            Some(Push::MaybeX87) => None,
            _ => Some(0),
        }
    }

    /// Returns `true` if every count is known: no [`Pop::Unknown`] and no
    /// [`Push::MaybeX87`].
    pub fn is_resolved(&self) -> bool {
        self.pop_slots().is_some() && self.fpu_pushes().is_some()
    }

    /// Returns the net change of the evaluation stack in slots (pushed less
    /// popped), or `None` when it is unresolved.
    pub fn net_slots(&self) -> Option<i32> {
        Some(i32::from(self.pushes()).saturating_sub(i32::from(self.pop_slots()?)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn effect(popped: Vec<Pop>, pushed: Option<Push>) -> StackEffect {
        StackEffect {
            popped,
            pushed,
            receiver: Receiver::None,
            pr: None,
        }
    }

    #[test]
    fn test_slots() {
        let e = effect(
            vec![Pop::X87, Pop::Eval(4), Pop::Slots(3), Pop::X87],
            Some(Push::Eval(1)),
        );
        assert_eq!(e.pop_slots(), Some(7));
        assert_eq!(e.fpu_pops(), 2);
        assert_eq!(e.pushes(), 1);
        assert_eq!(e.fpu_pushes(), Some(0));
        assert_eq!(e.net_slots(), Some(-6));
        assert!(e.is_resolved());
    }

    #[test]
    fn test_unresolved() {
        let e = effect(vec![Pop::Unknown], None);
        assert_eq!(e.pop_slots(), None);
        assert_eq!(e.net_slots(), None);
        assert!(!e.is_resolved());
        let e = effect(Vec::new(), Some(Push::MaybeX87));
        assert_eq!(e.fpu_pushes(), None);
        assert!(!e.is_resolved());
        assert_eq!(effect(Vec::new(), Some(Push::X87)).fpu_pushes(), Some(1));
    }
}
