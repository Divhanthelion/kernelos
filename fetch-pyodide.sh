#!/usr/bin/env bash
#
# Fetch a pinned Pyodide core runtime into assets/py/pyodide/ (PLAN M5b).
# The runtime is ~10–20MB and is deliberately not committed — run this before
# the first `trunk build` / `trunk serve` that needs `run_python`.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
VERSION="0.27.7"
DEST="$ROOT/assets/py/pyodide"
# Minimal core tarball from the official GitHub release (not the 200MB+ full set).
TARBALL_URL="https://github.com/pyodide/pyodide/releases/download/${VERSION}/pyodide-core-${VERSION}.tar.bz2"

for tool in curl tar mkdir rm; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "error: required tool '$tool' was not found" >&2
        exit 1
    fi
done

tmpdir="$(mktemp -d)"
cleanup() { rm -rf "$tmpdir"; }
trap cleanup EXIT

echo "==> fetching Pyodide ${VERSION} core"
curl -fL --retry 3 --retry-delay 2 -o "$tmpdir/pyodide-core.tar.bz2" "$TARBALL_URL"

echo "==> extracting into $DEST"
rm -rf "$DEST"
mkdir -p "$DEST"
# Archive root is a `pyodide/` directory of core files.
tar -xjf "$tmpdir/pyodide-core.tar.bz2" -C "$tmpdir"
if [[ -d "$tmpdir/pyodide" ]]; then
    # Move contents (not the wrapper dir) so indexURL=/py/pyodide/ is correct.
    shopt -s dotglob nullglob
    mv "$tmpdir/pyodide"/* "$DEST"/
    shopt -u dotglob nullglob
else
    echo "error: unexpected archive layout (no pyodide/ directory)" >&2
    exit 1
fi

if [[ ! -f "$DEST/pyodide.js" || ! -f "$DEST/pyodide.asm.wasm" || ! -f "$DEST/python_stdlib.zip" ]]; then
    echo "error: incomplete extract — expected pyodide.js, pyodide.asm.wasm, python_stdlib.zip" >&2
    ls -la "$DEST" >&2 || true
    exit 1
fi

echo "==> Pyodide ${VERSION} ready at assets/py/pyodide/"
du -sh "$DEST"
