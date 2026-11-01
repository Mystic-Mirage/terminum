// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// AC-04 / OQ-05: Markdown rendering is safe and resolves local resources.
// Bundles src/markdown.ts for node (highlighting is DOM-only and not exercised here).
import assert from "node:assert/strict";
import { createHash, createHmac } from "node:crypto";
import { test } from "node:test";
import { load } from "./bundle.mjs";

// the loopback page's sign-in, as the page stored it (api.ts reads it once, at load)
globalThis.localStorage = { getItem: (k) => (k === "tm.session" ? "tok123" : null) };
const { mdToHtml, proveServer } = await load("test/markdown-entry.mjs");
const IMG = "![arch](docs/arch.png)";

test("no image address carries the sign-in, before or after terminumd proved itself (AC 09)", async () => {
  const before = mdToHtml(IMG, "/Users/alex/work/api/README.md");
  assert.ok(!before.includes("tok123") && !before.includes(" src="), before);
  // terminumd answers the proof for the panel's nonce and sign-in id (as auth/signin.rs does)
  const asked = [];
  globalThis.fetch = async (url) => {
    const q = new URL(url, "http://127.0.0.1").searchParams;
    asked.push(q.get("id"));
    const key = createHmac("sha256", "tok123").update("tm-proof-key").digest();
    const proof = createHmac("sha256", key).update(`tm-hello-v1:${q.get("n")}`).digest("hex");
    return { ok: true, status: 200, json: async () => ({ proof, boot: "b".repeat(64) }) };   // boot: what its credential is made with
  };
  await proveServer();
  assert.deepEqual(asked, [createHash("sha256").update("tok123").digest("hex").slice(0, 16)]);
  const after = mdToHtml(IMG, "/Users/alex/work/api/README.md");
  assert.ok(!after.includes("tok123") && !after.includes(" src="), "fetched with the header once shown (hydrateRaw), never an address with it");
});

const P = "/Users/alex/work/api/README.md";

test("tables, task lists, fenced code", () => {
  const h = mdToHtml("| a | b |\n|---|---|\n| 1 | 2 |\n\n- [x] done\n- [ ] todo\n\n```rust\nfn main() {}\n```\n", P);
  assert.match(h, /<table>[\s\S]*<td>1<\/td>/);
  assert.match(h, /checked="" disabled="" type="checkbox"> done/);
  assert.match(h, /<pre><code class="language-rust">fn main\(\) \{\}/);
});

test("raw HTML is escaped, not executed", () => {
  const h = mdToHtml('<img src=x onerror=alert(1)>\n\n<script>alert(2)</script>', P);
  assert.ok(!h.includes("<img src=x"), h);
  assert.ok(!h.includes("<script>"), h);
  assert.match(h, /&lt;img src=x onerror=alert\(1\)&gt;/);
});

test("javascript: links are dropped", () => {
  const h = mdToHtml("[x](javascript:alert(1))", P);
  assert.ok(!/href="javascript:/i.test(h), h);
});

test("relative images name their absolute path, filled from /api/raw once shown", () => {
  const h = mdToHtml("![arch](docs/arch.png)", P);
  assert.match(h, /<img data-raw="\/Users\/alex\/work\/api\/docs\/arch\.png" alt="arch">/);
});

test("remote images are not loaded", () => {
  const h = mdToHtml("![logo](https://example.com/logo.png)", P);
  assert.ok(!h.includes("<img"), h);
  assert.match(h, /class="remote-img"/);
});

test("encoded hash and question mark belong to the local image filename", () => {
  const h = mdToHtml("![arch](docs/a%23b%3Fc.png)", P);
  assert.match(h, /data-raw="\/Users\/alex\/work\/api\/docs\/a#b\?c\.png"/);
});

test("headings get anchors", () => {
  assert.match(mdToHtml("## Install & Run", P), /<h2 id="install--run">/);
});
