#!/usr/bin/env node
/**
 * M7 gate — divergence experiment.
 *
 * Same non-trivial coding prompt, N sequential runs, thinking on.
 * Streaming so you see progress; shorter prompt so flash finishes in
 * tens of seconds rather than hanging on a silent non-stream body.
 *
 * Usage:
 *   export DEEPSEEK_API_KEY=sk-…
 *   node scripts/divergence-experiment.mjs
 *
 * Optional env:
 *   N=3
 *   MODEL=deepseek-v4-flash
 *   OUT_DIR=scripts/divergence-out
 *   TIMEOUT_MS=180000   per-run abort (default 3 min)
 */

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const API_URL = "https://api.deepseek.com/chat/completions";
const MODEL = process.env.MODEL || "deepseek-v4-flash";
const N = Math.max(2, Number(process.env.N || 3) | 0);
const TIMEOUT_MS = Math.max(30_000, Number(process.env.TIMEOUT_MS || 180_000) | 0);
const OUT_DIR = path.resolve(
  process.env.OUT_DIR || path.join(__dirname, "divergence-out")
);

const SYSTEM = `You are a senior engineer. Reply with one short rationale
(≤5 sentences) then one fenced TypeScript code block. Commit to a single
design — no menus, no questions.`;

/** Smaller than v1 but still open to multiple strategies. */
const USER = `Implement a tiny in-memory rate limiter in TypeScript (one file,
no deps). Must support: fixed window OR token bucket (pick one), a
check(key) → allow/deny API, and automatic eviction of stale keys.
Ship one design.`;

function requireKey() {
  const key = process.env.DEEPSEEK_API_KEY || process.env.DEEPSEEK_KEY;
  if (!key) {
    console.error(
      "Set DEEPSEEK_API_KEY (BYOK). Refusing to read keys from the browser store."
    );
    process.exit(1);
  }
  return key;
}

/**
 * Stream one completion. Mirrors KernelOS: buffer bytes, split on \\n,
 * ignore `: keep-alive`, stop on [DONE].
 */
async function oneRun(key, runIndex) {
  const body = {
    model: MODEL,
    stream: true,
    stream_options: { include_usage: true },
    thinking: { type: "enabled" },
    messages: [
      { role: "system", content: SYSTEM },
      { role: "user", content: USER },
    ],
  };

  const t0 = Date.now();
  const ac = new AbortController();
  const timer = setTimeout(() => ac.abort(), TIMEOUT_MS);

  let res;
  try {
    res = await fetch(API_URL, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${key}`,
      },
      body: JSON.stringify(body),
      signal: ac.signal,
    });
  } catch (e) {
    clearTimeout(timer);
    if (e?.name === "AbortError") {
      throw new Error(`run ${runIndex} timed out after ${TIMEOUT_MS}ms`);
    }
    throw e;
  }

  if (!res.ok) {
    clearTimeout(timer);
    const raw = await res.text();
    throw new Error(`HTTP ${res.status}: ${raw.slice(0, 500)}`);
  }

  const reader = res.body.getReader();
  const decoder = new TextDecoder();
  let buf = new Uint8Array(0);
  let content = "";
  let reasoning = "";
  let usage = {};
  let ticks = 0;

  const append = (chunk) => {
    const merged = new Uint8Array(buf.length + chunk.length);
    merged.set(buf);
    merged.set(chunk, buf.length);
    buf = merged;
  };

  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      append(value);

      // Split on \n
      while (true) {
        const nl = buf.indexOf(0x0a);
        if (nl < 0) break;
        const lineBytes = buf.slice(0, nl);
        buf = buf.slice(nl + 1);
        let line = decoder.decode(lineBytes).replace(/\r$/, "");
        if (!line || line.startsWith(":")) continue;
        if (!line.startsWith("data:")) continue;
        const payload = line.slice(5).trimStart();
        if (payload === "[DONE]") continue;
        let json;
        try {
          json = JSON.parse(payload);
        } catch {
          continue;
        }
        const delta = json.choices?.[0]?.delta || {};
        if (typeof delta.reasoning_content === "string") {
          reasoning += delta.reasoning_content;
        }
        if (typeof delta.content === "string") {
          content += delta.content;
        }
        if (json.usage) usage = json.usage;

        ticks++;
        if (ticks % 8 === 0) {
          process.stdout.write(".");
        }
      }
    }
  } finally {
    clearTimeout(timer);
  }

  process.stdout.write(" ");
  return {
    run: runIndex,
    ms: Date.now() - t0,
    model: MODEL,
    content,
    reasoning_content: reasoning,
    usage: {
      prompt_tokens: usage.prompt_tokens,
      completion_tokens: usage.completion_tokens,
      prompt_cache_hit_tokens: usage.prompt_cache_hit_tokens,
      prompt_cache_miss_tokens: usage.prompt_cache_miss_tokens,
      total_tokens: usage.total_tokens,
    },
  };
}

function extractCode(content) {
  const fence = content.match(
    /```(?:typescript|ts|javascript|js)?\n([\s\S]*?)```/
  );
  return (fence ? fence[1] : content).trim();
}

function extractRationale(content) {
  return content.replace(/```[\s\S]*?```/g, "").trim().slice(0, 2000);
}

function fingerprints(code, rationale) {
  const text = `${code}\n${rationale}`.toLowerCase();
  const checks = {
    fixed_window: /fixed\s*window|window\s*size|per.?second|per.?minute/i.test(
      text
    ),
    token_bucket: /token\s*bucket|refill|tokens?\s*(?:left|remaining)/i.test(
      text
    ),
    sliding_window: /sliding\s*window/i.test(text),
    map_store: /\b(?:Map|WeakMap|Record|object)\b/.test(code),
    class_based: /\bclass\b/.test(code),
    functional: /export\s+(?:function|const)/.test(code),
    eviction_ttl: /ttl|expire|evict|stale|lastSeen|lastAccess/i.test(text),
    eviction_lru: /\blru\b|least.?recent/i.test(text),
  };
  return Object.fromEntries(Object.entries(checks).filter(([, v]) => v));
}

function jaccard(a, b) {
  const ta = new Set(a.toLowerCase().split(/\W+/).filter((w) => w.length > 3));
  const tb = new Set(b.toLowerCase().split(/\W+/).filter((w) => w.length > 3));
  if (!ta.size && !tb.size) return 1;
  let inter = 0;
  for (const t of ta) if (tb.has(t)) inter++;
  return inter / (ta.size + tb.size - inter);
}

function summarize(results) {
  const lines = [];
  lines.push(`# Divergence experiment — ${new Date().toISOString()}`);
  lines.push("");
  lines.push(`Model: \`${MODEL}\` · N=${results.length} · thinking + streaming`);
  lines.push("");
  lines.push("## Prompt");
  lines.push("");
  lines.push("```");
  lines.push(USER.trim());
  lines.push("```");
  lines.push("");
  lines.push("## Per-run");
  lines.push("");

  const codes = results.map((r) => extractCode(r.content));
  const rats = results.map((r) => extractRationale(r.content));
  const fps = codes.map((c, i) => fingerprints(c, rats[i]));

  for (let i = 0; i < results.length; i++) {
    const r = results[i];
    const hit = r.usage.prompt_cache_hit_tokens ?? "?";
    const miss = r.usage.prompt_cache_miss_tokens ?? "?";
    lines.push(`### Run ${i + 1}`);
    lines.push("");
    lines.push(
      `- ${r.ms} ms · tokens in/out ${r.usage.prompt_tokens}/${r.usage.completion_tokens} · cache hit/miss ${hit}/${miss}`
    );
    lines.push(`- code lines: ${codes[i].split("\n").length}`);
    lines.push(
      `- fingerprints: ${Object.keys(fps[i]).join(", ") || "(none matched)"}`
    );
    lines.push("");
    lines.push("Rationale excerpt:");
    lines.push("");
    lines.push("> " + rats[i].split("\n").join("\n> ").slice(0, 800));
    lines.push("");
  }

  lines.push("## Pairwise code Jaccard (token bag, crude)");
  lines.push("");
  for (let i = 0; i < codes.length; i++) {
    for (let j = i + 1; j < codes.length; j++) {
      lines.push(
        `- run${i + 1}↔run${j + 1}: ${(jaccard(codes[i], codes[j]) * 100).toFixed(1)}%`
      );
    }
  }
  lines.push("");

  const fpKeys = new Set(fps.flatMap((f) => Object.keys(f)));
  const shared = [...fpKeys].filter((k) => fps.every((f) => f[k]));
  const differing = [...fpKeys].filter((k) => !fps.every((f) => f[k]));
  lines.push("## Fingerprint overlap");
  lines.push("");
  lines.push(`- shared across all runs: ${shared.join(", ") || "(none)"}`);
  lines.push(
    `- present in some but not all: ${differing.join(", ") || "(none)"}`
  );
  lines.push("");
  lines.push("## Verdict (fill in by eye)");
  lines.push("");
  lines.push(
    "Heuristics cannot decide this. Open `run-*.md` and ask: would a developer"
  );
  lines.push(
    "pick different branches for different reasons, or are these the same"
  );
  lines.push("design with different names?");
  lines.push("");
  lines.push("- [ ] **Strategic divergence** — M7 premise holds; proceed.");
  lines.push(
    "- [ ] **Cosmetic only** — M7 three-way diff is weak; reconsider scope."
  );
  lines.push("- [ ] **Mixed** — note which axes differed.");
  lines.push("");
  return lines.join("\n");
}

async function main() {
  const key = requireKey();
  fs.mkdirSync(OUT_DIR, { recursive: true });
  // Clear prior partial runs
  for (const f of fs.readdirSync(OUT_DIR)) {
    fs.unlinkSync(path.join(OUT_DIR, f));
  }

  const results = [];
  console.log(
    `Running N=${N} streaming completions on ${MODEL} (timeout ${TIMEOUT_MS}ms)…`
  );
  for (let i = 1; i <= N; i++) {
    process.stdout.write(`  run ${i}/${N} `);
    const r = await oneRun(key, i);
    results.push(r);
    fs.writeFileSync(
      path.join(OUT_DIR, `run-${i}.md`),
      [
        `# Run ${i}`,
        "",
        "```json",
        JSON.stringify({ usage: r.usage, ms: r.ms, model: r.model }, null, 2),
        "```",
        "",
        "## Reasoning",
        "",
        r.reasoning_content || "(empty)",
        "",
        "## Content",
        "",
        r.content,
        "",
      ].join("\n")
    );
    console.log(
      `ok ${r.ms}ms cache hit=${r.usage.prompt_cache_hit_tokens ?? "?"}`
    );
  }

  const summary = summarize(results);
  const summaryPath = path.join(OUT_DIR, "SUMMARY.md");
  fs.writeFileSync(summaryPath, summary);
  console.log(`\nWrote ${summaryPath}`);
  console.log(summary.split("## Verdict")[0].trim());
  console.log("\nOpen SUMMARY.md and check a verdict box.");
}

main().catch((e) => {
  console.error("\n" + (e?.stack || e));
  process.exit(1);
});
