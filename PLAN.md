# KernelOS — the plan

**Thesis:** the machine is a data structure — so do the things that only becomes
possible when it is.

The narrow version of this is safety. Every other agentic coding tool has to
*earn* trust — sandboxes, approval gates, diff-by-diff review — because the agent
is loose on a real machine with real keys and a real `rm -rf`. KernelOS doesn't
need to earn it: the agent's entire universe is a `HashMap` in a browser tab.
Worst case, you close the tab. That alone inverts the review burden — look at
the result and undo, rather than approving each write blind.

But safety is the *consequence*, not the point. The point is that **a filesystem
which is a value can be forked, compared, replayed and shipped** — and none of
those are affordable when the filesystem is a disk.

You cannot cheaply fork a developer's laptop. Forking a `HashMap` costs a clone.
So the things that are exotic infrastructure everywhere else — named restore
points, user-directed branch-and-compare, deterministic replay, mailing someone
your entire machine state — are *ordinary operations* here. That is the whole
bet, and it is a bet nobody with a real machine can call.

(Identical-prompt “best of N” agent fanout looked free too; a live divergence
experiment showed thinking mode converges cosmetically — so we supply
divergence via the user and via restore points, not via hoping the model
branches for us. See §3a.)


The same inversion runs through the whole project: what makes KernelOS "not a
real OS" is exactly what makes these possible.

**Supersedes** `PLAN_WASI.md`, `PLAN_SMITHY.md`, and the "run Debian" thread.
`HANDOFF.md` remains the maintenance queue.

---

## 1. What already exists and now becomes load-bearing

| Asset | Role in the product |
|---|---|
| VFS (`src/filesystem.rs`) | the agent's whole world — and why the blast radius is zero |
| Capability system (`Grant`, import-presence gating) | keeps the API key structurally unreachable |
| Plugin ABI + loader (`src/plugin/`) | runs checkers/tools compiled to wasm |
| Terminal, Text Editor, window manager | the surface the agent and user share |
| Session persistence | work survives reload |

None of this was built for this product. All of it serves it.

## 2. Evidence that shaped the plan

### Rust checking in the browser is a dead end for now

I previously recommended shipping rust-analyzer-wasm as a quick feedback loop.
**That was wrong** and this plan corrects it:

- `rust-analyzer/rust-analyzer-wasm` was **archived May 2025**, with no
  documented JS API. It never stabilised.
- `use-ink/ink-playground` (the one production RA-in-browser deployment) was
  **archived Feb 2024**, and critically it used a **backend** `crate_extractor`
  to parse the Cargo project into JSON. The browser half couldn't do it alone.
- **rust-analyzer does not borrow-check.** It works at HIR level and never
  lowers to MIR, where borrowck lives. That's structural, not a missing feature —
  so it cannot answer "will this compile" for exactly the error class Rust is
  famous for.

⚠️ The research also claimed "rustc can't compile to wasm" and "metadata is for
linking, not checking." Both are wrong and I verified otherwise directly:
`bjorn3/wasm-rustc` ships a 77 MB `rustc.wasm` for `wasm32-wasip1-threads`, and
`cargo check` *is* `--emit=metadata -Z no-codegen`. The rustc path is
**disqualified for this product** (threads → SAB → COI → Safari Browser dies),
not merely unproven — see M6.

### So lead with the languages that already work

This is the strategic pivot. Feedback fidelity by language, today:

| Language | In-browser checking | Fidelity |
|---|---|---|
| **TypeScript** | `tsc` **is JavaScript** — runs natively, no wasm | **Complete** |
| **Python** | Pyodide runs real CPython in wasm | **Complete — actual execution** |
| JS lint/bundle | `ruff` / `esbuild` ship **browser** packages (not WASI) | Good — wire as tools, no WASI host |
| **Rust** | RA dead + no borrowck; rustc unproven | **Weak** |

An agent that can typecheck converges. An agent writing blind is fancy
autocomplete. So the product ships strongest where the loop is genuinely closed —
TypeScript and Python — and treats Rust as the stretch goal it turned out to be.

### Agent loop constraints (verified against DeepSeek's docs)

- **`reasoning_content` must be echoed back verbatim** on any assistant turn
  carrying `tool_calls`. Dropping it causes silent loop death — the model returns
  `finish_reason: "stop"` mid-task. This has broken real harnesses. Highest-risk
  detail in the whole build.
- **Context caching is automatic and ~50x on cache hits**, prefix-based. This
  should drive context design: append-only history, byte-stable tool
  definitions (use `BTreeMap`/ordered structs — a `HashMap`-backed JSON
  serialization reorders keys and destroys the prefix), never rewrite the front
  of the array.
- **Tool calls occasionally leak into `content` as plain text** (~11%, correlated
  with 40+ tool definitions). Mitigate by keeping the tool count to ~6–8, using
  strict mode on the `/beta` endpoint, and adding a salvage parser.
- Sampling params are **silently ignored** when thinking mode is on (the
  default). `temperature: 0` does nothing unless thinking is disabled.
- ⚠️ Model names and pricing move fast — re-verify against
  api-docs.deepseek.com at implementation time rather than trusting this doc.

---

## 2a. Architecture: the Worker boundary is required, cross-origin isolation is not

This is still the cross-browser answer. The *reasoning* behind it shifted in
2026 (research Aug 2026); the *conclusion* did not. Update the PLAN rather than
resting on a claim that is now only half true.

### The problem already exists in this codebase

`src/plugin/imports.rs:95`:

```rust
let data = s.fs.borrow().read_file(&path).unwrap_or_default();
```

That is a host import called **from inside a guest `update()`**, returning bytes
before it returns. It is structurally identical to WASI's `fd_read`. It works
today for exactly one reason: **`localStorage` is the web's only synchronous
persistent storage API.** IndexedDB, OPFS-async and the Cache API are all async.

So moving off localStorage breaks this call path **whether or not we ever adopt
WASI**. Sync guest I/O widens the set of calls that must be synchronous.

### What changed in 2026 (partial obsolescence)

1. **JSPI (WebAssembly Promise Integration) has shipped** — Chrome/Edge 137+,
   Firefox 153+, Safari still Technology Preview. A main-thread wasm guest can
   *logically* block on an async Promise (OPFS `getFile()`, IndexedDB) without
   freezing the UI: the engine suspends the wasm stack. That partially
   obsoletees "sync guest reads force a Worker" **on Chromium and Firefox**.
2. **Document-Isolation-Policy (DIP)** shipped Chrome/Edge 137+. It grants
   `SharedArrayBuffer` per-document **without** COOP/COEP and without breaking
   third-party iframes. Unsupported on Safari and Firefox.
3. **`createSyncAccessHandle()` is still Worker-only** everywhere. OPFS
   directory rename is still unsupported everywhere — the flat-blob recommendation
   stands.

So Chromium-only products could reconsider split-thread + DIP, or main-thread +
JSPI. **This product must keep Safari.** Cross-browser, the colocated-Worker
answer still stands.

### Cross-origin isolation remains disqualifying here

Splitting runtime and storage across two threads (wasmer-js / CheerpX /
sqlite-`opfs` / Turso) still needs SAB + `Atomics.wait` without JSPI. For this
codebase that is **disqualifying, not a tradeoff**:

`src/components/browser.rs` frames third-party sites. `COEP: require-corp`
blocks embeds without CORP (Wikipedia, HN, example.com). `COEP: credentialless`
is Chrome/Firefox only — **Safari: not supported**. DIP does not help Safari.
Enabling cross-origin isolation on Safari deletes the Browser app.

### The resolution: colocate (unchanged)

Put the runtime **and** the OPFS sync handles **in the same dedicated Worker**.
The thread that blocks is the thread that owns the handles, so no SAB is needed
and no COOP/COEP is needed. This is what `opfs-sahpool` and wa-sqlite's
`OPFSCoopSyncVFS` proved in production.

Failure modes to design for (production libraries already do):
- **Handle exhaustion** — pre-allocate a fixed pool of opaque OPFS blobs; do not
  open one sync handle per VFS file.
- **Multi-tab lock contention** — exclusive locks on sync handles; decide
  single-writer or a coop protocol.
- **Async-open inside a sync region** — `createSyncAccessHandle()` returns a
  Promise; pre-acquire handles before entering the sync region (boot + already-
  async `plugin::instantiate()`).

### Storage shape (unchanged)

**OPFS only, owned end-to-end by one Worker.** Flat blob pool keyed by opaque id,
plus an in-RAM metadata tree journaled to a single always-open OPFS file, with
small file bodies inlined into the journal and large ones getting their own blob.

Rationale: OPFS filenames cannot contain `/`, and **no browser can rename a
directory** — so mirroring the VFS tree into OPFS directories would make
`FileSystem::rename()` an N-file copy. A flat pool makes rename a metadata edit.

Reject "metadata in IndexedDB, blobs in OPFS" — metadata reads are synchronous
too on the guest path, and IndexedDB has no sync API.

### Consequence for sequencing (revised Aug 2026)

**M7 (RAM-only forks) does not require this migration.** Forks that live only for
the duration of a parallel run, with only the winner written back to
localStorage, never multiply persisted quota. The Worker/OPFS migration is still
coming — a ~1.2 MB project already eats ~¼ of the 5 MiB UTF-16 quota with no
forking — but it is **not** a gate on M7.

**M6-as-WASI and any sync guest I/O beyond today's plugin reads** still want the
Worker boundary. Deferring the migration past that point becomes a rewrite.

Note: `src/plugin/wasm_store.rs` (IndexedDB via `rexie`) is **correct as-is** —
plugin binaries load at already-async `instantiate()`. The sync constraint
applies to VFS *content* reads from inside guest calls, not to binary loading.

## 3. Milestones

### M1 — Streaming transport ✅ DONE (`9e00a94`)

Shipped: `src/agent/{stream,accum,mod}.rs` + an "AI Agent" registry app. SSE
parser buffers `Vec<u8>` and splits on `b'\n'`; handles `: keep-alive` comments,
`[DONE]`, events split mid-byte, and malformed payloads (warn, continue).
`reasoning_content` retained for M3. API key in its own localStorage namespace,
unreachable from any VFS grant — asserted against the real `allow_vfs_path`.
401/402/429 render distinctly. Tests 37 → 46.

⚠️ **Unverified:** the model id `deepseek-chat` may be retired in favour of the
v4 line. Canned-byte tests prove the parser handles shapes we anticipated, not
that DeepSeek sends them. One prompt with a real key settles both.

<details><summary>original spec</summary>
SSE reader over `fetch` + `ReadableStream`, and a turn accumulator.

Add to `Cargo.toml` (all currently missing): `Request`, `RequestInit`,
`Response`, `Headers`, `ReadableStream`, `ReadableStreamDefaultReader`,
`AbortController`, `AbortSignal`.

Buffer `Vec<u8>` and split on `b'\n'` rather than using `TextDecoder` — a `0x0A`
byte can never occur inside a multi-byte UTF-8 sequence, so every complete line
is valid UTF-8 by construction. This sidesteps the split-codepoint bug entirely.

Handle: `: keep-alive` comment lines (DeepSeek sends them), `data: [DONE]` (not
JSON — check before parsing), and `stream_options.include_usage` for the
cost/cache telemetry chunk.

*Verify:* stream a completion into the terminal, token by token, with a working
stop button wired to `AbortController`.

</details>

### M2 — Tools over the VFS
Six tools, mapping near 1:1 onto existing `FileSystem` methods: `read_file`,
`write_file`, `list_directory`, `create_directory`, `delete`, `rename`.

Keep the count low — it's the primary mitigation for the plain-text-tool-call
bug. Truncate large tool results with an explicit marker; an unbounded
`read_file` will blow the context window.

*Verify:* the agent creates a file from a prompt and it appears in File Explorer.

### M3 — The loop
Multi-turn tool calling with the `reasoning_content` round-trip, an iteration cap
(~25), a repetition detector (same tool + same args 3x → break), and a live
token/cost display using the cache hit/miss split.

*Verify:* a task requiring 5+ sequential tool calls completes. **Write an
explicit test for the `reasoning_content` round-trip** — it's the thing that will
silently break.

### M4 — Undo instead of permission
No per-write confirmation. It buys nothing when the blast radius is zero, and
prompts that are always correct to approve train users not to read them.

Instead: copy-on-write journal of prior file contents (O(changes), not
O(filesystem)) and one prominent **Undo agent run** button. Every `write_file`
renders as a collapsed diff in the transcript with a per-file revert.

Note the VFS keeps content under a separate `kernelosv2_file:` prefix from the
metadata map — a snapshot must capture both.

*Still gate* the few things that genuinely escape the tab: network fetches,
clipboard writes, downloads, recursive delete above a threshold, and anything
touching `/system/config` or the key store. Short list, so users actually read
them.

*Verify:* agent makes 10 edits, one click reverts all of them.

### M5 — Real feedback loops ✅ DONE (uncommitted close-out Aug 2026)
The milestone that makes it useful rather than a toy.

- **TypeScript:** `tsc` in-browser; `typecheck` tool; full PLAN acceptance passed
  (agent writes type error → sees diagnostic → fixes without intervention).
- **Python:** Pyodide 0.27.7; `run_python` tool; module-cache baseline eviction
  so edited helpers and same-named modules across directories re-import correctly;
  traceback noise stripped; stdout/stderr restored per run.

*Verify (done):* TS type-error loop; Python import-edit re-run observes the
change; cross-dir `helper.py` does not shadow.

### M6 — Tool integrations *(respecified Aug 2026 — was "WASI layer")*

**Do not build a `wasi_snapshot_preview1` host as written.** Research (Aug 2026)
against the four tools PLAN named:

| Tool | Official wasm target | Preview-1 host? |
|---|---|---|
| **ruff** | `wasm32-unknown-unknown` + wasm-bindgen (`@astral-sh/ruff-wasm-web`) | **No** — takes source strings |
| **esbuild** | `GOOS=js GOARCH=wasm` + `wasm_exec.js` | **No** — not WASI |
| **uutils coreutils** | `wasm32-wasip1` | Yes |
| **rustc** (`bjorn3/wasm-rustc`) | `wasm32-wasip1-threads` → SAB → COI | **Disqualified** (deletes Browser on Safari) |

A preview-1 host buys **coreutils**. The two tools we actually wanted are
easier *without* WASI — ruff is an afternoon of JS bindings. rustc is dead for
the reason PLAN already suspected.

**Ship instead:** integrate ruff (and later esbuild) via their browser packages
as agent tools, same pattern as `typecheck` / `run_python`. Keep the empirical
WASI notes below as reference if a future tool *only* ships wasip1 (uutils) —
they remain correct (`path_open` 9-arg trampoline, 12→13 imports post-1.94,
preopen `"/"` from fd 3). Estimated preview-1 host: ~10–15 engineer-days for
coreutils alone; do not spend that unless a concrete wasip1-only tool is worth
it.

<details><summary>WASI preview-1 host notes (deferred reference)</summary>

**⚠️ `path_open` cannot be a `Closure`.** 9 params, two `i64`. wasm-bindgen
0.2.126 caps `Closure` at 8 args — JS trampoline required.

**⚠️ "12 imports" is toolchain-pinned.** Post rust `ba462864f` (≥1.94): 13
imports (`fd_fdstat_get`, `fd_seek`). Implement both up front.

**Preopen:** libc probes from fd 3; root must be named `"/"`. Silent
`exit(71)` / hang forever are the failure modes for wrong probe errnos.

**Entry/exit:** call `_start`; clean `main` return does not call `proc_exit` —
use exception identity for exit codes.

`GuestMemory` in `src/plugin/memory.rs` is reusable. Reference:
`bjorn3/browser_wasi_shim`.

</details>

---

## 3a. The ambitious half

M1–M5 build a competent single-agent coding environment with closed feedback
loops. That is table stakes. These are the milestones that are only cheap
*here*, and they are the reason to build this at all.

### Divergence experiment — ran 2026-08-05, closed the old M7 pitch

N=3 identical prompts on `deepseek-v4-flash` with thinking on
(`scripts/divergence-experiment.mjs`). **Verdict: cosmetic only.** All three
shipped the same strategy (token bucket + Map + TTL); Jaccard ~30–50% was
rename noise. Thinking mode alone does **not** produce usefully different
designs.

So we will **not** build “run the same agent task N ways and pick a strategy”
as a default feature. The filesystem-as-value thesis still holds — the product
just has to supply divergence differently.

### M7 — Named restore points, then user-directed forks

M4’s copy-on-write journal already snapshots prior file contents. Promote that
into first-class **named restore points**, then let the user **fork** the VFS
to run deliberately different experiments.

**M7a — Named restore points** ✅ (2026-08-05)

- After an agent run (or on demand), save a named snapshot of trunk:
  every path as `PathState` (`src/agent/restore.rs`). Cap: 5 points /
  512k serialized chars; oldest dropped first. Key:
  `kernelosv2_restore_points` (outside the VFS).
- UI: Save restore point / Restore / Delete in the Agent app. Undo agent
  run remains the fast path for the last run; named points are for “go
  back to before I tried X.” Restoring clears the run journal.
- Persist in localStorage sparingly. Full snapshots for v1 (not relative
  deltas yet) — still one trunk on disk plus the capped point list.

*Verify:* unit tests round-trip trunk through save→mutate→restore; Agent
UI save/restore/delete wired.

**M7b — User-directed forks** ✅ (2026-08-05)

- User forks trunk into a RAM branch (`FileSystem::fork_ephemeral` —
  `persist: false`, hydrated `contents`), edits the prompt, runs the agent
  against that branch, diffs vs trunk, then **promotes all / cherry-picks /
  discards**.
- Divergence comes from the **user**. Branches are session-scoped; reload
  drops them. Trunk + named restore points remain the only persisted state.
- Clone cost is O(filesystem) for v1 (no HAMT yet) — fine while trunks stay
  small.

*Verify:* unit tests for isolation, diff, promote-all, cherry-pick; Agent UI
Fork / workspace switch / Diff vs trunk / Promote / Discard.

**Explicitly out of M7:** automatic N-way identical-prompt parallel runs and
a three-way “pick a strategy” UI. Revisit only with evidence that a prompt
class actually diverges usefully.

### M8 — Parallel agents as an OS metaphor
You already have a window manager, a per-window plugin instance model, and
per-window state. Give each agent window **its own forked VFS** (M7b) and let
several run at once — each with its own brief, not clones of one prompt.

That is a genuine use of the desktop metaphor: windows become processes,
forks become address spaces, the taskbar a process list.

*Verify:* three agent windows on isolated forks with different tasks;
closing one discards its branch without touching the others.

### M9 — Self-extension
The agent writes a KernelOS plugin, it compiles in-browser, `pkg install` picks
it up, and **an icon appears on the desktop**. The system extends itself while
running.

Every piece exists: plugin ABI, install path with consent, capability grants, and
an agent that can write files. The missing link is an in-browser compiler for
*some* language targeting the ABI — AssemblyScript (its compiler is JavaScript)
is the cheap route; a hand-rolled wasm emitter is the ambitious one.

*Verify:* "write me a stopwatch app" produces a working desktop icon in one
session, with the capability prompt appearing for whatever it requests.

### M10 — Ship the machine, not the code
Serialize the whole VFS — files, installed plugins, agent transcript, open
windows — into a shareable artifact. A URL or a file that boots someone else
into your exact state.

Not "here's my repo, good luck with setup." The actual machine, running, on a
Chromebook, in a tab. This is only possible because the machine is a value; it
is the payoff of every structural decision above it.

*Verify:* export from one browser profile, import into another, and land in an
identical desktop with history intact.

### Sequencing honesty

Ordered by dependency, not appetite. **M7a** needs M4’s journal (shipped).
**M7b** needs M7a’s snapshot model (or an equivalent detachable `FileSystem`).
M8 needs M7b’s forks. M9 needs M5’s compile loop (shipped) plus an in-browser
ABI compiler. M10 needs all of it plus §2a OPFS when trunk outgrows
localStorage.

**Next:** M7a (named restore points). M6-as-ruff is optional. WASI host and
automatic N-way fanout are off the path. Worker/OPFS is quota-driven.

## 4. Security

The blast radius is zero **except** for the API key. If the agent can read the
VFS and the key is in the VFS, the agent can exfiltrate its own credentials.
That's the one real hole.

1. **Key never enters the VFS.** Separate store; no `VfsRead` prefix reaches it.
   Plugins must not receive a capability for it — given the capability-escape
   bug already fixed once, write an explicit test.
2. **CSP in `index.html`** (attempted Aug 2026, **backed out**): target policy
   cannot ship without material weakening. Trunk injects an inline
   `<script type="module">` bootstrap (needs `'unsafe-inline'` or per-build
   hashes); Pyodide's `pyodide.asm.js` uses `eval(` for ASM_CONSTS (needs
   `'unsafe-eval'`). Do not silently add either. No COOP/COEP. Revisit when
   Trunk externalizes its bootloader and/or Pyodide drops eval — until then
   `connect-src` remains the structural goal, not a shipped meta tag.
   ```
   default-src 'self'; script-src 'self' 'wasm-unsafe-eval';
   connect-src 'self' https://api.deepseek.com; object-src 'none'; base-uri 'none'
   ```
   Product necessities if revived: `style-src 'unsafe-inline'`, `frame-src https:
   http:`, `img-src 'self' data: blob:`.
3. **BYOK is the correct model** and is what comparable tools do. The alternative
   — proxying through a server — is strictly worse for the user and creates a
   honeypot. Optional hardening: encrypt the key with a **non-extractable**
   WebCrypto `CryptoKey` in IndexedDB, which downgrades permanent key theft to
   session-scoped abuse.
4. Agent output is untrusted text. **Never render it as HTML.**

---

## 5. Open questions — research before the milestone that needs them

- **Restore-point storage format.** Deltas vs full tree blobs under a size/count
  cap. Must not multiply the 5 MiB UTF-16 localStorage quota the way naive
  N-fork persistence would.
- **Storage migration timing.** OPFS + colocated Worker (§2a) when trunk +
  restore points need it. JSPI/DIP help Chromium only — Safari still forces
  the Worker design. No COOP/COEP.
- **WASI host** — deferred; only for a concrete wasip1-only tool worth ~10–15d.
- **`rustc.wasm`** — disqualified (`wasip1-threads` → SAB → Safari Browser dies).
- **When (if ever) identical-prompt fanout is worth it** — only with a prompt
  class that has failed the cosmetic-only test the other way. Not assumed.

## 6. Non-goals

- Linux compatibility, emulation, CheerpX, Debian
- Porting the Smithy IDE — its thesis is native, and stripping that leaves
  something worse
- Smithy-the-language — a language nobody writes doesn't help an agent write code
- Being "an OS" for its own sake. If a feature only makes it feel more
  OS-like, it's out.
