//! The stack effects, checked against the compiled fixtures.
//!
//! Every procedure is simulated over its control flow
//! ([`ProcedureStack::simulate`]): each value pushed has the width its
//! producer states, each pop must find a value of the width the consumer
//! states, the paths that meet at a jump target must agree, and the stack
//! must be empty where a statement begins (`LargeBos`) and where the
//! procedure returns - except for the return positions of the `GoSub`s the
//! path is inside. A coverage census lists the opcodes the fixtures
//! exercise.

#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]

mod common;

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write,
};

use visualbasic::{
    VbProject,
    pcode::{
        calltarget::{CallResolver, CallSignature, SlotInterfaces},
        decoder::Instruction,
        semantics::OpcodeSemantics,
        stackeffect::PrSource,
        stacksim::{Cut, InstructionStack, ProcedureStack},
    },
    project::MethodEntry,
};

use crate::common::{fixture, projects};

/// What the simulation of all procedures found.
#[derive(Default)]
struct Report {
    procedures: usize,
    instructions: usize,
    /// Instructions some path reaches.
    reached: usize,
    /// Paths cut at a call of unknown arity, by mnemonic.
    unresolved: BTreeMap<String, usize>,
    /// x87 results of unknown callees, settled by the next instruction.
    inferred: usize,
    /// Procedures with code reached at more than one GoSub depth, and
    /// `OnGosub` sites.
    shared_gosub_procedures: usize,
    on_gosub_sites: usize,
    /// Instructions paths reach with different Pr sources.
    pr_conflicts: usize,
    /// Statement boundaries (`LargeBos`) reached, and those with values on
    /// the stack.
    boundaries: usize,
    boundary_failures: Vec<String>,
    /// Procedure exits reached, and those with values on a stack.
    exits: usize,
    exit_failures: Vec<String>,
    /// Pops that found no value, a value of another width, or split one.
    pop_failures: Vec<String>,
    /// Jump targets the paths reach with different stacks.
    merge_failures: Vec<String>,
}

impl Report {
    fn failures(&self) -> usize {
        self.boundary_failures.len()
            + self.exit_failures.len()
            + self.pop_failures.len()
            + self.merge_failures.len()
    }

    fn summary(&self) -> String {
        let mut text = String::new();
        let unresolved: usize = self.unresolved.values().sum();
        writeln!(
            text,
            "{} procedures, {} instructions ({} reached); {} statement boundaries, {} exits; \
             {} paths cut at calls of unknown arity {:?}; {} x87 results inferred; \
             {} procedures with GoSub code reached at two depths; {} OnGosub sites; \
             {} instructions reached with two Pr sources",
            self.procedures,
            self.instructions,
            self.reached,
            self.boundaries,
            self.exits,
            unresolved,
            self.unresolved,
            self.inferred,
            self.shared_gosub_procedures,
            self.on_gosub_sites,
            self.pr_conflicts
        )
        .unwrap();
        for (what, list) in [
            ("pop", &self.pop_failures),
            ("merge", &self.merge_failures),
            ("boundary", &self.boundary_failures),
            ("exit", &self.exit_failures),
        ] {
            writeln!(text, "{what} failures: {}", list.len()).unwrap();
            for line in list.iter().take(40) {
                writeln!(text, "  {line}").unwrap();
            }
        }
        text
    }

    /// Checks one simulated procedure.
    fn check(&mut self, label: &str, instructions: &[Instruction], stack: &ProcedureStack) {
        self.procedures += 1;
        self.instructions += instructions.len();
        self.shared_gosub_procedures += usize::from(!stack.shared_gosub_code.is_empty());
        self.pr_conflicts += stack.pr_conflicts.len();
        self.on_gosub_sites += instructions
            .iter()
            .filter(|i| {
                matches!(i.info.semantics, OpcodeSemantics::GoSub)
                    && i.operands.iter().flatten().any(|o| {
                        matches!(o, visualbasic::pcode::operand::Operand::JumpTable { .. })
                    })
            })
            .count();
        for conflict in &stack.merge_conflicts {
            let insn = &instructions[conflict.index];
            self.merge_failures.push(format!(
                "{label} {:04x} {}: {:?} vs {:?}",
                insn.offset, insn.info.mnemonic, conflict.first, conflict.other
            ));
        }
        for (insn, record) in instructions.iter().zip(&stack.instructions) {
            let here = format!("{label} {:04x} {}", insn.offset, insn.info.mnemonic);
            match record {
                InstructionStack::Unreached => {}
                InstructionStack::Cut { reason, .. } => {
                    self.reached += 1;
                    match reason {
                        Cut::UnknownCallee(_) => {
                            *self
                                .unresolved
                                .entry(insn.info.mnemonic.to_string())
                                .or_default() += 1;
                        }
                        Cut::Pop(failure) => self.pop_failures.push(format!("{here}: {failure:?}")),
                    }
                }
                InstructionStack::Reached { entry, effect, .. } => {
                    self.reached += 1;
                    self.inferred += usize::from(effect.x87_inferred);
                    if insn.is_bos() {
                        self.boundaries += 1;
                        if !entry.statement_values().is_empty() {
                            self.boundary_failures.push(format!("{here}: {entry:?}"));
                        }
                    }
                    // ExitProcR4 / ExitProcR8 load the result from the frame.
                    if matches!(insn.info.semantics, OpcodeSemantics::Return) {
                        self.exits += 1;
                        if !entry.eval.is_empty() || entry.x87 != 0 {
                            self.exit_failures.push(format!("{here}: {entry:?}"));
                        }
                    }
                }
            }
        }
    }
}

/// Replaces the signature the resolver found for the call at an
/// instruction of a procedure (by its label).
type Perturb =
    dyn Fn(&str, &Instruction, Option<CallSignature<'static>>) -> Option<CallSignature<'static>>;

/// Simulates every procedure of every fixture; `perturb` may replace the
/// signature of a call (the control).
fn simulate_all(perturb: &Perturb) -> Report {
    let mut report = Report::default();
    for name in &projects() {
        let bytes: &'static [u8] =
            Box::leak(std::fs::read(fixture(name)).unwrap().into_boxed_slice());
        let project: &'static VbProject<'static> =
            Box::leak(Box::new(VbProject::from_bytes(bytes).unwrap()));
        let resolver = CallResolver::new(project).unwrap();
        for (object_index, object) in project.objects().unwrap().enumerate() {
            let object = object.unwrap();
            let object_index = u16::try_from(object_index).unwrap();
            let object_name = object.name().unwrap_or_default().to_string();
            for (index, entry) in object.methods().unwrap().enumerate() {
                let Ok(MethodEntry::PCode(method)) = entry else {
                    continue;
                };
                let label = format!("{name} {object_name}#{index}");
                let instructions: Vec<Instruction> = method
                    .instructions()
                    .unwrap()
                    .collect::<Result<_, _>>()
                    .unwrap();
                // The frame slots' interfaces from a first simulation, then
                // the simulation that resolves through them.
                let first =
                    ProcedureStack::simulate(&instructions, method.pcode_bytes(), &|insn, pr| {
                        resolver.resolve_with_pr(object_index, insn, pr)
                    });
                let slots = SlotInterfaces::infer(&resolver, object_index, &instructions, &first);
                let resolve = |insn: &Instruction, pr: Option<&PrSource>| {
                    perturb(
                        &label,
                        insn,
                        resolver.resolve_with_slots(object_index, insn, pr, &slots),
                    )
                };
                let stack = ProcedureStack::simulate(&instructions, method.pcode_bytes(), &resolve);
                report.check(&label, &instructions, &stack);
            }
        }
    }
    report
}

/// **The hypothesis**: with the table's effects and the resolver's call
/// signatures, every pop finds values of the widths it expects, every jump
/// target is reached with one stack, and the evaluation stack is empty at
/// every statement boundary and every exit, but for the `GoSub` return
/// positions.
#[test]
fn stack_balances_over_every_procedure() {
    let report = simulate_all(&|_, _, call| call);
    println!("{}", report.summary());
    assert!(report.procedures > 100, "{}", report.summary());
    assert_eq!(report.failures(), 0, "{}", report.summary());
}

/// **The control**: one call whose arity is known, given one argument too
/// few, must break the balance.
#[test]
fn a_wrong_arity_breaks_the_balance() {
    let report = simulate_all(&|label, insn, call| {
        let mut call = call?;
        if label == "types Program#3" && insn.offset == 0x000F {
            let widths = call.arg_widths.as_mut()?;
            widths.pop();
            call.arg_slots = Some(widths.iter().map(|&w| u16::from(w)).sum());
        }
        Some(call)
    });
    assert!(
        report
            .exit_failures
            .iter()
            .any(|f| f.starts_with("types Program#3 ")),
        "{}",
        report.summary()
    );
}

/// **The coverage census**: how many times each opcode occurs across the
/// fixtures.
#[test]
fn coverage_census() {
    let mut counts: BTreeMap<(u8, u8), (usize, &'static str)> = BTreeMap::new();
    for name in &projects() {
        let bytes = std::fs::read(fixture(name)).unwrap();
        let project = VbProject::from_bytes(&bytes).unwrap();
        for object in project.objects().unwrap() {
            for entry in object.unwrap().methods().unwrap() {
                let Ok(MethodEntry::PCode(method)) = entry else {
                    continue;
                };
                for insn in method.instructions().unwrap().flatten() {
                    let entry = counts
                        .entry((insn.info.table as u8, insn.info.index))
                        .or_insert((0, insn.info.mnemonic));
                    entry.0 += 1;
                }
            }
        }
    }
    let tables: BTreeSet<u8> = counts.keys().map(|&(t, _)| t).collect();
    let mut text = String::new();
    for table in tables {
        let row: Vec<String> = counts
            .iter()
            .filter(|((t, _), _)| *t == table)
            .map(|((_, op), (n, m))| format!("{op:02x} {m} {n}"))
            .collect();
        writeln!(text, "table {table}: {}", row.join(", ")).unwrap();
    }
    println!("{} distinct opcodes\n{text}", counts.len());
    assert!(
        counts.len() >= 600,
        "the fixtures cover {} opcodes",
        counts.len()
    );
}
