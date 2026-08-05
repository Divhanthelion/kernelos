/**
 * KernelOS TypeScript typecheck surface (PLAN M5a).
 *
 * Exactly two globals:
 *   window.kernelosLoadTypescript() → Promise  (idempotent)
 *   window.kernelosTypecheck(filesJson, rootsJson) → String  (sync)
 *
 * Lib .d.ts files live under /ts/ and are fetched once at load time.
 * VFS file contents are always passed in from Rust — JS never reads localStorage.
 */
(function (global) {
  "use strict";

  var TS_SCRIPT = "/ts/typescript.min.js";
  var LIB_BASE = "/ts/";

  // Closed reference set for target ES2020 + lib ["es2020","dom","dom.iterable"].
  var LIB_FILES = [
    "lib.decorators.d.ts",
    "lib.decorators.legacy.d.ts",
    "lib.dom.d.ts",
    "lib.dom.iterable.d.ts",
    "lib.es2015.collection.d.ts",
    "lib.es2015.core.d.ts",
    "lib.es2015.d.ts",
    "lib.es2015.generator.d.ts",
    "lib.es2015.iterable.d.ts",
    "lib.es2015.promise.d.ts",
    "lib.es2015.proxy.d.ts",
    "lib.es2015.reflect.d.ts",
    "lib.es2015.symbol.d.ts",
    "lib.es2015.symbol.wellknown.d.ts",
    "lib.es2016.array.include.d.ts",
    "lib.es2016.d.ts",
    "lib.es2016.intl.d.ts",
    "lib.es2017.arraybuffer.d.ts",
    "lib.es2017.d.ts",
    "lib.es2017.date.d.ts",
    "lib.es2017.intl.d.ts",
    "lib.es2017.object.d.ts",
    "lib.es2017.sharedmemory.d.ts",
    "lib.es2017.string.d.ts",
    "lib.es2017.typedarrays.d.ts",
    "lib.es2018.asyncgenerator.d.ts",
    "lib.es2018.asynciterable.d.ts",
    "lib.es2018.d.ts",
    "lib.es2018.intl.d.ts",
    "lib.es2018.promise.d.ts",
    "lib.es2018.regexp.d.ts",
    "lib.es2019.array.d.ts",
    "lib.es2019.d.ts",
    "lib.es2019.intl.d.ts",
    "lib.es2019.object.d.ts",
    "lib.es2019.string.d.ts",
    "lib.es2019.symbol.d.ts",
    "lib.es2020.bigint.d.ts",
    "lib.es2020.d.ts",
    "lib.es2020.date.d.ts",
    "lib.es2020.intl.d.ts",
    "lib.es2020.number.d.ts",
    "lib.es2020.promise.d.ts",
    "lib.es2020.sharedmemory.d.ts",
    "lib.es2020.string.d.ts",
    "lib.es2020.symbol.wellknown.d.ts",
    "lib.es5.d.ts",
  ];

  /** @type {Record<string, string>} */
  var libContents = Object.create(null);
  /** @type {Promise<void>|null} */
  var loadPromise = null;

  function injectScript(src) {
    return new Promise(function (resolve, reject) {
      if (global.ts) {
        resolve();
        return;
      }
      var existing = document.querySelector('script[data-kernelos-ts="' + src + '"]');
      if (existing) {
        existing.addEventListener("load", function () { resolve(); });
        existing.addEventListener("error", function () {
          reject(new Error("failed to load " + src));
        });
        return;
      }
      var s = document.createElement("script");
      s.src = src;
      s.async = false;
      s.setAttribute("data-kernelos-ts", src);
      s.onload = function () { resolve(); };
      s.onerror = function () { reject(new Error("failed to load " + src)); };
      document.head.appendChild(s);
    });
  }

  function fetchText(url) {
    return fetch(url).then(function (resp) {
      if (!resp.ok) {
        throw new Error("failed to fetch " + url + " (" + resp.status + ")");
      }
      return resp.text();
    });
  }

  /**
   * Idempotent while in-flight or successful. A rejected load clears the cache
   * so a later retry can succeed (transient network failures).
   * @returns {Promise<void>}
   */
  global.kernelosLoadTypescript = function kernelosLoadTypescript() {
    if (loadPromise) {
      return loadPromise;
    }
    loadPromise = injectScript(TS_SCRIPT).then(function () {
      if (!global.ts) {
        throw new Error("typescript.min.js loaded but window.ts is missing");
      }
      return Promise.all(
        LIB_FILES.map(function (name) {
          return fetchText(LIB_BASE + name).then(function (text) {
            libContents[name] = text;
          });
        })
      );
    }).then(function () {
      /* void */
    }).catch(function (err) {
      loadPromise = null;
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

  function joinPath(base, rel) {
    var parts = (base === "/" ? [] : base.split("/")).concat(rel.split("/"));
    var out = [];
    for (var i = 0; i < parts.length; i++) {
      var part = parts[i];
      if (!part || part === ".") continue;
      if (part === "..") {
        if (out.length) out.pop();
        continue;
      }
      out.push(part);
    }
    return "/" + out.join("/");
  }

  function tryResolveRelative(containingFile, moduleName, files) {
    var base = dirname(normalizeSlashes(containingFile));
    var candidate = joinPath(base, moduleName);
    var exts = ["", ".ts", ".tsx", ".d.ts", "/index.ts", "/index.tsx"];
    for (var i = 0; i < exts.length; i++) {
      var full = candidate + exts[i];
      if (Object.prototype.hasOwnProperty.call(files, full)) {
        return full;
      }
    }
    return undefined;
  }

  /**
   * Fully synchronous typecheck.
   * @param {string} filesJson  JSON object: { "/path/file.ts": "source...", ... }
   * @param {string} rootsJson  JSON array of root file paths
   * @returns {string} JSON string: { "output": "..." } or { "error": "..." }
   */
  global.kernelosTypecheck = function kernelosTypecheck(filesJson, rootsJson) {
    try {
      var ts = global.ts;
      if (!ts) {
        return JSON.stringify({ error: "typescript not loaded" });
      }

      var vfsFiles = JSON.parse(filesJson);
      var roots = JSON.parse(rootsJson);
      if (!Array.isArray(roots)) {
        return JSON.stringify({ error: "roots must be a JSON array" });
      }
      if (typeof vfsFiles !== "object" || vfsFiles === null) {
        return JSON.stringify({ error: "files must be a JSON object" });
      }

      /** @type {Record<string, string>} */
      var files = Object.create(null);
      var name;
      for (name in libContents) {
        if (Object.prototype.hasOwnProperty.call(libContents, name)) {
          files[name] = libContents[name];
        }
      }
      for (name in vfsFiles) {
        if (Object.prototype.hasOwnProperty.call(vfsFiles, name)) {
          files[normalizeSlashes(name)] = String(vfsFiles[name]);
        }
      }

      roots = roots.map(normalizeSlashes);

      if (roots.length === 0) {
        return JSON.stringify({
          output: "no TypeScript files found",
        });
      }

      var options = {
        noEmit: true,
        target: ts.ScriptTarget.ES2020,
        module: ts.ModuleKind.ESNext,
        moduleResolution: ts.ModuleResolutionKind.NodeJs,
        // Full filenames — short names like "es2020" are resolved as paths and
        // fail under a custom host that only serves lib.*.d.ts by basename.
        lib: ["lib.es2020.d.ts", "lib.dom.d.ts", "lib.dom.iterable.d.ts"],
        strict: true,
        skipLibCheck: true,
        allowJs: false,
      };

      var host = {
        getSourceFile: function (fileName, languageVersion) {
          var key = normalizeSlashes(fileName);
          // Strip leading ./ from lib lookups
          if (key.indexOf("lib.") === 0 || key.indexOf("/lib.") >= 0) {
            var base = key.split("/").pop();
            if (Object.prototype.hasOwnProperty.call(files, base)) {
              key = base;
            }
          }
          if (!Object.prototype.hasOwnProperty.call(files, key)) {
            return undefined;
          }
          return ts.createSourceFile(key, files[key], languageVersion, true);
        },
        getDefaultLibFileName: function () {
          return "lib.es2020.d.ts";
        },
        writeFile: function () { /* noEmit */ },
        getCurrentDirectory: function () {
          return "/";
        },
        getCanonicalFileName: function (fileName) {
          return normalizeSlashes(fileName);
        },
        useCaseSensitiveFileNames: function () {
          return true;
        },
        getNewLine: function () {
          return "\n";
        },
        fileExists: function (fileName) {
          var key = normalizeSlashes(fileName);
          if (Object.prototype.hasOwnProperty.call(files, key)) return true;
          var base = key.split("/").pop();
          return Object.prototype.hasOwnProperty.call(files, base);
        },
        readFile: function (fileName) {
          var key = normalizeSlashes(fileName);
          if (Object.prototype.hasOwnProperty.call(files, key)) return files[key];
          var base = key.split("/").pop();
          return files[base];
        },
        directoryExists: function () {
          return true;
        },
        getDirectories: function () {
          return [];
        },
        resolveModuleNames: function (moduleNames, containingFile) {
          return moduleNames.map(function (mod) {
            if (mod.charAt(0) === ".") {
              var resolved = tryResolveRelative(containingFile, mod, files);
              if (resolved) {
                return { resolvedFileName: resolved, isExternalLibraryImport: false };
              }
            }
            return undefined;
          });
        },
      };

      var program = ts.createProgram(roots, options, host);
      var diags = ts.getPreEmitDiagnostics(program);

      var lines = [];
      for (var i = 0; i < diags.length; i++) {
        var d = diags[i];
        var msg = ts.flattenDiagnosticMessageText(d.messageText, "\n");
        var code = typeof d.code === "number" ? d.code : 0;
        var category = d.category === ts.DiagnosticCategory.Error
          ? "error"
          : d.category === ts.DiagnosticCategory.Warning
            ? "warning"
            : "message";
        if (d.file && typeof d.start === "number") {
          var lc = d.file.getLineAndCharacterOfPosition(d.start);
          var line = lc.line + 1;
          var col = lc.character + 1;
          lines.push(
            d.file.fileName +
              ":" +
              line +
              ":" +
              col +
              " - " +
              category +
              " TS" +
              code +
              ": " +
              msg
          );
        } else {
          lines.push(category + " TS" + code + ": " + msg);
        }
      }

      var errorCount = 0;
      for (var j = 0; j < diags.length; j++) {
        if (diags[j].category === ts.DiagnosticCategory.Error) errorCount++;
      }

      var output;
      if (errorCount === 0 && lines.length === 0) {
        output = "no errors";
      } else {
        var summary =
          errorCount === 1 ? "1 error" : errorCount + " errors";
        output = lines.concat([summary]).join("\n");
      }

      return JSON.stringify({ output: output });
    } catch (e) {
      var message = e && e.message ? e.message : String(e);
      return JSON.stringify({ error: message });
    }
  };
})(typeof window !== "undefined" ? window : globalThis);
