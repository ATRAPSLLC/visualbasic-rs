# VB6 fixtures

Each directory is a Visual Basic 6 project compiled to P-code (the
`-native` ones to native code), its sources beside the binary the build
produced (an executable, or the DLL or OCX of an ActiveX project). They are
our own programs: a binary imports the VB runtime (`MSVBVM60.DLL`) and
contains none of it.

| Project | What it exercises |
|---|---|
| `hello` | The smallest program: a module function and `Sub Main`. |
| `calls` | Calls on objects: a class's public and private methods, property `Get`/`Let`/`Set`, parameters `ByVal` of every width, `ByRef`, `Optional` and `ParamArray`, a call through `Me`, `Implements`, `RaiseEvent`. |
| `late` | Late-bound calls on an `Object`: methods, properties, a named argument. |
| `controls` | A form whose code uses a `TextBox` and a `CommandButton`. |
| `exprs` | Arithmetic, strings, `Variant` temporaries, file I/O, declared API calls, a `ByRef` swap, `Select Case`. |
| `types` | One public class method per parameter and return type, `ByRef`, arrays, `Optional` with defaults, `ParamArray`, indexed properties, `Friend`, private methods, module functions of each return type, `Declare`s. |
| `flow` | `For`/`Next` over every counter type (`Integer`, `Long`, `Single`, `Double`, `Currency`, `Byte`, `Date`, `Variant`) with no, a constant and a variable `Step`; `For Each` over a `Collection`, arrays and a `Variant` holding an array; the `Do` loops, `While`/`Wend`, `Exit For`/`Exit Do`; `Select Case` with lists, ranges and `Is` on integers, strings, floats, `Currency` and `Variant`s; nested `GoSub`; `On ... GoTo`/`GoSub`; `On Error GoTo`, `Resume`, `Resume Next`, `Resume` to a label, `On Error Resume Next`, `On Error GoTo 0`, `Err.Raise`, `Error`, `Erl` (VB marks statements with `LargeBos` only in the procedures that use `Resume`, `Resume Next`, `On Error Resume Next` or line numbers: `ErrResumeNext`, `ErrResume`, `ErrInline`, `ErrLines`; not in those whose handler only exits, re-raises or does `Resume` to a label); `With`; `IIf`, `Choose`, `Switch`. |
| `flow-native` | `flow`'s source compiled to native code (`CompilationType=0`): a project with no P-code. |
| `data` | `ReDim`, `ReDim Preserve`, multi-dimensional, static and UDT arrays, `Erase`, `LBound`/`UBound`; UDTs with fixed-length strings and arrays; `Variant` operators, `Like`, `Null`; `Currency` and `Date` arithmetic; the string functions and the `Mid` statement; every conversion function; math; file I/O in each mode (`Print #`, `Write #`, `Input #`, `Line Input #`, `Get`/`Put` of a UDT with and without a record number, `Seek`, `EOF`, `LOF`); `Debug.Print`; `Is`, `TypeOf`, `IsMissing`, `IsNull`, `VarType`, `TypeName`. |
| `forms` | A form's private Subs and Function called from event handlers and from each other, public members called from a module, `Me`, a control array (an element loaded at run time, the `Index` argument), the `Controls` collection, and a UserControl with a property, an event and a constituent control. |
| `dispid` | Calls by DISPID (`LateId*`): a form's calls on the UserControl it hosts, which go through the control's extender - properties, indexed and object properties, methods, named arguments. Also the `LateMem*` forms `late` lacks: a plain property store, named arguments to a Sub, a named indexed store and a named `Set`. |
| `events` | `WithEvents` handlers, `RaiseEvent` with `ByVal` and `ByRef` arguments, `Class_Initialize`/`Terminate`, `Implements` of an interface with methods of each return type called through it, private class methods of each return type called through `Me`, a `Friend` call. |
| `statics` | Storage outside the frame: a module's own `Private` and `Public` variables (`FMem*` on the module data block at `ebp+8`), `Static` locals and a `Static Function` in a module and in a class (a block whose pointer is at module data + 0x3C, `Me` + 0x40, loaded into Pr), a UDT and an array member, `With` on a module UDT, another module's `Public` variables (`ImpAd*`). |
| `vtable` | Calls that return their value without an HRESULT: through an interface of `Raw.tlb` (typed `VCall*`), and `Declare`d functions of every return type (typed `ImpAdCall*`). Compiles only; nothing in it is meant to run. |
| `members` | A class's public member variables of every type, written and read from a module (each through its `GetMem*` / `PutMem*` / `SetMem*` accessor), classes implementing two and three interfaces called through each, and a class with every method-link group: public variables before and after `Implements`, public and private `WithEvents` variables, public, `Friend` and private procedures. |
| `mdi` | An MDIForm and an MDI child form: their own public members, built-in members with and without their optional arguments (`Move`, `Show`, `Arrange`), a child held in a variable and the predeclared instance, `CommandButton`, `TextBox` and `Label` control arrays walked with `For Each`. |
| `activex` | A form hosting third-party ActiveX controls: `MSWINSCK.OCX` Winsock (one and a control array), `MSINET.OCX` Inet, `COMDLG32.OCX` CommonDialog, `MSMASK32.OCX` MaskEdBox (data-bindable properties) and `MSCOMCTL.OCX` ProgressBar, ListView and TreeView; handlers for their events, calls on their methods and properties. |
| `extender` | An ActiveX control project (`extender.ocx`) of UserControls that differ in one designer property each (`Alignable`, `CanGetFocus`, `DefaultCancel`, `InvisibleAtRuntime`, `ControlContainer`, `ForwardFocus`, `DataBindingBehavior`, `Windowless`), one whose events have each stdole coordinate type, Variants and objects, and one hosting an instance of each. |
| `geometry` | A form whose controls have known positions: negative, beyond 32767, nested in a container; a Line with fractional coordinates, a Timer, a Shape. |
| `ocx` | An ActiveX control project (`ocx.ocx`): a public UserControl with a property, events and a method, a second UserControl hosting it (one instance and a control array, handlers for its own and its extender's events), and a PropertyPage. |
| `docs` | An ActiveX document DLL (`docs.dll`): a UserDocument with a property, a method, handlers for its own and its controls' events, and a public class. |
| `server` | An ActiveX EXE (`Type=OleExe`): one class per `Instancing` value (`Private`, `PublicNotCreatable`, `SingleUse`, `GlobalSingleUse`, `MultiUse`, `GlobalMultiUse`), each with a property, a method and an event, used from `Sub Main`. |
| `events-native` | `events`' source compiled to native code: classes with publics, `Implements` and `WithEvents`, with no P-code. |
| `props` | A form with every intrinsic control, each scalar property of its runtime table set to a value other than its default (generated from the table), PictureBoxes with a user scale and with `AutoRedraw` and `FontTransparent`, and a menu tree three levels deep. The form records are checked against the source, property by property. |
| `coverage` | Generated source for the opcode table: per type (`Byte` to `Boolean`, `String`, `Variant`), arithmetic, comparisons and logic, loads and stores through every storage class (locals, `ByRef` parameters, the module's own and another module's variables, array elements, UDT fields, `With`), every conversion function from every type, string and `Variant` comparisons under `Option Compare Text`, `Variant`s passed by value, objects and `IUnknown`s in every storage class, file input of every type and the printing forms, `Print` on a form and on `Printer` with every item separator, a class's members of every type used in its own code and its typed functions called through `Me`. Compiles only. |

## Opcode coverage

Together the fixtures contain 600 of the opcode table's 1163 rows
(`tests/stack.rs` `coverage_census` counts them and keeps the floor). Of the
rest:

- `Lead0` to `Lead4` are the prefix bytes that select tables 1 to 5, not
  instructions.
- The per-item printing and input opcodes (`PrintChan`, `WriteChan`,
  `PrintItem*`, `PrintSpc`, `PrintTab`, `Input`, `InputItem*`,
  `InputDone`) are not what the compiler emits for `Print #`, `Write #`,
  `Input #` or `Print` on an object: it emits `PrintFile`, `WriteFile`,
  `InputFile` and `PrintObject` with a descriptor of the items.
- `Assert`: compiled code drops `Debug.Assert`, as it drops `Debug.Print`.
- `WMem*` and `IWMem*` (memory at the pointer in `[ebp+0x10]`): `With` on a
  module or object variable, `Friend` members and `Static` storage compile
  to `FMem*` and `Mem*` through a temporary instead.

Whether the compiler emits the other rows (typed `VCall*`/`ThisVCall*`
returning `Single`, `Double` or `Currency` by value, the UDT-to-`Variant`
conversions, `ImpAdSt*`, `VarLateMem*`, the `NoPop` frees) is unmeasured:
no fixture source produced them.

## Building

The compiler is extracted from licensed Visual Studio 6.0 media, into the
git-ignored `toolchain/vb6/`, and runs under Wine in Docker:

    tests/fixtures/build/extract-toolchain.sh <Microsoft_Visual_Studio_6.0.iso>
    tests/fixtures/build/build-wine.sh --all

`extract-toolchain.sh` checks every file against `build/toolchain.sha256`.
A project builds with `CompilationType=-1` (P-code) in its `.vbp`.
Its `.vbp` and sources need CRLF line endings: with LF alone VB6 reports
"Build failed" and no reason. A project builds to `<name>.exe`, `.dll` or
`.ocx`, as its type gives.

An ActiveX project (EXE, DLL, OCX) and a project that hosts OCX controls build
with the media's own `OLEAUT32.DLL` in place of Wine's: VB6 creates their
type library through `ICreateTypeLib2`, which Wine's implementation cannot
save ("Automation error", "Not implemented"). `vb6-make.sh` swaps it in
after registering the runtime and the controls with Wine's. The controls'
design-time licences come from the media's registry scripts
(`OS/SYSTEM/*.SRG`). The data-bindable controls (MaskEdBox, the Common
Controls) need the Data Source Interfaces type library (`MSDATSRC.TLB`)
registered, which `vb6-make.sh` does by writing its keys. A
UserControl's `PropertyPages` property lives in its binary `.ctx` file, so
`ocx`'s PropertyPage is not attached to its control.

Under Wine the compiler has no `VBRUN` library ("Visual Basic runtime objects
and procedures"): its constants (`vbBlue`) and classes (`PropertyBag`) do
not resolve, so no fixture uses them.

`vtable` references `Raw.tlb`, the type library `Raw.odl` describes, which is
committed beside it. It was built with MKTYPLIB from the same media
(`VC98/BIN/MKTYPLIB.EXE`), under Wine in the same image, with `STDOLE2.TLB`
copied into the working directory as `stdole2.tlb`:

    MKTYPLIB /nocpp /win32 /tlb Raw.tlb Raw.odl

Wine's type library writer does not implement `module` (DLL entry point)
declarations, so the library holds only the interface.

Two builds of one project differ in the PE timestamp and in the GUIDs VB6
generates for the project and its type library; the P-code and every other
structure are the same (`build-wine.sh --alt <name>` builds `<name>-alt.exe`
to compare).
