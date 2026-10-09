//! The evaluation and x87 stacks of a procedure, simulated over its control
//! flow.
//!
//! [`StackEffect`] says what one instruction pops and pushes, partly in
//! slots: the arguments of a `Declare` function are a [`Pop::Slots`] run,
//! since the executable does not record their types. Over a whole procedure
//! the producers of those values are known, so [`ProcedureStack::simulate`]
//! walks every path from the entry (and from each `On Error` handler, which
//! the runtime enters with empty stacks), tracks every value with the width
//! its producer pushed, and gives per instruction:
//!
//! - the stacks on entry ([`StackState`]);
//! - the effect with every value resolved ([`ResolvedEffect`]): slot runs
//!   split into the values pushed, and a call whose x87 result depends on an
//!   unknown callee settled by the instruction after it;
//! - or why a path stopped there ([`Cut`]): a call whose argument count only
//!   its (unknown) callee could give, or a pop that found no value of its
//!   width.
//!
//! A `GoSub` pushes its return position, on the path into its body only;
//! the body's `Return` pops it, and the instruction after the `GoSub` is
//! reached with the stacks as the `GoSub` found them. A `GoSub` body shared
//! by `GoSub`s at different depths is simulated once per depth, and must
//! find the same statement values at each.
//!
//! The simulation checks what it relies on: two paths reaching an
//! instruction must bring the same values ([`ProcedureStack::merge_conflicts`]).
//! On the compiled fixtures every path that is not cut agrees, every
//! statement (`LargeBos`) starts with no value on the stack, and every exit
//! leaves both stacks empty (`tests/stack.rs`).

use std::collections::HashMap;

use crate::pcode::{
    calltarget::{CallSignature, Callee},
    decoder::Instruction,
    semantics::OpcodeSemantics,
    stackeffect::{Pop, PrSource, Push, StackEffect},
};

/// A value on one of the two stacks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StackValue {
    /// A value on the evaluation stack, this many 4-byte slots wide.
    Eval(u8),
    /// A value on the x87 stack.
    X87,
    /// The return position a `GoSub` pushed on the evaluation stack (one
    /// slot).
    GoSubReturn,
}

/// The two stacks at one point of a procedure.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct StackState {
    /// The evaluation stack, bottom first: [`StackValue::Eval`] values and
    /// `GoSub` return positions.
    pub eval: Vec<StackValue>,
    /// The values on the x87 stack.
    pub x87: u8,
}

impl StackState {
    /// Returns the evaluation stack's depth in slots.
    pub fn eval_slots(&self) -> u32 {
        self.eval
            .iter()
            .map(|value| match value {
                StackValue::Eval(width) => u32::from(*width),
                _ => 1,
            })
            .sum()
    }

    /// Returns the number of `GoSub` return positions on the stack: how many
    /// `GoSub` bodies the point is inside.
    pub fn gosub_depth(&self) -> usize {
        self.eval
            .iter()
            .filter(|&&value| value == StackValue::GoSubReturn)
            .count()
    }

    /// Returns the values above the innermost `GoSub` return position: the
    /// ones the current statement pushed.
    pub fn statement_values(&self) -> &[StackValue] {
        let start = self
            .eval
            .iter()
            .rposition(|&value| value == StackValue::GoSubReturn)
            .map_or(0, |at| at.saturating_add(1));
        self.eval.get(start..).unwrap_or_default()
    }

    /// Pops one value off the evaluation stack and checks its width.
    fn pop_eval(&mut self, width: u8) -> Result<StackValue, PopFailure> {
        match self.eval.pop() {
            Some(StackValue::Eval(found)) if found == width => Ok(StackValue::Eval(found)),
            Some(found) => Err(PopFailure::Width {
                expected: width,
                found,
            }),
            None => Err(PopFailure::Empty),
        }
    }
}

/// Why a pop failed: the instruction's stated effect disagrees with the
/// values its paths bring.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PopFailure {
    /// The stack it pops is empty.
    Empty,
    /// The value on top is another one: of another width, or a `GoSub`
    /// return position.
    Width {
        /// The width the instruction pops.
        expected: u8,
        /// The value found.
        found: StackValue,
    },
    /// A run of slots ends inside a value.
    Split {
        /// The slots of the run.
        slots: u16,
    },
    /// A `Return` found no `GoSub` return position on top.
    NoReturnPosition,
}

/// Why the simulation stopped at an instruction.
#[derive(Debug, Clone)]
pub enum Cut {
    /// A call whose arguments only its callee's signature could count: the
    /// callee as [`CallResolver`](super::calltarget::CallResolver) found it
    /// (an [`External`](Callee::External) interface its type library
    /// describes, a `VCall*` on Pr of unknown class), or
    /// `None` without a resolver.
    UnknownCallee(Option<Callee>),
    /// A pop found no value of the width it pops.
    Pop(PopFailure),
}

/// An instruction's effect with every value resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedEffect {
    /// The values popped, last operand first (see
    /// [`StackEffect::popped`]); a slot run split into the values that
    /// were pushed.
    pub popped: Vec<StackValue>,
    /// The value pushed, if any: for a `GoSub`, its return position, on the
    /// path into its body.
    pub pushed: Option<StackValue>,
    /// `true` if the x87 result of a call whose callee's return type is
    /// unknown ([`Push::MaybeX87`]) was settled by the instruction after it:
    /// a float result if that instruction takes more x87 values than there
    /// are.
    pub x87_inferred: bool,
}

/// What the simulation found at one instruction.
#[derive(Debug, Clone)]
pub enum InstructionStack {
    /// No simulated path reaches it.
    Unreached,
    /// Reached: the stacks on entry and the instruction's resolved effect.
    Reached {
        /// The stacks before the instruction.
        entry: StackState,
        /// Where Pr was last loaded on the path, before the instruction:
        /// what the simulation passed its resolver.
        pr: Option<PrSource>,
        /// What it pops and pushes.
        effect: ResolvedEffect,
    },
    /// Reached, but the path stops here.
    Cut {
        /// The stacks before the instruction.
        entry: StackState,
        /// Where Pr was last loaded on the path, before the instruction.
        pr: Option<PrSource>,
        /// Why.
        reason: Cut,
    },
}

/// Two paths reaching an instruction with different values on the stacks.
#[derive(Debug, Clone)]
pub struct MergeConflict {
    /// The index of the instruction.
    pub index: usize,
    /// The stacks the first path brought.
    pub first: StackState,
    /// The stacks another path brought.
    pub other: StackState,
}

/// The stacks of one procedure, per instruction.
#[derive(Debug, Clone)]
pub struct ProcedureStack {
    /// Per instruction, in the order of the instructions simulated.
    pub instructions: Vec<InstructionStack>,
    /// Paths that disagree where they meet.
    pub merge_conflicts: Vec<MergeConflict>,
    /// The instructions paths reach with different Pr sources, by index; the
    /// record keeps the first path's.
    pub pr_conflicts: Vec<usize>,
    /// The instructions reached at more than one `GoSub` depth: a `GoSub`
    /// body entered from the main path and from another body (a nested
    /// `GoSub`), by index.
    ///
    /// Such code is simulated at each depth. The states differ only below
    /// the innermost `GoSub` return position: the statement values above it
    /// ([`StackState::statement_values`]) and the x87 stack are the same at
    /// every depth, or the instruction is a [`MergeConflict`]. The body's
    /// code therefore reads the same however deep it was entered, and its
    /// record in [`instructions`](Self::instructions) is the first depth's.
    pub shared_gosub_code: Vec<usize>,
}

impl ProcedureStack {
    /// Simulates a procedure: `instructions` are its decoded instructions in
    /// order, `code` its P-Code (for `On ... GoTo` jump tables), `resolve`
    /// gives the signature of a call instruction from the instruction and
    /// where the path last loaded Pr (for instance
    /// [`CallResolver::resolve_with_pr`](super::calltarget::CallResolver::resolve_with_pr)
    /// with the procedure's object). Pr's source follows each path; where
    /// paths meet, the first one's is kept.
    ///
    /// The paths start at the first instruction and at each `On Error`
    /// handler, with empty stacks. A shared `GoSub` body is simulated once
    /// per `GoSub` depth; where an instruction is reached more than once at
    /// one depth, the first path's state is kept.
    pub fn simulate<'s>(
        instructions: &[Instruction],
        code: &[u8],
        resolve: &dyn Fn(&Instruction, Option<&PrSource>) -> Option<CallSignature<'s>>,
    ) -> Self {
        let at: HashMap<u16, usize> = instructions
            .iter()
            .enumerate()
            .map(|(index, insn)| (insn.offset, index))
            .collect();
        let mut result: Vec<InstructionStack> = instructions
            .iter()
            .map(|_| InstructionStack::Unreached)
            .collect();
        let mut merge_conflicts = Vec::new();
        let mut seen: HashMap<(usize, usize), StackState> = HashMap::new();
        // The first state per instruction, at whichever GoSub depth.
        let mut statement: HashMap<usize, StackState> = HashMap::new();
        let mut first_pr: HashMap<usize, Option<PrSource>> = HashMap::new();
        let mut pr_conflicts = Vec::new();
        let mut work: Vec<(usize, StackState, Option<PrSource>)> =
            vec![(0, StackState::default(), None)];
        for insn in instructions {
            if matches!(insn.info.semantics, OpcodeSemantics::OnError) {
                work.extend(
                    insn.jump_targets(code)
                        .filter_map(|target| at.get(&target))
                        .map(|&index| (index, StackState::default(), None)),
                );
            }
        }

        while let Some((index, state, pr)) = work.pop() {
            let Some(insn) = instructions.get(index) else {
                continue;
            };
            match first_pr.get(&index) {
                Some(first) if *first != pr => pr_conflicts.push(index),
                Some(_) => {}
                None => {
                    first_pr.insert(index, pr);
                }
            }
            let key = (index, state.gosub_depth());
            if let Some(first) = seen.get(&key) {
                if *first != state {
                    merge_conflicts.push(MergeConflict {
                        index,
                        first: first.clone(),
                        other: state,
                    });
                }
                continue;
            }
            // A GoSub body reached at another depth must hold the same
            // statement values: its code then reads the same at each.
            match statement.get(&index) {
                Some(first)
                    if first.statement_values() != state.statement_values()
                        || first.x87 != state.x87 =>
                {
                    merge_conflicts.push(MergeConflict {
                        index,
                        first: first.clone(),
                        other: state.clone(),
                    });
                }
                Some(_) => {}
                None => {
                    statement.insert(index, state.clone());
                }
            }
            seen.insert(key, state.clone());
            let call = resolve(insn, pr.as_ref());
            let effect = insn.stack_effect(call.as_ref());
            let pr_before = pr;
            let pr = effect.pr.or(pr);
            let next_fpu_pops = instructions
                .get(index.saturating_add(1))
                .map_or(0, |next| next.info.fpu_pops);
            let semantics = insn.info.semantics;
            let mut after = state.clone();
            let resolved = match Self::step(&mut after, &effect, semantics, next_fpu_pops) {
                Ok(resolved) => resolved,
                Err(reason) => {
                    let reason = match reason {
                        Cut::UnknownCallee(_) => Cut::UnknownCallee(call.map(|c| c.callee)),
                        other => other,
                    };
                    Self::record(
                        &mut result,
                        index,
                        InstructionStack::Cut {
                            entry: state,
                            pr: pr_before,
                            reason,
                        },
                    );
                    continue;
                }
            };
            let gosub = matches!(semantics, OpcodeSemantics::GoSub);
            Self::record(
                &mut result,
                index,
                InstructionStack::Reached {
                    entry: state,
                    pr: pr_before,
                    effect: resolved,
                },
            );
            if matches!(
                semantics,
                OpcodeSemantics::Return | OpcodeSemantics::Resume | OpcodeSemantics::GoSubReturn
            ) {
                continue;
            }
            // The jump targets (an On Error handler is an entry of its own),
            // then the next instruction unless control never falls through.
            if !matches!(semantics, OpcodeSemantics::OnError) {
                let mut into = after.clone();
                if gosub {
                    into.eval.push(StackValue::GoSubReturn);
                }
                work.extend(
                    insn.jump_targets(code)
                        .filter_map(|target| at.get(&target))
                        .map(|&target| (target, into.clone(), pr)),
                );
            }
            if !insn.info.is_terminator() {
                work.push((index.saturating_add(1), after, pr));
            }
        }
        let mut depths: HashMap<usize, usize> = HashMap::new();
        for &(index, _) in seen.keys() {
            let count = depths.entry(index).or_default();
            *count = count.saturating_add(1);
        }
        let mut shared_gosub_code: Vec<usize> = depths
            .into_iter()
            .filter(|&(_, count)| count > 1)
            .map(|(index, _)| index)
            .collect();
        shared_gosub_code.sort_unstable();
        pr_conflicts.sort_unstable();
        pr_conflicts.dedup();
        Self {
            instructions: result,
            merge_conflicts,
            pr_conflicts,
            shared_gosub_code,
        }
    }

    /// Keeps the first record of an instruction reached at several `GoSub`
    /// depths.
    fn record(result: &mut [InstructionStack], index: usize, record: InstructionStack) {
        if let Some(slot) = result.get_mut(index)
            && matches!(slot, InstructionStack::Unreached)
        {
            *slot = record;
        }
    }

    /// Applies one effect to `state`, resolving its values.
    fn step(
        state: &mut StackState,
        effect: &StackEffect,
        semantics: OpcodeSemantics,
        next_fpu_pops: u8,
    ) -> Result<ResolvedEffect, Cut> {
        let mut popped = Vec::with_capacity(effect.popped.len());
        if matches!(semantics, OpcodeSemantics::GoSubReturn) {
            return match state.eval.pop() {
                Some(StackValue::GoSubReturn) => Ok(ResolvedEffect {
                    popped: vec![StackValue::GoSubReturn],
                    pushed: None,
                    x87_inferred: false,
                }),
                _ => Err(Cut::Pop(PopFailure::NoReturnPosition)),
            };
        }
        if effect.popped.contains(&Pop::Unknown) {
            return Err(Cut::UnknownCallee(None));
        }
        for pop in &effect.popped {
            match *pop {
                Pop::Eval(width) => popped.push(state.pop_eval(width).map_err(Cut::Pop)?),
                Pop::X87 => {
                    state.x87 = state
                        .x87
                        .checked_sub(1)
                        .ok_or(Cut::Pop(PopFailure::Empty))?;
                    popped.push(StackValue::X87);
                }
                Pop::Slots(slots) => {
                    let mut left = u32::from(slots);
                    while left > 0 {
                        match state.eval.pop() {
                            Some(StackValue::Eval(width)) if u32::from(width) <= left => {
                                left = left.saturating_sub(u32::from(width));
                                popped.push(StackValue::Eval(width));
                            }
                            Some(StackValue::Eval(_)) => {
                                return Err(Cut::Pop(PopFailure::Split { slots }));
                            }
                            Some(found) => {
                                return Err(Cut::Pop(PopFailure::Width { expected: 1, found }));
                            }
                            None => return Err(Cut::Pop(PopFailure::Empty)),
                        }
                    }
                }
                Pop::Unknown => return Err(Cut::UnknownCallee(None)),
            }
        }
        let mut x87_inferred = false;
        let pushed = match effect.pushed {
            _ if matches!(semantics, OpcodeSemantics::GoSub) => Some(StackValue::GoSubReturn),
            Some(Push::Eval(width)) => {
                state.eval.push(StackValue::Eval(width));
                Some(StackValue::Eval(width))
            }
            Some(Push::X87) => {
                state.x87 = state.x87.saturating_add(1);
                Some(StackValue::X87)
            }
            Some(Push::MaybeX87) => {
                x87_inferred = true;
                if next_fpu_pops > state.x87 {
                    state.x87 = state.x87.saturating_add(1);
                    Some(StackValue::X87)
                } else {
                    None
                }
            }
            None => None,
        };
        Ok(ResolvedEffect {
            popped,
            pushed,
            x87_inferred,
        })
    }

    /// Returns the instructions where a path stopped, with the reason.
    pub fn cuts(&self) -> impl Iterator<Item = (usize, &Cut)> {
        self.instructions
            .iter()
            .enumerate()
            .filter_map(|(index, record)| match record {
                InstructionStack::Cut { reason, .. } => Some((index, reason)),
                _ => None,
            })
    }

    /// Returns `true` if every instruction some path reaches was simulated
    /// with no cut and no conflict.
    pub fn is_complete(&self) -> bool {
        self.cuts().next().is_none() && self.merge_conflicts.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pcode::decoder::InstructionIterator;

    fn simulate(code: &[u8]) -> (Vec<Instruction>, ProcedureStack) {
        let instructions: Vec<Instruction> = InstructionIterator::new(code, code.len() as u16)
            .collect::<Result<_, _>>()
            .unwrap();
        let stack = ProcedureStack::simulate(&instructions, code, &|_, _| None);
        (instructions, stack)
    }

    #[test]
    fn test_literal_store() {
        // LitI4 1; FStR4 var_88; ExitProc
        let (_, stack) = simulate(&[0xF5, 1, 0, 0, 0, 0x71, 0x78, 0xFF, 0x14]);
        assert!(stack.is_complete());
        let InstructionStack::Reached { entry, effect, .. } = &stack.instructions[1] else {
            panic!("{:?}", stack.instructions[1]);
        };
        assert_eq!(entry.eval, vec![StackValue::Eval(1)]);
        assert_eq!(effect.popped, vec![StackValue::Eval(1)]);
        let InstructionStack::Reached { entry, .. } = &stack.instructions[2] else {
            panic!();
        };
        assert_eq!(entry, &StackState::default());
    }

    #[test]
    fn test_gosub_return_position() {
        // Gosub loc_0005; ExitProc; Return
        let (instructions, stack) = simulate(&[0xFD, 0x0A, 0x05, 0x00, 0x14, 0xFC, 0xC9]);
        assert_eq!(instructions[2].info.mnemonic, "Return");
        assert!(stack.is_complete(), "{stack:?}");
        let InstructionStack::Reached { entry, effect, .. } = &stack.instructions[2] else {
            panic!();
        };
        assert_eq!(entry.eval, vec![StackValue::GoSubReturn]);
        assert_eq!(entry.gosub_depth(), 1);
        assert!(entry.statement_values().is_empty());
        assert_eq!(effect.popped, vec![StackValue::GoSubReturn]);
        // Past the GoSub, the stack is as the GoSub found it.
        let InstructionStack::Reached { entry, .. } = &stack.instructions[1] else {
            panic!();
        };
        assert!(entry.eval.is_empty());
    }

    #[test]
    fn test_nested_gosub_reads_the_same_at_each_depth() {
        // Gosub loc_0009; Gosub loc_000B; ExitProc;
        // loc_0009: Return; loc_000B: Gosub loc_0009; Return
        let (instructions, stack) = simulate(&[
            0xFD, 0x0A, 0x09, 0x00, 0xFD, 0x0A, 0x0B, 0x00, 0x14, 0xFC, 0xC9, 0xFD, 0x0A, 0x09,
            0x00, 0xFC, 0xC9,
        ]);
        assert_eq!(instructions[3].offset, 9);
        assert!(stack.is_complete(), "{stack:?}");
        assert_eq!(stack.shared_gosub_code, vec![3]);
    }

    #[test]
    fn test_shared_gosub_body_with_other_statement_values_conflicts() {
        // Gosub loc_0011; LitI4 1; LitI4 1; Branch loc_0016;
        // loc_0011: LitI4 1; loc_0016: FStR4 var_88; Return
        // loc_0016 is reached in the body with one value and from the main
        // path with two.
        let (instructions, stack) = simulate(&[
            0xFD, 0x0A, 0x11, 0x00, 0xF5, 0x01, 0x00, 0x00, 0x00, 0xF5, 0x01, 0x00, 0x00, 0x00,
            0x1E, 0x16, 0x00, 0xF5, 0x01, 0x00, 0x00, 0x00, 0x71, 0x78, 0xFF, 0xFC, 0xC9,
        ]);
        assert_eq!(instructions[5].offset, 0x16);
        assert!(stack.shared_gosub_code.contains(&5), "{stack:?}");
        assert!(
            stack.merge_conflicts.iter().any(|c| c.index == 5),
            "{stack:?}"
        );
        assert!(!stack.is_complete());
    }

    #[test]
    fn test_pop_failure_cuts_the_path() {
        // AddI4 on an empty stack.
        let (_, stack) = simulate(&[0xAA, 0x14]);
        assert!(matches!(
            stack.instructions[0],
            InstructionStack::Cut {
                reason: Cut::Pop(PopFailure::Empty),
                ..
            }
        ));
        assert!(matches!(stack.instructions[1], InstructionStack::Unreached));
        assert!(!stack.is_complete());
    }
}
