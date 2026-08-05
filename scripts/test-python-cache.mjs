#!/usr/bin/env node
/**
 * Integration tests for the Pyodide module-cache purge (PLAN M5 close-out).
 * Requires ./fetch-pyodide.sh to have populated assets/py/pyodide/.
 *
 *   node scripts/test-python-cache.mjs
 */
import { pathToFileURL } from "url";
import path from "path";
import { fileURLToPath } from "url";
import fs from "fs";
import vm from "vm";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(__dirname, "..");
const pyodideDir = path.join(root, "assets/py/pyodide");
const pythonJs = path.join(root, "assets/py/python.js");

if (!fs.existsSync(path.join(pyodideDir, "pyodide.js"))) {
  console.error("missing assets/py/pyodide — run ./fetch-pyodide.sh first");
  process.exit(1);
}

// Node load of pyodide via its own API (not the browser script injector).
const { loadPyodide } = await import(pathToFileURL(path.join(pyodideDir, "pyodide.mjs")).href);
// Node wants a filesystem path (with trailing slash), not a file:// URL —
// a file:// indexURL gets concatenated into `cwd + "file:/..."` and 404s.
const indexURL = pyodideDir.endsWith(path.sep) ? pyodideDir : pyodideDir + path.sep;
const api = await loadPyodide({ indexURL });

api.runPython("import sys\n_KO_BASELINE = frozenset(sys.modules)\n_KO_WORKDIRS = set()\n");

function purge() {
  api.runPython(
    "import sys, importlib\n" +
      "for _n in [n for n in list(sys.modules) if n not in _KO_BASELINE]:\n" +
      "    sys.modules.pop(_n, None)\n" +
      "importlib.invalidate_caches()\n"
  );
}

function writeTree(files) {
  for (const [p, body] of Object.entries(files)) {
    const parts = p.split("/");
    let cur = "";
    for (let i = 1; i < parts.length - 1; i++) {
      cur += "/" + parts[i];
      try {
        api.FS.mkdir(cur);
      } catch (_) {}
    }
    api.FS.writeFile(p, body);
  }
}

function runEntry(entry, workdir) {
  purge();
  api.runPython(
    `import sys
from io import StringIO
_ko_workdir = ${JSON.stringify(workdir)}
for _p in list(_KO_WORKDIRS):
    while _p in sys.path:
        sys.path.remove(_p)
_KO_WORKDIRS.clear()
_KO_WORKDIRS.add(_ko_workdir)
sys.path.insert(0, _ko_workdir)
_ko_real_stdout = sys.stdout
_ko_real_stderr = sys.stderr
_ko_stdout = StringIO()
_ko_stderr = StringIO()
sys.stdout = _ko_stdout
sys.stderr = _ko_stderr
_ko_status = 0
_ko_tb = ''
`
  );
  try {
    api.runPython(
      `import runpy, traceback
try:
    runpy.run_path(${JSON.stringify(entry)}, run_name='__main__')
except SystemExit as _ko_e:
    if isinstance(_ko_e.code, int):
        _ko_status = _ko_e.code
    elif _ko_e.code is None:
        _ko_status = 0
    else:
        _ko_status = 1
except Exception:
    _ko_tb = traceback.format_exc()
    _ko_status = 1
`
    );
  } finally {
    api.runPython("sys.stdout = _ko_real_stdout\nsys.stderr = _ko_real_stderr\n");
  }
  return String(api.runPython("_ko_stdout.getvalue()") || "");
}

// --- Test 1: edited helper is re-imported ---
writeTree({
  "/p/helper.py": "def double(n):\n    return n * 1\n",
  "/p/main.py": "from helper import double\nprint(double(21))\n",
});
const out1 = runEntry("/p/main.py", "/p").trim();
if (out1 !== "21") {
  console.error("FAIL edit#1 expected 21 got", JSON.stringify(out1));
  process.exit(1);
}
writeTree({
  "/p/helper.py": "def double(n):\n    return n * 100\n",
  "/p/main.py": "from helper import double\nprint(double(21))\n",
});
const out2 = runEntry("/p/main.py", "/p").trim();
if (out2 !== "2100") {
  console.error("FAIL edit#2 expected 2100 got", JSON.stringify(out2), "(stale cache?)");
  process.exit(1);
}
console.log("ok: edited helper re-imported (21 → 2100)");

// --- Test 2: same module name in different directories must not shadow ---
writeTree({
  "/a/helper.py": "def tag():\n    return 'A'\n",
  "/a/main.py": "from helper import tag\nprint(tag())\n",
  "/b/helper.py": "def tag():\n    return 'B'\n",
  "/b/main.py": "from helper import tag\nprint(tag())\n",
});
const a = runEntry("/a/main.py", "/a").trim();
const b = runEntry("/b/main.py", "/b").trim();
if (a !== "A" || b !== "B") {
  console.error("FAIL cross-dir shadowing: got", { a, b });
  process.exit(1);
}
console.log("ok: /a/helper.py and /b/helper.py do not shadow");

// --- Test 3: traceback trim helper (from python.js) ---
const sandbox = { window: {}, globalThis: {}, console };
sandbox.window = sandbox;
sandbox.globalThis = sandbox;
vm.runInNewContext(fs.readFileSync(pythonJs, "utf8"), sandbox);
const noisy =
  'Traceback (most recent call last):\n  File "<exec>", line 3, in <module>\n  File "<frozen runpy>", line 287, in run_path\n  File "/p/b.py", line 4, in <module>\n    boom()\nValueError: x';
const cleaned = sandbox.kernelosStripTracebackNoise(noisy, "/p/b.py");
if (cleaned.includes("<exec>") || cleaned.includes("<frozen runpy>")) {
  console.error("FAIL traceback noise still present:\n", cleaned);
  process.exit(1);
}
if (!cleaned.includes('File "/p/b.py"')) {
  console.error("FAIL entry frame missing:\n", cleaned);
  process.exit(1);
}
console.log("ok: traceback noise stripped");

console.log("all python cache tests passed");
