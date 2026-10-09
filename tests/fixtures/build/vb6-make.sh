#!/bin/bash
# Compiles one VB6 project inside the `vb6-fixtures:wine` image.
#
# Run by `build-wine.sh`, with the toolchain mounted at `/toolchain` and the
# fixtures directory at `/work`:
#
#   vb6-make.sh <project dir> <output base name>
#
# The output keeps the extension VB6 gave it (`.exe`, `.dll`, `.ocx`).
# Installs the toolchain into the prefix the way Setup would - the compiler in
# `C:\VB98`, the runtime and the type libraries in the system directory,
# registered, and the licence keys imported - then runs `VB6.EXE /make` on the
# project and fails unless VB6 reports the build succeeded.

set -euo pipefail

readonly project="${1:?usage: vb6-make.sh <project dir> <output name>}"
readonly output="${2:?usage: vb6-make.sh <project dir> <output name>}"
readonly vbp="$(basename "$project").vbp"
readonly vb98="$WINEPREFIX/drive_c/VB98"
readonly system="$WINEPREFIX/drive_c/windows/syswow64"

mkdir -p "$vb98"
for file in VB6.EXE VBA6.DLL VB6.OLB VB6EXT.OLB VB6IDE.DLL VB6DEBUG.DLL C2.EXE \
    LINK.EXE CVPACK.EXE MSPDB60.DLL VBAEXE6.LIB; do
    cp "/toolchain/$file" "$vb98/"
done
for file in MSVBVM60.DLL STDOLE2.TLB MSSTDFMT.DLL DAO350.DLL DAO2535.TLB MSDATSRC.TLB \
    MSWINSCK.OCX MSINET.OCX COMDLG32.OCX MSMASK32.OCX MSCOMCTL.OCX; do
    cp "/toolchain/$file" "$system/"
done
for dll in MSVBVM60.DLL DAO350.DLL MSSTDFMT.DLL \
    MSWINSCK.OCX MSINET.OCX COMDLG32.OCX MSMASK32.OCX MSCOMCTL.OCX; do
    wine regsvr32 /s "C:\\windows\\syswow64\\$dll"
done
wine regedit /S 'Z:\toolchain\licenses.reg'
# A project hosting a control another fixture built (`Object=...; name.ocx`
# not in the toolchain) has that fixture's OCX registered too.
{ grep -i '^Object=' "/work/$project/$vbp" || true; } | sed -E 's/.*; *//; s/\r$//' | while read -r ocx; do
    [ -f "/toolchain/$ocx" ] && continue
    built="$(find /work -maxdepth 2 -iname "$ocx" | head -1)"
    [ -n "$built" ] || continue
    cp "$built" "$system/"
    wine regsvr32 /s "C:\\windows\\syswow64\\$(basename "$built")"
done
# The data-bindable controls (MaskEdBox) reference the Data Source
# Interfaces type library; with it unregistered VB6 reports "Object library
# not registered". Wine has no tool to register a bare .tlb, so its keys are
# written directly.
msdatsrc='HKEY_CLASSES_ROOT\TypeLib\{7C0FFAB0-CD84-11D0-949A-00A0C91110ED}\1.0'
printf 'REGEDIT4\r\n\r\n[%s]\r\n@="Microsoft Data Source Interfaces"\r\n\r\n[%s\\0\\win32]\r\n@="C:\\\\windows\\\\syswow64\\\\MSDATSRC.TLB"\r\n\r\n[%s\\FLAGS]\r\n@="0"\r\n' \
    "$msdatsrc" "$msdatsrc" "$msdatsrc" > /tmp/msdatsrc.reg
wine regedit /S 'Z:\tmp\msdatsrc.reg'
# An ActiveX project (a DLL, OCX or ActiveX EXE) makes VB6 write the
# project's type library through `ICreateTypeLib2`, whose Wine implementation
# fails to save it: the build stops with "Automation error" before linking.
# A project that hosts ActiveX controls (an `Object=` line) fails the same
# way with "Not implemented". Such projects build with the media's own
# OLEAUT32.DLL in place of Wine's; any other keeps Wine's. The runtime and
# the controls are registered with Wine's first.
# Wine maps the DLLs its KnownDLLs key lists from its own copies when its
# server starts, so the entry is removed and the server restarted before VB6
# runs.
if ! grep -qi '^Type=Exe' "/work/$project/$vbp" || grep -qi '^Object=' "/work/$project/$vbp"; then
    cp /toolchain/OLEAUT32.DLL "$system/oleaut32.dll"
    export WINEDLLOVERRIDES="oleaut32=n,b"
    wine reg delete 'HKLM\System\CurrentControlSet\Control\Session Manager\KnownDLLs' \
        /v oleaut32 /f >/dev/null
    wineserver -w
fi

out="$(mktemp -d)"
log="$out/build.log"
Xvfb :99 -screen 0 1024x768x24 -nolisten tcp 2>/dev/null &
xvfb=$!
export DISPLAY=:99
sleep 1
# VB6 keeps the IDE's message loop running after a failed build, so it is
# bounded rather than waited on, and left once the log reports the failure.
# Sources need CRLF line endings: with LF alone VB6 fails without a reason.
wine 'C:\VB98\VB6.EXE' /make "Z:\\work\\${project//\//\\}\\$vbp" \
    /outdir "Z:${out//\//\\}" /out "Z:${log//\//\\}" &
pid=$!
for _ in $(seq 1 300); do
    kill -0 "$pid" 2>/dev/null || break
    if grep -q 'failed' "$log" 2>/dev/null; then
        sleep 5
        break
    fi
    sleep 1
done
kill "$pid" 2>/dev/null || true
wineserver -k || true
kill "$xvfb" 2>/dev/null || true

cat "$log" 2>/dev/null || { echo "VB6 wrote no build log" >&2; exit 1; }
grep -q 'succeeded' "$log" || exit 1
built="$(find "$out" -maxdepth 1 \( -iname '*.exe' -o -iname '*.dll' -o -iname '*.ocx' \) | head -1)"
[ -n "$built" ] || { echo "VB6 reported success and wrote no binary" >&2; exit 1; }
extension="$(echo "${built##*.}" | tr '[:upper:]' '[:lower:]')"
cp "$built" "/work/$project/$output.$extension"
chown "$(stat -c %u:%g /work)" "/work/$project/$output.$extension"
