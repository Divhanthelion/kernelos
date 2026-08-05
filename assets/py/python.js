/**
 * KernelOS Python execution surface (PLAN M5b) via Pyodide.
 *
 * Exactly two globals:
 *   window.kernelosLoadPython() → Promise  (idempotent; rejection is not cached)
 *   window.kernelosRunPython(filesJson, entryPath) → String (JSON)  (sync)
 *
 * Pyodide lives under /py/pyodide/ (populated by ./fetch-pyodide.sh).
 * VFS file contents are always passed in from Rust — JS never reads localStorage.
 *
 * LIMITATION: execution is synchronous on the main thread. A `while True:`
 * (or any unbounded loop) from the agent will hang the tab. Close the tab to
 * recover. A Worker-based fix is PLAN §2a, not this milestone.
 */
(function (global) {
  "use strict";

  var PYODIDE_SCRIPT = "/py/pyodide/pyodide.js";
  var PYODIDE_INDEX = "/py/pyodide/";

  /** @type {Promise<void>|null} */
  var loadPromise = null;
  /** @type {any} */
  var pyodide = null;

  function injectScript(src) {
    return new Promise(function (resolve, reject) {
      if (typeof global.loadPyodide === "function") {
        resolve();
        return;
      }
      var existing = document.querySelector(
        'script[data-kernelos-py="' + src + '"]'
      );
      if (existing) {
        existing.addEventListener("load", function () {
          resolve();
        });
        existing.addEventListener("error", function () {
          reject(new Error("failed to load " + src));
        });
        return;
      }
      var s = document.createElement("script");
      s.src = src;
      s.async = false;
      s.setAttribute("data-kernelos-py", src);
      s.onload = function () {
        resolve();
      };
      s.onerror = function () {
        reject(new Error("failed to load " + src));
      };
      document.head.appendChild(s);
    });
  }

  /**
   * Idempotent while in-flight or successful. A rejected load clears the cache
   * so a later retry can succeed.
   * @returns {Promise<void>}
   */
  global.kernelosLoadPython = function kernelosLoadPython() {
    if (loadPromise) {
      return loadPromise;
    }
    loadPromise = injectScript(PYODIDE_SCRIPT)
      .then(function () {
        if (typeof global.loadPyodide !== "function") {
          throw new Error(
            "pyodide.js loaded but loadPyodide is missing — run ./fetch-pyodide.sh"
          );
        }
        return global.loadPyodide({ indexURL: PYODIDE_INDEX });
      })
      .then(function (api) {
        pyodide = api;
        // Snapshot modules present at load time. Before every run we evict
        // anything not in this set so edited helpers and same-named modules in
        // different directories re-import correctly. Do NOT filter by path —
        // Pyodide's stdlib spans /lib/pythonX.Y/ and /lib/pythonXY.zip/.
        api.runPython(
          "import sys\n" +
            "_KO_BASELINE = frozenset(sys.modules)\n" +
            "_KO_WORKDIRS = set()\n"
        );
      })
      .catch(function (err) {
        loadPromise = null;
        pyodide = null;
        throw err;
      });
    return loadPromise;
  };

  function normalizeSlashes(p) {
    return String(p).replace(/\\/g, "/");
  }

  function dirname(p) {
    var i = p.lastIndexOf("/");
    return i <= 0 ? "/" : p.slice(0, i);
  }

  function ensureDir(fs, path) {
    if (path === "/" || path === "") return;
    var parts = path.split("/");
    var cur = "";
    for (var i = 1; i < parts.length; i++) {
      cur += "/" + parts[i];
      try {
        fs.mkdir(cur);
      } catch (e) {
        /* exists */
      }
    }
  }

  /**
   * Drop <exec> / <frozen runpy> frames above the entry file — the agent only
   * needs its own source lines.
   * @param {string} tb
   * @param {string} entry
   * @returns {string}
   */
  function stripTracebackNoise(tb, entry) {
    if (!tb) return tb;
    var lines = tb.split("\n");
    var marker = 'File "' + entry + '"';
    var start = -1;
    for (var i = 0; i < lines.length; i++) {
      if (lines[i].indexOf(marker) !== -1) {
        start = i;
        break;
      }
    }
    if (start < 0) return tb;
    // Keep the "Traceback (most recent call last):" header if present.
    var header = [];
    if (lines.length && lines[0].indexOf("Traceback") === 0) {
      header.push(lines[0]);
    }
    return header.concat(lines.slice(start)).join("\n");
  }

  /**
   * Fully synchronous Python run.
   * @param {string} filesJson  JSON object of VFS path → source
   * @param {string} entryPath  absolute path of the script to execute
   * @returns {string} JSON: { output } or { error }
   */
  global.kernelosRunPython = function kernelosRunPython(filesJson, entryPath) {
    try {
      if (!pyodide) {
        return JSON.stringify({ error: "python not loaded" });
      }

      var files = JSON.parse(filesJson);
      var entry = normalizeSlashes(entryPath);
      if (typeof files !== "object" || files === null) {
        return JSON.stringify({ error: "files must be a JSON object" });
      }

      // Evict user modules imported by prior runs (baseline snapshot at load).
      pyodide.runPython(
        "import sys, importlib\n" +
          "for _n in [n for n in list(sys.modules) if n not in _KO_BASELINE]:\n" +
          "    sys.modules.pop(_n, None)\n" +
          "importlib.invalidate_caches()\n"
      );

      var FS = pyodide.FS;
      var name;
      for (name in files) {
        if (!Object.prototype.hasOwnProperty.call(files, name)) continue;
        var norm = normalizeSlashes(name);
        ensureDir(FS, dirname(norm));
        FS.writeFile(norm, String(files[name]));
      }

      var workdir = dirname(entry);
      // Put the entry's directory on sys.path so `import sibling` works.
      // Files written by Python do NOT land in the KernelOS VFS — stdout /
      // stderr / traceback only (out of scope for M5b).
      pyodide.runPython(
        "import sys\n" +
          "from io import StringIO\n" +
          "_ko_workdir = " +
          JSON.stringify(workdir) +
          "\n" +
          // Drop prior KernelOS workdirs so /p/helper.py cannot shadow /s/helper.py.
          "for _p in list(_KO_WORKDIRS):\n" +
          "    while _p in sys.path:\n" +
          "        sys.path.remove(_p)\n" +
          "_KO_WORKDIRS.clear()\n" +
          "_KO_WORKDIRS.add(_ko_workdir)\n" +
          "sys.path.insert(0, _ko_workdir)\n" +
          "_ko_real_stdout = sys.stdout\n" +
          "_ko_real_stderr = sys.stderr\n" +
          "_ko_stdout = StringIO()\n" +
          "_ko_stderr = StringIO()\n" +
          "sys.stdout = _ko_stdout\n" +
          "sys.stderr = _ko_stderr\n" +
          "_ko_status = 0\n" +
          "_ko_tb = ''\n"
      );

      try {
        pyodide.runPython(
          "import runpy, traceback\n" +
            "try:\n" +
            "    runpy.run_path(" +
            JSON.stringify(entry) +
            ", run_name='__main__')\n" +
            "except SystemExit as _ko_e:\n" +
            "    if isinstance(_ko_e.code, int):\n" +
            "        _ko_status = _ko_e.code\n" +
            "    elif _ko_e.code is None:\n" +
            "        _ko_status = 0\n" +
            "    else:\n" +
            "        _ko_status = 1\n" +
            "except Exception:\n" +
            "    _ko_tb = traceback.format_exc()\n" +
            "    _ko_status = 1\n"
        );
      } finally {
        pyodide.runPython(
          "sys.stdout = _ko_real_stdout\n" +
            "sys.stderr = _ko_real_stderr\n"
        );
      }

      var stdout = String(pyodide.runPython("_ko_stdout.getvalue()") || "");
      var stderr = String(pyodide.runPython("_ko_stderr.getvalue()") || "");
      var tb = String(pyodide.runPython("_ko_tb") || "");
      var status = Number(pyodide.runPython("_ko_status"));
      if (!isFinite(status)) status = 1;

      tb = stripTracebackNoise(tb, entry);

      var parts = [];
      if (stdout) {
        parts.push(stdout.replace(/\n$/, ""));
      }
      if (stderr) {
        parts.push("stderr:\n" + stderr.replace(/\n$/, ""));
      }
      if (tb) {
        parts.push(tb.replace(/\n$/, ""));
      }
      parts.push("[exit " + status + "]");
      var output = parts.join("\n");
      if (!stdout && !stderr && !tb && status === 0) {
        output = "[exit 0]";
      }

      return JSON.stringify({ output: output, status: status });
    } catch (e) {
      // Best-effort restore if we crashed mid-hijack.
      try {
        if (pyodide) {
          pyodide.runPython(
            "import sys\n" +
              "if '_ko_real_stdout' in dir():\n" +
              "    sys.stdout = _ko_real_stdout\n" +
              "if '_ko_real_stderr' in dir():\n" +
              "    sys.stderr = _ko_real_stderr\n"
          );
        }
      } catch (_ignore) { /* */ }
      var message = e && e.message ? e.message : String(e);
      return JSON.stringify({ error: message });
    }
  };

  // Exported for host/node tests of traceback trimming (not a load-bearing API).
  global.kernelosStripTracebackNoise = stripTracebackNoise;
})(typeof window !== "undefined" ? window : globalThis);
