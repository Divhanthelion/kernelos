#!/usr/bin/env bash
#
# Build every KernelOS plugin with a hard 16 MiB memory ceiling and stage the
# resulting module plus its manifest under assets/plugins for Trunk.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
PLUGINS_DIR="$ROOT/plugins"
ASSETS_DIR="$ROOT/assets/plugins"
TARGET="wasm32-unknown-unknown"
MAX_PAGES=256
MAX_BYTES=$((MAX_PAGES * 65536))
MODE="${1:-build}"

if [[ "$MODE" != "build" && "$MODE" != "check" ]]; then
    echo "usage: $0 [build|check]" >&2
    exit 2
fi

for tool in cargo python3 rustup shasum cmp; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "error: required tool '$tool' was not found" >&2
        exit 1
    fi
done

if ! rustup target list --installed | awk -v target="$TARGET" '
    $0 == target { found = 1 }
    END { exit(found ? 0 : 1) }
'; then
    rustup target add "$TARGET"
fi

mkdir -p "$ASSETS_DIR"
shopt -s nullglob

plugin_count=0
for plugin_dir in "$PLUGINS_DIR"/*; do
    [[ -d "$plugin_dir" && -f "$plugin_dir/Cargo.toml" ]] || continue
    plugin_count=$((plugin_count + 1))

    slug="$(basename "$plugin_dir")"
    source_manifest="$PLUGINS_DIR/$slug.json"
    if [[ ! -f "$source_manifest" ]]; then
        echo "error: missing plugin manifest $source_manifest" >&2
        exit 1
    fi

    metadata="$(python3 - "$source_manifest" "$plugin_dir/Cargo.toml" <<'PY'
import json
import pathlib
import sys
import tomllib

manifest = json.loads(pathlib.Path(sys.argv[1]).read_text())
cargo = tomllib.loads(pathlib.Path(sys.argv[2]).read_text())
plugin_id = manifest["id"]
max_pages = manifest.get("max_pages", 256)
lib_name = cargo.get("lib", {}).get(
    "name",
    cargo["package"]["name"].replace("-", "_"),
)
print(f"{plugin_id}\t{max_pages}\t{lib_name}")
PY
)"
    IFS=$'\t' read -r plugin_id max_pages lib_name <<<"$metadata"

    if [[ "$plugin_id" != "$slug" ]]; then
        echo "error: $source_manifest id '$plugin_id' must match directory '$slug'" >&2
        exit 1
    fi
    if [[ "$max_pages" != "$MAX_PAGES" ]]; then
        echo "error: $plugin_id max_pages=$max_pages; build cap requires $MAX_PAGES" >&2
        exit 1
    fi

    echo "==> building $plugin_id"
    # Build a wasm-only RUSTFLAGS for this invocation. Do not inherit a shell
    # RUSTFLAGS that already contains `-C link-arg=--max-memory=…` — that flag
    # is meaningless (and fatal) for host `cc` links if it leaks into the
    # surrounding environment. Callers can set PLUGIN_RUSTFLAGS for extras.
    plugin_rustflags="${PLUGIN_RUSTFLAGS:-}"
    if [[ -n "$plugin_rustflags" ]]; then
        plugin_rustflags="$plugin_rustflags "
    fi
    # Hard memory ceiling for the guest linear memory.
    plugin_rustflags="${plugin_rustflags}-C link-arg=--max-memory=$MAX_BYTES"
    # Drop DWARF and remap every absolute prefix we know about so `file!()` /
    # panic locations (and thus wasm bytes + wasm_hash) do not depend on
    # $HOME, the checkout path, the cargo registry hash directory, or the
    # rustc sysroot. Without this, every machine (and often every build) gets
    # a different content hash and pinning is meaningless.
    plugin_rustflags="${plugin_rustflags} -C debuginfo=0"
    plugin_rustflags="${plugin_rustflags} --remap-path-prefix=${ROOT}="
    cargo_home="${CARGO_HOME:-$HOME/.cargo}"
    if [[ -d "$cargo_home/registry/src" ]]; then
        for src_dir in "$cargo_home"/registry/src/*; do
            [[ -d "$src_dir" ]] || continue
            plugin_rustflags="${plugin_rustflags} --remap-path-prefix=${src_dir}=/cargo-registry"
        done
    fi
    if [[ -d "$cargo_home/git/checkouts" ]]; then
        for src_dir in "$cargo_home"/git/checkouts/*/*; do
            [[ -d "$src_dir" ]] || continue
            plugin_rustflags="${plugin_rustflags} --remap-path-prefix=${src_dir}=/cargo-git"
        done
    fi
    sysroot="$(rustc --print sysroot)"
    plugin_rustflags="${plugin_rustflags} --remap-path-prefix=${sysroot}=/rustc"
    if [[ -n "${CARGO_TARGET_DIR:-}" ]]; then
        plugin_rustflags="${plugin_rustflags} --remap-path-prefix=${CARGO_TARGET_DIR}=/target"
    fi
    # Prefer repo remap over $HOME so checkout paths collapse cleanly.
    plugin_rustflags="${plugin_rustflags} --remap-path-prefix=${HOME}="

    RUSTFLAGS="$plugin_rustflags" cargo build \
        --manifest-path "$plugin_dir/Cargo.toml" \
        --target "$TARGET" \
        --release \
        --locked

    target_directory="$(cargo metadata \
        --manifest-path "$plugin_dir/Cargo.toml" \
        --format-version 1 \
        --no-deps \
        --locked |
        python3 -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])')"
    built_wasm="$target_directory/$TARGET/release/$lib_name.wasm"
    if [[ ! -f "$built_wasm" ]]; then
        echo "error: Cargo did not produce $built_wasm" >&2
        exit 1
    fi

    python3 - "$built_wasm" "$MAX_PAGES" <<'PY'
import pathlib
import sys

data = pathlib.Path(sys.argv[1]).read_bytes()
expected_max = int(sys.argv[2])

if data[:8] != b"\0asm\x01\0\0\0":
    raise SystemExit(f"error: {sys.argv[1]} is not a WebAssembly 1.0 module")

def read_uleb(offset):
    value = 0
    shift = 0
    while True:
        byte = data[offset]
        offset += 1
        value |= (byte & 0x7f) << shift
        if byte & 0x80 == 0:
            return value, offset
        shift += 7

offset = 8
memory_maxima = []
while offset < len(data):
    section_id = data[offset]
    offset += 1
    section_size, offset = read_uleb(offset)
    section_end = offset + section_size
    if section_id == 5:
        count, cursor = read_uleb(offset)
        for _ in range(count):
            flags, cursor = read_uleb(cursor)
            _, cursor = read_uleb(cursor)
            maximum = None
            if flags & 0x01:
                maximum, cursor = read_uleb(cursor)
            memory_maxima.append(maximum)
    offset = section_end

if not memory_maxima:
    raise SystemExit(f"error: {sys.argv[1]} has no defined linear memory")
if any(maximum != expected_max for maximum in memory_maxima):
    raise SystemExit(
        f"error: {sys.argv[1]} memory maximum is {memory_maxima}; "
        f"expected {expected_max} pages"
    )
PY

    built_hash="$(shasum -a 256 "$built_wasm" | awk '{print $1}')"
    staged_wasm="$ASSETS_DIR/$plugin_id.wasm"
    staged_manifest="$ASSETS_DIR/$plugin_id.json"

    if [[ "$MODE" == "build" ]]; then
        python3 - "$source_manifest" "$built_hash" "$MAX_PAGES" <<'PY'
import json
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
manifest = json.loads(path.read_text())
manifest["max_pages"] = int(sys.argv[3])
manifest["wasm_hash"] = sys.argv[2]
path.write_text(json.dumps(manifest, indent=2, ensure_ascii=False) + "\n")
PY
        cp "$built_wasm" "$staged_wasm"
        cp "$source_manifest" "$staged_manifest"
        echo "    staged assets/plugins/$plugin_id.{wasm,json} ($built_hash)"
    else
        expected_hash="$(python3 - "$source_manifest" <<'PY'
import json
import pathlib
import sys

print(json.loads(pathlib.Path(sys.argv[1]).read_text()).get("wasm_hash", ""))
PY
)"
        if [[ "$expected_hash" != "$built_hash" ]]; then
            echo "error: $plugin_id manifest hash does not match fresh build" >&2
            exit 1
        fi
        if [[ ! -f "$staged_wasm" || ! -f "$staged_manifest" ]]; then
            echo "error: staged assets for $plugin_id are missing" >&2
            exit 1
        fi
        if ! cmp -s "$built_wasm" "$staged_wasm"; then
            echo "error: $staged_wasm is stale" >&2
            exit 1
        fi
        if ! cmp -s "$source_manifest" "$staged_manifest"; then
            echo "error: $staged_manifest is stale" >&2
            exit 1
        fi
        echo "    assets are current ($built_hash)"
    fi
done

if [[ "$plugin_count" -eq 0 ]]; then
    echo "error: no plugin crates found under $PLUGINS_DIR" >&2
    exit 1
fi

for staged_wasm in "$ASSETS_DIR"/*.wasm; do
    plugin_id="$(basename "$staged_wasm" .wasm)"
    if [[ ! -d "$PLUGINS_DIR/$plugin_id" || ! -f "$PLUGINS_DIR/$plugin_id.json" ]]; then
        echo "error: orphaned staged plugin asset $staged_wasm" >&2
        exit 1
    fi
done

if [[ "$MODE" == "build" ]]; then
    echo "Plugin assets built and staged successfully ($plugin_count total)."
else
    echo "Plugin assets verified successfully ($plugin_count total)."
fi
