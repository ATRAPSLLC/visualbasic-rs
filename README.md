# visualbasic

Parse and inspect Visual Basic 6 compiled binaries.

This crate provides typed access to all internal structures within a VB6
compiled executable, from the PE entry point down to individual P-Code
bytecode instructions.

## Quick start

```rust
use visualbasic::{VbProject, RecognitionFailure};

let file_bytes = std::fs::read("sample.exe")?;

let project = match VbProject::from_bytes(&file_bytes) {
    Ok(p) => p,
    Err(e) => match e.recognition_failure() {
        Some(RecognitionFailure::NotRecognized | RecognitionFailure::UnrecognizedFormat) => {
            // Quietly skip non-VB6 files.
            return Ok(());
        }
        Some(RecognitionFailure::TruncatedContainer) => {
            eprintln!("warn: looks VB6 but truncated: {e}");
            return Ok(());
        }
        _ => return Err(e.into()),
    },
};

println!("Project: {}", project.project_name()?);
println!("Objects: {}", project.object_count()?);

for obj in project.objects()? {
    let obj = obj?;
    println!("  {} ({})", obj.name()?, obj.object_kind()?);

    for method in obj.pcode_methods()? {
        let method = method?;
        for insn in method.instructions()? {
            println!("    {}", insn?);
        }
    }
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

## What it parses

- **PE entry point** detection (EXE push-stub and DLL export patterns)
- **VBHeader**, **ProjectData**, **ObjectTable** and the full structure chain
- **PublicObjectDescriptor**, **ObjectInfo**, **OptionalObjectInfo**, **PrivateObjectDescriptor**
- **P-Code bytecode**: opcode tables, operand decoding, streaming instruction
  iterator; each opcode's effect on the evaluation and x87 stacks and on Pr,
  the object register, read from the runtime's handlers
- **Controls**: ControlInfo, event sink vtables, event handler thunks
- **COM metadata**: GUIDs, TypeLib registration, external component tables
- **Form binary data**: control trees, property streams, font/picture resources
- **MSVBVM60.DLL exports**: every export by name and ordinal, with signatures
  for most, so the runtime functions P-Code imports by ordinal are named
- **Imports**: the PE import table by import address table slot

## High-level walkers

For consumer code that wants a single tagged stream rather than walking
each substructure by hand:

- [`VbProject::code_entrypoints()`] - every code VA in the project
  (P-Code stubs, native procs, native thunks, event handlers, `Sub Main`)
  in one `Vec<CodeEntrypoint>`.
- [`VbObject::events()`] - joined `(control, event_slot, handler_va)`
  bindings for a form, with per-control-type event-name resolution.
- [`VbProject::gui_entries_with_form_data()`] - pairs each GUI table
  entry with its parsed form binary in one iterator.
- [`VbProject::compilation_mode()`] - distinguishes `Pcode` / `Native` /
  `Mixed` binaries (combines the project flag with a per-object scan).
- [`VbProject::diagnostics()`] - eager parse-health probe surfacing
  missing optional structures and known-anomaly patterns.

## Analysing P-Code

The `pcode` module documents the interpreter (dispatch, the two stacks, Pr,
the frame, calling conventions) and describes each instruction's runtime
effect, as the handlers of `MSVBVM60.DLL` implement it:

```rust,no_run
use visualbasic::{
    VbProject,
    pcode::{calltarget::CallResolver, decoder::Instruction, stacksim::ProcedureStack},
    project::MethodEntry,
};

# fn main() -> Result<(), Box<dyn std::error::Error>> {
let bytes = std::fs::read("sample.exe")?;
let project = VbProject::from_bytes(&bytes)?;
let calls = CallResolver::new(&project)?;
for (object_index, object) in project.objects()?.enumerate() {
    let object_index = u16::try_from(object_index)?;
    for entry in object?.methods()? {
        let Ok(MethodEntry::PCode(method)) = entry else { continue };
        let code: Vec<Instruction> = method.instructions()?.collect::<Result<_, _>>()?;
        // Each instruction's effect: values popped (last operand first, from
        // both stacks), the value pushed, the receiver, Pr's source.
        for insn in &code {
            let effect = insn.stack_effect(calls.resolve(object_index, insn).as_ref());
            println!("{insn}  {effect:?}");
        }
        // The stacks over the procedure's control flow, every value
        // resolved to the width its producer pushed.
        let stack = ProcedureStack::simulate(&code, method.pcode_bytes(), &|insn, pr| {
            calls.resolve_with_pr(object_index, insn, pr)
        });
        for (index, cut) in stack.cuts() {
            println!("path cut at {:04x}: {cut:?}", code[index].offset);
        }
    }
}
# Ok(())
# }
```

- [`CallResolver`] gives each call's callee (a project procedure, a
  `Declare` function, an imported runtime function, an external interface by
  IID and vtable offset, a late-bound member) and signature.
- [`FrameResolver`] names `%a` frame offsets: arguments, locals, the
  interpreter's slots.
- [`ConstantPool`] reads pool entries by the index `%s` / `%c` operands
  carry.

## Cargo features

| Feature | Default | Effect |
|---|---|---|
| `tracing` | off | Emits structured `tracing::warn!` events at silent fail-soft sites. No effect when disabled - the helpers compile to no-ops. |

## Example tool

The included `dump` example produces an ildasm-style text dump of a VB6 executable:

```sh
cargo run --example dump -- path/to/sample.exe
```

## Disclaimer

The VB6 compiled binary format was never officially or publicly documented by
Microsoft. All structure layouts, field semantics, and P-Code opcode definitions
in this crate have been reverse engineered from MSVBVM60.DLL (6.00.9848) and
VB6.EXE (6.00.8176) by humans and AI. While the results have been cross-verified
against runtime behavior, errors and inaccuracies are possible.

## License

Copyright 2026 ATRAPS LLC. Licensed under the Apache License,
Version 2.0. See `LICENSE` and `NOTICE`.
