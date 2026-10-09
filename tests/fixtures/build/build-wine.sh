#!/bin/bash
# Builds the VB6 fixtures on Linux, under Wine, with no Windows host.
#
# The compiler is the one `extract-toolchain.sh` extracted from licensed media
# into `toolchain/vb6/`; the image `Dockerfile` describes holds only Wine. Each
# project is a directory of `tests/fixtures` holding `<name>.vbp` and its
# sources, and builds to `<name>.exe` (`.dll`, `.ocx` for an ActiveX project)
# beside them:
#
#   ./build-wine.sh --all              # every project
#   ./build-wine.sh <name> [...]       # e.g. ./build-wine.sh calls late
#
# `--alt` as the first argument builds to `<name>-alt.<ext>` instead, which
# is what the nondeterminism check compares.

set -euo pipefail

cd "$(dirname "$0")"

readonly IMAGE="vb6-fixtures:wine"
readonly TOOLCHAIN="$(realpath ../../../toolchain/vb6)"

[ -f "$TOOLCHAIN/VB6.EXE" ] \
    || { echo "no toolchain: run extract-toolchain.sh <iso> first" >&2; exit 1; }

# Rebuilt when its Dockerfile is newer than it.
ensure_image() {
    local created
    created="$(docker image inspect -f '{{.Created}}' "$IMAGE" 2>/dev/null || true)"
    if [ -z "$created" ] || [ "$(date -d "$created" +%s)" -lt "$(stat -c %Y Dockerfile)" ]; then
        docker build -t "$IMAGE" .
    fi
}

build() {
    local name="$1" suffix="$2"
    [ -f "../$name/$name.vbp" ] || { echo "no project ../$name/$name.vbp" >&2; exit 1; }
    echo "[$name] -> $name/$name$suffix"
    docker run --rm \
        -v "$(realpath ..):/work" \
        -v "$TOOLCHAIN:/toolchain:ro" \
        -v "$(pwd)/vb6-make.sh:/vb6-make.sh:ro" \
        "$IMAGE" bash /vb6-make.sh "$name" "$name$suffix" \
        2> >(grep -v XDG_RUNTIME_DIR >&2)
}

ensure_image

suffix=""
if [ "${1:-}" = "--alt" ]; then suffix="-alt"; shift; fi
if [ "${1:-}" = "--all" ]; then
    set -- $(cd .. && for vbp in */*.vbp; do dirname "$vbp"; done)
fi
[ "$#" -gt 0 ] || { echo "usage: build-wine.sh [--alt] --all | <name> ..." >&2; exit 2; }
for name in "$@"; do
    build "$name" "$suffix"
done
