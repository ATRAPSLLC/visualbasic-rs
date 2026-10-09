//! The compiled fixtures (`tests/fixtures`), read whole: every procedure of
//! every project decodes, instruction by instruction, to exactly the end of
//! its P-code.

#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]

mod common;

use std::collections::BTreeSet;

use visualbasic::{VbProject, project::MethodEntry};

use crate::common::{fixture, projects};

/// **Every procedure decodes to the end of its P-code, and no further.** An
/// opcode the table sizes wrongly either fails to decode or carries the decoder
/// past the procedure's end, so the instructions' lengths summing to the
/// P-code's length - less the padding to a multiple of four bytes after the
/// final terminator - is what says the stream stayed in sync.
#[test]
fn every_procedure_decodes_to_its_end() {
    let mut procedures = 0usize;
    for name in &projects() {
        let bytes = std::fs::read(fixture(name)).expect("fixture readable");
        let project = VbProject::from_bytes(&bytes).expect("a VB6 project");
        for object in project.objects().expect("objects") {
            let object = object.expect("object");
            let object_name = object.name().unwrap_or_default().to_string();
            for (index, entry) in object.methods().expect("methods").enumerate() {
                let Ok(MethodEntry::PCode(method)) = entry else {
                    continue;
                };
                procedures += 1;
                let mut decoded = 0usize;
                let mut last_terminates = false;
                let mut starts = BTreeSet::new();
                let mut targets = BTreeSet::new();
                for instruction in method.instructions().expect("instructions") {
                    let instruction = instruction.unwrap_or_else(|error| {
                        panic!("{name} {object_name}#{index}: decode error after {decoded} bytes: {error}")
                    });
                    assert_eq!(
                        usize::from(instruction.offset),
                        decoded,
                        "{name} {object_name}#{index}: an instruction starts where the last ended"
                    );
                    assert_ne!(
                        instruction.info.mnemonic, "InvalidExcode",
                        "{name} {object_name}#{index}: an invalid opcode at {:#06x}",
                        instruction.offset
                    );
                    starts.insert(instruction.offset);
                    targets.extend(instruction.jump_targets(method.pcode_bytes()));
                    decoded += usize::from(instruction.raw_len);
                    last_terminates = instruction.info.is_terminator();
                }
                let stray: Vec<u16> = targets.difference(&starts).copied().collect();
                assert!(
                    stray.is_empty(),
                    "{name} {object_name}#{index}: jumps into no instruction: {stray:04x?}"
                );
                // The code ends on a terminator, and the compiler pads it to a
                // multiple of four bytes - usually with zeros, but not always.
                let pcode = method.pcode_bytes();
                assert!(
                    last_terminates,
                    "{name} {object_name}#{index}: the code ends on a terminator"
                );
                assert!(
                    pcode.len().saturating_sub(decoded) < 4,
                    "{name} {object_name}#{index}: decoded {decoded} of {} bytes",
                    pcode.len()
                );
            }
        }
    }
    assert!(
        procedures > 20,
        "the fixtures hold their procedures: {procedures}"
    );
}
