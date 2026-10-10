# Changelog

All notable changes to this crate are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.5.0] - 2026-10-09

Every name of the ProjectInfo2 name area now has an owner: the private
object descriptor's +0x20 and +0x24 arrays, misread as a method name table
and a parameter name table, describe an object's public variables and
implemented interfaces, and its events' prototypes.

### Added

- **Structure extents** (`extents`): `VbProject::structure_extents`
  states the extent of every structure the crate reads (`StructureExtent`,
  `StructureKind`), each measured by the walk that reads it: the headers
  and tables, each object's descriptors, method, method link, GUID and
  control tables, event sinks and DISPID maps, prototypes, members, record
  layouts, constant pools and the data their entries name, procedure
  descriptors with their line tables, P-Code, the external and component
  tables, form data, COM registration data, the code markers, and native
  procedures' unwind records with their tables. Native stubs and procedures
  are never an extent.
- **Pool entry roles**: `Instruction::pool_references` lists every constant
  pool entry an instruction names with what its opcode says it holds
  (`PoolReference`, `PoolEntryRole`).
- **Pool descriptors** (`vb::pooldesc`): `RecordIoDescriptor` (the
  recursive descriptor `Get #` and `Put #` of a record read, with
  `RecordIoEntry`, `RecordIoKind`), `IoItems` (the item list of `Print`,
  `Write` and `Input`, with `IoItem`, `IoSeparator`), `ArrayDescriptor`
  (the `SAFEARRAY` header of a fixed array in a record) and
  `CreationDescriptor` (what `New` creates when it is not a project class).
- **Record layouts of an object's `Type`s**: `VbObject::record_layouts`.
- **DISPID maps**: `ControlInfo::{dispid_map, dispid_map_count,
  dispid_map_va}` read the `{source DISPID, handler MEMID}` pairs
  (`DispidMapping`) of a `WithEvents` variable's or an `Implements`' entry;
  `ControlInfo::sink_slots` gives its event sink vtable's slot count.
- `MethodLinkKind::Variable::pushed`: the pool index and pool VA a `New`
  variable's accessor pushes.
- `VbObject::interface_iid_va`: the IID the compiler writes before an
  object's CLSID.
- `VbHeader::reserved_guid`, `TypeLibRef::flags`, `RecordLayout::ansi_size`,
  `ExternalDeclareInfo::BLOCK_SIZE`, `FuncTypDesc::size`, `MemberDesc::size`,
  `EventSinkVtable::size`, `ProcDscInfo::extent_size` (with the line-number
  table), `ProjectInfo2Iter::position`.
- Fixtures `records` (record I/O, arrays in records, record arrays, `For
  Each` into a typed variable) and `udts` (`Public` and `Private` `Type`s of
  classes).
- **Member descriptors** (`vb::member`): `MemberDesc` reads a public
  variable's or an `Implements`' name, DISPID, accessor vtable offset,
  instance offset and type (with the class's or interface's `ObjectInfo`
  VA); `MemberKind` tells variables, `WithEvents` variables and
  `Implements` apart. `VbObject::members` walks an object's array, so its
  public variables and their count are recoverable.
- **Event prototypes**: `VbObject::event_type_descs` yields the
  `FuncTypDesc` of each `Event` declaration, with its parameter names.
- **Name ownership**: `VbProject::name_references` lists every pointer to a
  parameter, event parameter, variable or interface name with its owner
  (`NameOwner`), including the names past where the ProjectInfo2 walk
  stops.
- **Control kinds**: `ControlInfo::kind` (`ControlKind::{Control,
  WithEvents, Implements}`, from the flags at +0x00, documented as always
  0x0040 before), `ControlInfo::{dispatch_slots, event_count}`;
  `EventSinkVtable::{dispatch_va, dispatch_slots}` read a dual sink's
  `IDispatch` slots.
- **Type references** (`vb::typeref`): `TypeLibRef` (a type library's GUID,
  version, LCID, path and name), `InterfaceRef` (an interface's library and
  IID) and `RecordRef` (a user-defined type's library, GUID, version and
  LCID). `ArgType::{interface, record}` read the descriptor of an interface
  (codes 0x1C, 0x1D) or record (0x14) type; `ArgType::{IUNKNOWN,
  INTERFACE}` name the interface codes.
- **UDT layouts and array element types** (`vb::controlprop`):
  `RecordLayout` reads a user-defined type's size and the members that need
  init or cleanup; `ControlPropertyEntry::{record_layout_va,
  element_layout_va, record_layout}` name it from a UDT member or an array
  of UDTs, `element_type` and `safearray_{offset, features, iid_va,
  vartype}` read an array's element type, inline `SAFEARRAY` descriptor and
  the IID or `VARTYPE` after it.
- `ProjectInfo2::object_descs` reads the per-object PrivateObjectDescriptor
  array.
- `VbProject::export_stubs` decodes an ActiveX DLL's or OCX's COM export
  stubs (`entrypoint::ExportStub`): the VBHeader and `.data` VAs each
  pushes and the `VBDll*` runtime function it jumps to.
- **Native unwind records** (`vb::native`): `ProcUnwindInfo` reads the
  record a native procedure's prologue stores in its frame, in all four
  sizes: the addresses of the code that releases its `Me`, its variables
  and its statement's temporaries when it unwinds, and of its error
  handling's tables, `ErrorHandlerTable` (each `On Error GoTo` label,
  `ErrorHandler`), `ResumeTable` (each statement's address, for `Resume`
  and `Resume Next`) and `LineNumberTable` (each statement's line number,
  for `Erl`). `ProcUnwindInfo::from_prologue` finds the record from the
  procedure's start, `ProcUnwindInfo::code_vas` lists the code it names,
  and `VbProject::unwind_records` (`ProcedureUnwind::scan`) finds every
  procedure that stores one, including those no table names.
- **Native code**: `VbProject::native_entries` lists every address of
  native code the project's structures name (the entry point and its stub
  target, export stubs, entry stubs, method link thunks and targets, event
  sink thunks and handlers, pool stubs, dual-entry partners, the procedures
  adjustor thunks enter and every procedure that stores an unwind record);
  `VbProject::native_code_range` gives the region between the code markers
  `ProjectData::{code_start_va, code_end_va}` name, empty for a P-Code
  build; `VbProject::entry_point_va` and `entrypoint::entry_stub_target`
  decode the PE entry point's stub (an executable's `push imm32; call` and
  a DLL's `pop edx; push imm32; push imm32; push edx; jmp`).
  `NativeEventThunk::JUMP_OFFSET` is the bare jump of a native adjustor
  thunk.
- `ExportSignature::noreturn` (a sixth column of the export table): the
  runtime functions that never return to their caller (`__vbaError`,
  `__vbaErrorOverflow`, `__vbaGenerateBoundsError`, `__vbaFailedFriend`,
  `__vbaFPException`, `__vbaEnd`, `__vbaStopExe`, `rtcAppleScript`,
  `ThunRTMain`).
- Fixture `errors-native`: error handling compiled to native code, each
  procedure varying one table of its unwind record.
- `CodeEntry::target_va`: a native build's event handler stub enters the
  procedure at this VA (a native class has no method table to index).
- **Event names**: `VbObject::event_names` names each `Event` declaration
  from the project's type library (`VbProject::type_library_bytes`, the
  `TYPELIB` resource of an ActiveX DLL, OCX or EXE), matching the event's
  DISPID in the events dispinterface. A Standard EXE has no type library:
  its event names are pooled strings no structure points to, and stay
  unnamed.
- Fixture `eventnames`: classes sharing member and event name spellings.
- Fixture `typerefs`: an ActiveX DLL with interface- and UDT-typed members,
  parameters and return values, and `WithEvents` variables of a project
  class and of a `TextBox`.
- **Late-bound calls by DISPID resolve**: `Callee::Late` carries the
  member its DISPID names (`LateTarget`) when the receiver's class is
  known: a project UserControl's procedure, a hosted ActiveX control's
  member, or the control extender's (`_VBControlExtender`, whose members
  the compiler calls 0x3000 below the DISPIDs it declares,
  `RuntimeInterfaces::{CONTROL_EXTENDER, extender_member}`).
  `InterfaceCatalog::dispatched` describes a member by DISPID (empty by
  default); `InvokeKind::{of_late_call, admits}` tell how a late-bound
  opcode invokes its member, `InvokeKind::accessor_prefix` the name COM
  gives the accessor.
- **Hosted controls name their class**: `FormControlRecord::{prog_id,
  prog_id_bytes}` read the ProgID a hosted control's record begins with,
  with or without an event handler; a form's getter for the control
  returns its class's default interface
  (`CallResolver::returned_interface`), and the getter of a control array
  of one its elements' (`CallResolver::returned_elements`), which
  `SlotInterfaces` gives to the slot the array's `Item`
  (`RuntimeInterfaces::CONTROL_ARRAY_ITEM`) writes.

### Changed

- `PrivateObjectDescriptor::{var_stub_count, var_stubs_va}` are now
  `record_layout_count` and `record_layouts_va`: the array holds the record
  layouts of the object's `Type` declarations.
- `ControlInfo::{dispid_count_or_zero, dispid_table_va}` are now
  `dispid_map_count` (the u16 at +0x10) and `dispid_map_va`: the map is on
  disk, not filled at runtime.
- `VbHeader::SIZE` is 0x78 and `TypeLibRef::SIZE` 0x28, the sizes the
  compiler writes.
- `ControlPropertyEntry::{record_layout_va, element_layout_va}` return
  `None` for 0 and 0xFFFFFFFF, which a UDT with no member to initialize or
  release holds.
- Depends on `msft-typelib` 0.2.0 to read the embedded type library.
- `PrivateObjectDescriptor::method_name_table_va` is now `member_descs_va`
  and `param_names_va` is now `event_descs_va`.
- `read_name_strings` documents what its strings are (parameter, event
  parameter, variable and interface names) and where its walk stops.
- `EventSinkVtable::parse` takes the entry's `ControlInfo`.
- `InterfaceMetadata` is now `vb::typeref::TypeLibRef`, with
  `ControlTypeEntry::typelib_va` and `typelib` for `interface_metadata_va`
  and `interface_metadata`; its fields at +0x08 and +0x0C, documented as
  always 6 and 9, are the library's version and LCID (`MSMask` 1.1, LCID 0).
- `ArgType::object_va` is now `descriptor_va`: for an interface or record
  type it is not an object.
- `FuncTypDesc::dispid` returns the whole DISPID as an `i32`, the four
  bytes at +0x0C (0x60030000 plus the index for a Sub or Function,
  0x68030000 for a property procedure, an event's number from 1), which
  is what a late-bound caller names the member by; it read only the low
  word.

### Removed

- `vb::varstub` (`VarStubDesc`, `VarStubIter`) and `VbObject::{var_stubs,
  var_stub_count}`: no structure of that format exists; the array they read
  is the record layouts of the object's `Type`s.
- `InterfaceMetadata::{dispatch_names, all_dispatch_names}`: the first read
  the library name, now `TypeLibRef::name`; the second scanned 4 KiB past
  it for identifier-like strings that belong to other structures.

### Fixed

- `EventSinkVtable` of an `Implements` entry spans its four trailing zero
  slots: every sink is `0x0C + 4 * (slots + 3)` bytes, `+ 7` with flag bit
  0x08.
- The event handlers of a `WithEvents` variable: its sink vtable is a dual
  interface's, with four `IDispatch` slots before the handlers, and
  `VbControl::event_handler_va`, `EventSinkVtable::handler_va`,
  `VbObject::events` and `VbObject::code_entries` read those slots as the
  first handlers (`Listener.m_Source` gave the `IDispatch` thunks for
  `Started`, `Ticked` and `Named`). An `Implements` sink has the same four
  slots, which its handler count included: they no longer appear as event
  handlers.

## [0.4.0] - 2026-10-08

P-Code analysis: the opcode table covers every handler of `MSVBVM60.DLL`
6.00.8176 and 6.00.9848, and new APIs give each instruction's stack effect,
call target and signature, a stack simulation over a procedure, and the
interfaces of the objects in its frame. Form data, hosted controls and many
structure readers were corrected against compiled fixtures.

### Added

- **Stack effects** (`pcode::stackeffect`, `pcode::movement`):
  `Instruction::{stack_effect, pr_source, movement, procedure_return,
  value_type, frame_slots, jump_targets}`: what an instruction pops and
  pushes on the evaluation and x87 stacks, where Pr comes from, what a load
  or store moves and its side effects, and how an `ExitProc*` returns.
- **Stack simulation** (`pcode::stacksim`): `ProcedureStack::simulate`
  gives the stacks at every instruction over the control flow, including
  `GoSub` depths (`shared_gosub_code`) and paths cut at unknown callees.
- **Call resolution** (`pcode::calltarget`): `CallResolver`, `CallSignature`
  and `Callee` name the callee and argument widths of every call opcode:
  project procedures through vtables built from method link tables (forms
  and UserControls after their designer interface and control getters),
  `Declare`s, runtime imports, control getters, variable accessors,
  `RaiseEvent`, late-bound members and external interfaces.
- **External interfaces**: `InterfaceCatalog`, `InterfaceMember` (with
  `returns_interface`) and `CallResolver::with_interfaces` let a host supply
  type library members; `RuntimeInterfaces` covers the runtime's own
  (control arrays, `Err`).
- **Object slot typing**: `SlotInterfaces`, `ProjectSlots` and
  `CallResolver::{resolve_with_slots, simulate_procedure,
  simulate_procedure_seeded, infer_project_slots, returned_interface}` infer
  the interface of the object in each frame slot from casts, typed calls,
  call results and arguments across procedures, so typed `VCall*` without an
  IID resolve.
- **Opcode table**: columns `stack`, `popped`, `object`, `movement` and
  per-build handler addresses; `OpcodeInfo::{popped, rule_at, fpu_callee,
  pr_load, stack, receiver, handler, value_type}`, `StackRule`, `Receiver`;
  `OpcodeSemantics::{GoSub, GoSubReturn, Resume, OnError, Raise, End}`;
  `CallKind::Event`, `ArgumentOrder`; `Operand::{JumpTable, FrameList}`.
- **Procedures**: `ProcDscInfo::{zeroed_frame, line_numbers,
  line_table_offset}`, `PCodeMethod::error_handling`,
  `FrameResolver::{for_method, declarations}`, `FrameVar::{ReturnValue,
  HiddenResult}`, `FrameOwner`.
- **Constant pool and imports**: `ConstantPool::{entry_at, entries, va_at,
  string_at, name_at, guid_at, api_stub_at}` and `PoolEntry`;
  `imports::ImportTable` (`VbProject::imports`).
- **Objects**: `vb::designer::Designer`, `VbObject::{designer, object_kind,
  static_bytes, instancing}`, `Instancing`,
  `ObjectTypeFlags::{is_exposed, is_global_namespace}`,
  `VbProject::com_registration`, `MethodLink::kind`, `MethodEntry::Declare`,
  `OptionalObjectInfo::basic_class_object_size`, `proc_dsc_va` on
  `CodeEntry` and `CodeEntrypoint`, `ProjectInfo2Iter`, `ProjectData::path`,
  `GuiTableEntry::object_index`, `ExternalDeclareInfo::{ordinal,
  is_by_ordinal, resolve_cache_va}`.
- **Hosted ActiveX controls**: `ExternalComponentEntry::{clsid,
  events_iid, default_iid, instance_events_iid, array_events_iid,
  declared_event_count, license_key, event_dispids, event_conversions,
  bindable_properties, extender_flags, control_flags}`, `BindableProperty`,
  `ParamConversion`, `ConversionKind`; extender events are named.
- **Form data**: `FormDataParser::{form_name, form_type}`,
  `FormControlRecord::has_submenu`, `Property::index`,
  `PropertyIter::{position, at_terminator}`, `PropertyValue::{Single,
  Bounds, Scale}`, `ControlBounds`, `ScaleState`, `UserScale`.
- `impl FromStr for Guid` and `Error::InvalidGuid`.
- 34 runtime exports missing from the export table.
- Compiled test fixtures covering classes, forms, MDI, controls, ActiveX
  DLL/OCX/EXE projects, hosted ActiveX controls, every intrinsic control
  property, native builds and opcode coverage, with a Docker/Wine build and
  test suites for the stack simulation, call resolution and structures.

### Changed

- Opcode table stack effects: a Variant on the evaluation stack is its
  address (one slot); `Mem*`, `VCall*` and `Late*` take their object from
  Pr and the Pr loaders push nothing; the x87 result of `ImpAdCallFPR4`,
  `VCallFPR8` and `ThisVCall*` is the callee's.
- `FuncTypDesc` reads the entry count, property kind, `ParamArray` flag and
  type list as the compiler writes them; `ArgType` is a struct.
- Every P-Code procedure's arguments start at `ebp+0x0C`
  (`pcode_frame::FIRST_ARG`); `FrameResolver` names `ebp+8` `me` or
  `module_data`.
- `ProcOptFlags::{FRIEND, ADJUSTED_ME_AS_PRIMARY}` replace
  `HAS_ERROR_HANDLER` / `HAS_RESUME_NEXT`.
- Renamed: `Callee::Api` to `Declare`; `StackEffect::writes_pr` to `pr`;
  `PrivateObjectDescriptor::{func_count, func_count2, var_count,
  desc_size}` to `member_count`, `event_count`, `var_stub_count`,
  `instance_size`; `VbObject::class_form_public_bytes` to `public_bytes`
  (covers modules); `OptionalObjectInfo::pcode_count_raw` to
  `inherited_vtable_slots`; `EventHandlerThunk::{event_dispatch_id,
  return_handler_va}` to `this_adjust`, `engine_thunk_va`;
  `PropType::LongPair` to `Bounds`.
- An unknown property index is named `"?"` (with `Property::index`)
  instead of a leaked string.
- `examples/dump` annotates calls with their callee and effect.

### Removed

- `ControlPosition` / `PropertyValue::Position` (now `ControlBounds`),
  `decode_form_type`, `MethodLink::this_adjust`, `OpcodeInfo::{mem_read,
  mem_write}` (now `movement`), `ClassFormPublicBytes::{default_iid,
  events_iid}`, `eventname::standard_event_name`.
- `ExternalComponentEntry::{info_block_size, event_count, event_names}`:
  the fields are the licence key length and bindable properties.
- `ConstantPool`'s byte-offset accessors and `ConstPoolEntry`,
  `ImportResolver` / `CallTarget`, `PublicVarTable` and its iterator,
  `OptionalObjectInfo::{pcode_count, is_native_sentinel}`,
  `ExternalDeclareInfo::{api_stub, native_stub_va}`.

### Fixed

- Opcode table rows (arities of `ForVar`, `ForStepVar`, `PutMem8`,
  `PutMemVar`, `VerifyVarObj`, the `Next*` and `ExitProcCb*` forms;
  operand kinds of `ParmAry1St`, `WMemSt*`, `LitVarStr`, `CStrVarVal`;
  several instruction sizes) and runtime export signatures (`Rnd`,
  `rtcDateAdd`, `rtcVarFromFormatVar`, `rtDecFromVar`, the `VBDll*`
  exports).
- Late-bound member names are null-terminated UTF-16, not BSTRs.
- Form data: the designer's own record is no longer decoded as properties;
  `ScaleMode` carries its scale and `AutoRedraw` / `FontTransparent` (a
  PictureBox's misaligned the rest of its record); every menu after the
  first was lost; control bounds, Singles and signed coordinates decode
  correctly; an empty OLE container's `OleObjectBlob` has no data; 52
  property names and the control GUID table follow the runtime.
- `GuiObjectType` codes, designer vtable bases, control getters of
  controls without handlers, and the IDs and DISPIDs of hosted controls.
- `VbObject::{func_type_descs, has_pcode, events, methods, method_name}`,
  module variable tables, `ControlTypeIter`, `MethodLinkIterator`,
  `ExternalTableEntry::as_typelib` and the `ComRegObject` type tests.
- `code_entrypoints` resolves P-Code `Sub Main`; `code_entries` lists each
  method once.
- Counts read from the file are capped at what the data can hold, and a
  packer's `push imm32; ret` entry point is not taken for a VB6 header.

## [0.3.2] - 2026-09-13

### Added

- `ObjectInfo::has_method_table()` - `true` only when `methods_va` is non-zero,
  differs from `constants_va`, and the object's own `method_count` is non-zero.
- `VbProject::diagnostics()` reports an `Info` `Quirk` at site `method_table`
  when `methods_va` is set but the object's method count is zero (the pointer
  is uninitialized, not a dispatch table).

### Fixed

- `VbObject::has_method_table()` now delegates to
  `ObjectInfo::has_method_table()`. An object with a zero method count no longer
  has a descriptor-supplied count iterated over its uninitialized table pointer.
- `VbObject::code_entries()` and `VbProject::code_entrypoints()` are fail-soft
  per slot: an unreadable method slot, `proc_size`, or object is dropped (with a
  `tracing` warn event under the `tracing` feature) instead of aborting the
  whole walk.

### Changed

- Replaced em-dashes with plain hyphens across docs, comments, and
  `ParseDiagnostic` message strings. Consumers matching on exact diagnostic
  message text will see the new punctuation; match on `site` / `kind` instead.
- Refreshed transitive dependencies (`cargo update`). Direct dependencies were
  already current.
- UTF-16LE decoding in `BStr::to_string_lossy()` and `FuncTypDesc` string
  defaults uses `as_chunks::<2>()` instead of `chunks_exact(2)`, satisfying the
  `chunks_exact_to_as_chunks` lint added in clippy 1.98. No behaviour change.

## [0.3.1] - 2026-08-09

### Changed

- Recorded ATRAPS LLC as copyright holder and added a `NOTICE` file. The Apache-2.0
  appendix was never filled in - it still carried the literal
  `[yyyy] [name of copyright owner]` placeholder, so nothing in this repo stated
  who owned it. No functional change.
- Dropped the deprecated `authors` field and repointed `repository` at the organisation.
- Refreshed transitive dependencies (`cargo update`).
- CI lints `--all-targets --all-features`, so lint failures outside the library are
  gated rather than invisible.
- Publishing now uses crates.io trusted publishing instead of a stored registry token.

## [0.3.0] - 2026-06-09

### Added

- `CallApiStub::ordinal()`, `flags()`, and `is_by_ordinal()` - the
  `DllFunctionCall` descriptor is 16 bytes (not 8), carrying an import ordinal
  and a by-ordinal flag (bit 1) after the name pointers. Verified against
  MSVBVM60.DLL `sub_660315de`. By-ordinal imports omit the API name from the
  binary, a name-hiding signal for triage.
- `ExternalDeclareInfo::api_stub()` resolves a `Declare`'s native stub to its
  `CallApiStub`, exposing ordinal / by-ordinal state for declared imports
  (e.g. `Declare ... Alias "#123"`).
- `controlprop::CleanupAction` plus `ControlPropertyType::cleanup_action()`
  and `is_reference()` - the runtime resource-release classification
  (`FreeString` / `FreeVariant` / `ReleaseObject` / `DestroyArray` /
  `UnlockArray` / `DestructRecord`) for each class/form instance member,
  recovered from `CleanupSingleEntry` (0x66016AAA).
- `decoder::ErrorFlow` plus `Instruction::error_flow()` - classifies a
  `Resume` / `OnErrorGoto` instruction's signed operand into its source-level
  construct (`Resume`, `Resume Next`, `Resume <label>`, `On Error GoTo <label>`,
  `On Error Resume Next`, `On Error GoTo 0`). Verified against `op_Lead2_Resume`
  and `op_OnErrorGoto`.
- `Instruction::is_bos()` / `bos_distance()`, `OpcodeInfo::is_bos()`,
  `OpcodeSemantics::Bos`, and `PCodeMethod::statement_markers()` returning
  `StatementMarker { offset, distance }` - `LargeBos` is now modelled as a
  beginning-of-statement marker whose `u8` operand is the byte distance to the
  next statement boundary (`0` = last). This partitions a procedure's P-Code
  into source statements.
- `code_entrypoints()` now resolves the `Sub Main` target: a P-Code `Sub Main`
  carries `is_pcode = true` plus `stub_va` / `data_const_va` / `pcode_size`, and
  the entry is attributed to its owning module via `object_index` /
  `method_index` (by matching `lpSubMain` against the collected method entries).

### Changed

- **(breaking)** `ControlPropertyType` variants now reflect the runtime
  cleanup dispatcher rather than the size buckets: nibble 1 = `String`
  (was `Short`), 2 = `Variant` (was `Integer`), 3 = `Object` (was `Long`),
  4 = `FixedString`, 5 = `Array` (was `SafeArray`), 6 = `FixedArray`
  (was `Variant`), 9 = `Udt` (was `Object`); inline value nibbles collapse to
  `Value(u8)`. The previous names came from `CalcPropertyDataSize` alone, which
  cannot distinguish a 4-byte BSTR pointer from a 4-byte `Long`.
- **(breaking)** `LargeBos` is reclassified from `OpcodeSemantics::Nop` to
  `OpcodeSemantics::Bos`, and its operand is documented as the distance to the
  next statement boundary rather than a "line number."

### Fixed

- `Resume` and `OnErrorGoto` no longer disassemble their sentinel operands as
  bogus jump targets (`loc_FFFF` / `loc_FFFE`). They now render as
  `Resume Next`, `Resume`, `On Error Resume Next`, and `On Error GoTo 0` per the
  signed-operand encoding.

- Class/form instance property entry stride: `ControlPropertyEntry::total_size()`
  is now a faithful port of `CalcPropertyDataSize` and no longer adds an extra
  4-byte header to non-array entries. The previous `header + base` model
  over-counted nibbles 1/2/3/6/8/9/0xB by 4 bytes, which would desync the entry
  iterator on multi-member class tables.
- Parser robustness: `VbProject::from_bytes` now parses the PE with resources and
  imports skipped and goblin's permissive mode. VB6 navigation never needs
  goblin's resource/import tables, and packed / anti-analysis samples frequently
  carry malformed `.rsrc` or import data that made goblin's strict parser reject
  the whole file.
- P-Code decoder no longer reports spurious `UnexpectedEndOfPCode` errors on the
  zero-byte alignment padding VB6 appends to each procedure (proc size is padded
  to a 4-byte boundary). A lone trailing `0x00` previously looked like a
  truncated `LargeBos`; the iterator now ends cleanly at an all-zero tail.

## [0.2.1] - 2026-05-04

Patch release focused on parser integration ergonomics and stable
persistence surfaces.

### Added

- `VbProject::va_to_rva()`, `VbProject::pcode_method_rva()`, and
  `VbProject::code_entrypoint_rva()` so consumers can convert VB6 VAs
  without threading `image_base` through caller code.
- Stable `as_str()` discriminator helpers for `ExternalKind`,
  `PropertyValue`, `FormControlType`, and the new `IidKind`.
- `PropertyValue::picture_bytes()` and `PictureData::bytes()` for direct
  access to embedded BMP/ICO payload bytes.
- `PropertyValue::display_truncated(limit)` for char-boundary-safe display
  truncation.
- `ComRegData::MIN_BUFFER_SIZE` for callers that pre-slice COM registration
  data before parsing.
- `OptionalObjectInfo::typed_iids()` with `IidKind` and `TypedIidIter` to
  iterate GUI, default, and event IIDs through one typed stream.
- `FormControlRecord::parent_index()` exposing stable parent-control linkage
  in the parsed form-control list.

### Documentation

- Documented `OptionalObjectInfo` and `PrivateObjectDescriptor` accessor
  fallibility semantics.
- Documented the `PCodeMethod::instructions()` upper bound from the on-disk
  `u16` procedure-size field.

## [0.2.0] - 2026-04-26

A breaking-change release focused on adversarial-input safety, richer
typed walkers, and a tagged stream of code entry points. Verified
against runtime reverse-engineering of `MSVBVM60.DLL` (v6.00.9848)
and `VB6.EXE` (v6.00.8176).

### Added

#### Adversarial-input hardening

- Crate-wide lint denial of `clippy::unwrap_used`, `clippy::expect_used`,
  `clippy::panic`, `clippy::arithmetic_side_effects`, and
  `clippy::indexing_slicing`. Tests are exempt via `cfg(test)` allow.
- `Error::Truncated { needed, available }` for byte-level OOB reads.
- `Error::ArithmeticOverflow { context }` for offset/length wrap.
- All low-level byte readers in `crate::util` are panic-free (use
  `.get(...)` + `<[u8; N]>::try_from`); offset arithmetic uses
  `checked_add` / `wrapping_add` / `saturating_add` per semantics.
- Static `Send + Sync` assertion on `VbProject<'static>` and
  `PCodeMethod<'static>` so a future non-Send field breaks compilation
  here, not silently at a downstream `.await`.

#### Predicates and forward-compat aliases

- `OpcodeInfo::is_terminator()` and `OpcodeInfo::is_call()` -
  convenience predicates for CFG-style basic-block splitting.
- `CompilationMode { Pcode, Native, Mixed }` enum and
  `VbProject::compilation_mode()` - combines the project-level
  `lpNativeCode` flag with a per-object `has_pcode()` scan, so
  mixed-mode binaries surface explicitly.
- `PCodeMethod::cleanup_entries()` - re-export of the cleanup-table
  iterator on `ProcDscInfo` for ergonomic consumer access.
- `ConstantPool::entries_with_hints()` - reserved signature aliasing
  `entries()` for forward compatibility with future hint-enriched entries.
- `VbObject::form_designer_data()` - reserved signature aliasing
  `form_data_from_gui_entry()`.

#### Joined walkers and aggregators

- `VbObject::events()` and `VbObject::events_all_slots()` returning
  `Vec<EventBinding>` - joined walker over controls × event sink slots ×
  per-control-type event-name templates.
- `EventBinding { control_index, control_name, control_type, event_slot,
  event_name, handler_va }` with `is_connected()` and `label()` helpers.
- `VbProject::gui_entries_with_form_data()` returning
  `GuiEntriesWithFormData` iterator yielding `GuiEntryWithFormData
  { entry, form_data }` pairs - pre-pairs each form metadata entry with
  its parsed form binary.
- `VbProject::code_entrypoints()` - single-call aggregator returning
  `Vec<CodeEntrypoint>` over per-object dispatch + thunks + events +
  `Sub Main`. Each carries an `EntrypointKind` tag.
- `EntrypointKind { PCodeStub, NativeProc, NativeThunk, EventHandler,
  SubMain }` (`#[non_exhaustive]`).
- `InterfaceMetadata::typelib_path_va()` and
  `InterfaceMetadata::data_slot_va()` - raw VA accessors for the +0x10
  and +0x18 fields, with explicit "purpose undocumented" doc on +0x18.

#### Operand and constant pool typed accessors

- `Instruction::data_type()` - exposes the parent opcode's
  `PCodeDataType` for the instruction's stack result.
- `Instruction::operand_type(idx)` - per-slot inferred type, validated
  against operand presence.
- `ConstantPool::string_at(index)` - indexed BSTR accessor returning
  `Result<Option<BStr>, Error>`.
- `ConstantPool::api_stub_at(index)` - indexed `CallApiStub` resolution
  returning `Result<Option<CallApiStub>, Error>`.

#### Diagnostics and optional tracing

- Optional `tracing` cargo feature (`--features tracing`) - emits
  `target = "visualbasic::dropped"` `warn` events at silent fail-soft
  sites (`code_entries`, `events_inner`, `ImportResolver`,
  `CallResolver`). Default builds carry zero `tracing` dependency
  weight; helpers compile to no-ops.
- `VbProject::diagnostics()` returning `Vec<ParseDiagnostic>` - eager
  parse-health probe flagging missing `Sub Main`, mixed compilation
  mode, suspicious-absent `OptionalObjectInfo`/`PrivateObjectDescriptor`,
  and method-table overlap.
- `ParseDiagnostic`, `DiagnosticKind`, `DiagnosticSeverity` (all
  `#[non_exhaustive]`).

#### Recognition error discrimination

- `Error::NotRecognized` - valid PE container but no VB6 marker.
- `Error::TruncatedContainer { context }` - recognized as VB6 but a
  structure read overran the buffer.
- `Error::UnrecognizedFormat { reason }` - not a recognizable PE
  container at all (or PE32+).
- `RecognitionFailure { NotRecognized, TruncatedContainer,
  UnrecognizedFormat, CompressedAndOpaque }` (`#[non_exhaustive]`)
  with `Error::recognition_failure() -> Option<RecognitionFailure>`
  classifier - lets consumers silently deny non-VB6 files and only
  warn on truncation cases. `CompressedAndOpaque` is reserved for a
  future heuristic and not yet emitted.

#### Documentation

- `# Robustness contract` section in the crate root documenting the
  three behavioural categories (fail-loud primitives, skip-and-continue
  iterators, fail-soft high-level joins) plus the recognition-time
  `Error::recognition_failure` classification.
- `# Adversarial input invariants` section documenting the lint set
  guarantees.

### Changed (breaking)

#### `Result`-cascading accessors

Every fixed-offset accessor that reads from a backing byte slice now
returns `Result<T, Error>` instead of a panicking primitive. Affected
APIs include all `Vb*::*_va`, `*::frame_size`, `*::method_count`, etc.
across `src/vb/*` and `src/project/*`.

```rust
// Before:
let frame = method.frame_size();
let v = obj.method_count();

// After:
let frame = method.frame_size()?;
let v = obj.method_count()?;
```

The fallible signatures match the malware-analysis posture - every
read can now surface `Error::Truncated` rather than panicking on a
short slice.

#### `name()` returns `Cow<'a, str>`

For `VbObject`, `VbProject::project_name`, `VbControl::name`,
`FormControlRecord::name`, `ExternalComponentEntry::ocx_filename` /
`prog_id` / `class_name`, and `CallApiStub::library_name` /
`function_name`:

- The string accessor returns `Cow<'a, str>` (lossy UTF-8) - borrows
  for valid UTF-8 (the common case for ASCII identifiers), allocates
  only on U+FFFD substitution.
- The byte form is preserved as `*_bytes()`.

```rust
// Before:
let name = String::from_utf8_lossy(&obj.name()?);

// After:
let name = obj.name()?;            // Cow<'a, str>
let raw  = obj.name_bytes()?;      // &'a [u8]
```

`MethodNameResult::as_str()` added alongside `as_bytes()`.
`EventBinding::control_name` is `Cow<'a, str>` rather than `&'a [u8]`.

#### `Error::VbHeaderNotFound` removed

Replaced by the three discriminated variants `NotRecognized`,
`TruncatedContainer`, and `UnrecognizedFormat` documented in the
*Added → Recognition error discrimination* section above.
`VbProject::from_bytes` now classifies goblin parse failures as
`UnrecognizedFormat` and structural truncation during recognition
as `TruncatedContainer`, leaving `NotRecognized` for the
"no VB6 marker" case.

#### `u16` widening uses `From` instead of `as`

Internal cleanup: 17 sites where `index as u32` were mechanically
converted to `u32::from(index)`. No runtime change; cleaner code.
Affects `vbobject.rs`, `pcodemethod.rs`, `methodlink.rs`,
`methodentry.rs`, `constantpool.rs`, `functype.rs`.

### Removed

- `Error::VbHeaderNotFound` - superseded by the discriminated variants
  above. Match on `Error::recognition_failure()` to map old
  `VbHeaderNotFound` semantics to the new
  `RecognitionFailure::NotRecognized`.

### Fixed

- The constant-pool resolver is no longer single-threaded behind a
  panicking accessor; downstream code can run parses across threads
  thanks to the new `Send + Sync` assertion plus the elimination of
  panicking byte reads.

### Security

- The full crate parse path was audited under
  `clippy::{unwrap_used, expect_used, panic, arithmetic_side_effects,
  indexing_slicing}` - **no input byte sequence can panic the parser.**
  ~538 lint sites in the library and ~215 in `build.rs` were converted
  to checked alternatives. `build.rs` runs at compile time on
  CSV input from `data/` and is exempt from the per-byte lints.

### Reverse-engineering notes

- Verified `ProcCallEngine_Body` (0x66108C00) and `op_Lead2_Resume`
  (0x6610F212) in MSVBVM60: only `+0x06`, `+0x0C`, `+0x10`, `+0x18`,
  `+0x1C` of `ProcDscInfo` are read by the runtime. There is **no**
  `wLocalsNameTableOffset` field - per-procedure local-variable names
  are not recoverable from compiled binaries (documented in `TODO.md`).
- Verified `EbLoadRunTime` (0x6602F6CE): `PublicVarTable` entries
  carry only `frame_offset + type_code`. Variable names are
  recoverable indirectly via the trailing `var_count` entries of the
  FuncTypDesc array (`PrivateObjectDescriptor.func_type_descs_va`);
  static defaults don't exist in compiled binaries (documented in
  `TODO.md`).

## [0.1.0] - 2026-03-31

Initial public release.

[Unreleased]: https://github.com/ATRAPSLLC/visualbasic-rs/compare/v0.5.0...HEAD
[0.5.0]: https://github.com/ATRAPSLLC/visualbasic-rs/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/ATRAPSLLC/visualbasic-rs/compare/v0.3.2...v0.4.0
[0.3.2]: https://github.com/ATRAPSLLC/visualbasic-rs/compare/v0.3.1...v0.3.2
[0.3.1]: https://github.com/ATRAPSLLC/visualbasic-rs/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/ATRAPSLLC/visualbasic-rs/compare/v0.2.1...v0.3.0
[0.2.1]: https://github.com/ATRAPSLLC/visualbasic-rs/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/ATRAPSLLC/visualbasic-rs/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/ATRAPSLLC/visualbasic-rs/releases/tag/v0.1.0
