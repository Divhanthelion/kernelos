# KernelOS — work queue

**Baseline:** `master` after M5 close-out commit (see DONE). Product direction:
`PLAN.md`. This file is the maintenance queue.

**Verify:**
```
cargo check --lib --target wasm32-unknown-unknown
cargo test --lib
(cd kernelos-pdk && cargo check --target wasm32-unknown-unknown)
(cd plugins/hello && cargo check --target wasm32-unknown-unknown)
```

`PLAN_WASI.md` / `PLAN_SMITHY.md` remain superseded. Pyodide: `./fetch-pyodide.sh`
(gitignored under `assets/py/pyodide/`).

---

## DONE

| | Note |
|---|---|
| P0–P3 plugin work | through `510346a` |
| PLAN M1–M4 | through `c2426d6` |
| Live API end-to-end | 2026-08-05 — `reasoning_content`, undo, 89% cache |
| PLAN M5a/M5b | TypeScript + Pyodide; module-cache baseline; see commit |
| HANDOFF B/C/D | empty Undo hidden; reproducible plugins; PDK `UnsafeCell` |
| HANDOFF E CSP | attempted, backed out — Trunk inline boot + Pyodide `eval(` |
| Divergence experiment | cosmetic only — identical-prompt N-way fanout dropped from M7 |

Single-file Python acceptance can look green with a *stale* module cache
(`run_path` re-executes the entry). Evidence for the cache fix is
`scripts/test-python-cache.mjs`, not a green agent run.

---

## OPEN — ordered

### 1 — M7a Named restore points (next)
Promote M4’s journal into named snapshots you can restore. Cap count/size;
prefer deltas. See PLAN §3a. Does not need OPFS.

### 2 — M7b User-directed forks
RAM forks with user-supplied divergence (different prompt / seeds / files);
diff vs trunk; keep / discard / cherry-pick. Persistent HAMT optional but
likely. Automatic identical-prompt fanout is **out**.

### 3 — M6 respec — ruff (optional)
`@astral-sh/ruff-wasm-web` as a tool — not a WASI host.

### 4 — Storage migration (§2a) — when quota bites
localStorage ~5 MiB UTF-16/origin. OPFS + colocated Worker; no COOP/COEP.

---

## Driving the UI from automation — corrected 2026-08-05

CDP `Input.insertText` updates Yew state; synthetic edit keys do not. Reload to
clear, then type once. xterm.js wall not retested.

---

## Notes on things that are already right — don't "fix" these

- `src/plugin/memory.rs` re-derives the `Uint8Array` view every access.
- `allow_vfs_path` normalizes then `FileSystem::is_inside`. Never `starts_with`.
- Capability gating by import **presence**.
- SSE parser: `Vec<u8>` + split on `b'\n'`. No `TextDecoder`.
- Tool schemas via `BTreeMap`; eight tools max; append only; `typecheck` then
  `run_python` last.
- Exactly one `run_agent_loop`. Echo `reasoning_content` on tool-call turns.
- Runtime loads (TS/Python) are **non-fatal**.
- `typecheck` / `run_python` take `&FileSystem`, no journal.
- No per-write permission prompts — Undo replaces gating.
- `wasm_store.rs` async IndexedDB is correct.
- `getrandom` enables `rand`'s `wasm_js` backend on wasm32.
