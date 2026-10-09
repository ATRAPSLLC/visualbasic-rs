#!/bin/bash
# Extracts the Visual Basic 6 compiler from licensed Visual Studio 6.0 media
# into `toolchain/vb6/`, which git ignores: the files are never redistributed,
# only kept beside the fixtures they build.
#
# Usage:
#
#   tests/fixtures/build/extract-toolchain.sh <Microsoft_Visual_Studio_6.0.iso>
#
# Only the files `VB6.EXE /make` needs are taken - the IDE and compiler, the
# code generator and linker, the runtime and type libraries a project
# references, and the ActiveX controls a fixture hosts - each checked against
# `toolchain.sha256`, so a different ISO is refused rather than built with.

set -euo pipefail

cd "$(dirname "$0")"
readonly ISO="$(realpath "${1:?usage: extract-toolchain.sh <iso>}")"
readonly DEST="../../../toolchain/vb6"

# `<path on the ISO>`, one per line.
readonly FILES=(
    VB98/VB6.EXE
    VB98/VBA6.DLL
    VB98/VB6.OLB
    VB98/VB6EXT.OLB
    VB98/VB6IDE.DLL
    VB98/VB6DEBUG.DLL
    VB98/C2.EXE
    VB98/LINK.EXE
    VB98/CVPACK.EXE
    VB98/MSPDB60.DLL
    VB98/VBAEXE6.LIB
    OS/SYSTEM/MSVBVM60.DLL
    OS/SYSTEM/STDOLE2.TLB
    OS/SYSTEM/MSSTDFMT.DLL
    OS/SYSTEM/DAO350.DLL
    OS/SYSTEM/DAO2535.TLB
    OS/SYSTEM/OLEAUT32.DLL
    OS/SYSTEM/MSWINSCK.OCX
    OS/SYSTEM/MSINET.OCX
    OS/SYSTEM/COMDLG32.OCX
    OS/SYSTEM/MSMASK32.OCX
    OS/SYSTEM/MSCOMCTL.OCX
    SHARED/MSADC/MSDATSRC.TLB
)

mkdir -p "$DEST"
for path in "${FILES[@]}"; do
    7z e -y -bso0 -bsp0 -o"$DEST" "$ISO" "$path"
done
(cd "$DEST" && sha256sum -c --quiet "$OLDPWD/toolchain.sha256")

# The licence keys Setup writes from the same media, without which VB6 runs as
# the Working Model edition and refuses `/make`, and the design-time licences
# of the ActiveX controls (Winsock, Inet), without which VB6 cannot load a
# form that hosts them. Read from the setup table and the controls' registry
# scripts at extraction, like the rest of the toolchain, and never committed.
setup="$(mktemp -d)"
trap 'rm -rf "$setup"' EXIT
7z e -y -bso0 -bsp0 -o"$setup" "$ISO" SETUP/VS98ENT.STF 'OS/SYSTEM/*.SRG'
{
    printf 'REGEDIT4\r\n'
    {
        grep -a 'AddRegData' "$setup/VS98ENT.STF" \
            | sed -nE 's/.*""Licenses\\([^"]+)"","""",""([^"]+)"".*/\1 \2/p'
        # `[HKEY_CLASSES_ROOT\Licenses\<key>]` then `@ = "<value>"`.
        cat "$setup"/*.SRG | tr -d '\r' \
            | sed -nE '/^\[HKEY_CLASSES_ROOT\\Licenses\\/{N;s/^.*Licenses\\([^]]+)\]\n@ *= *"([^"]+)".*/\1 \2/p}'
    } \
        | sort -u \
        | while read -r key value; do
            printf '\r\n[HKEY_CLASSES_ROOT\\Licenses\\%s]\r\n@="%s"\r\n' "$key" "$value"
        done
} > "$DEST/licenses.reg"

echo "extracted ${#FILES[@]} files and $(grep -c '^\[' "$DEST/licenses.reg") licence keys to $(realpath "$DEST")"
