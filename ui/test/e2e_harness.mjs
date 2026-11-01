// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The harness of e2e_panel.mjs: a sandbox folder, a private tmux server (`tmux -L tm-test-panel-<pid>`)
// with two shell panes in it, a private terminumd on a free port with its own TM_HOME and state
// folder, the sign-in through its CLI socket, and the check helpers. Nothing here touches the
// user's tmux server, sign-ins or workspaces.

import { execFileSync, spawn } from "node:child_process";
import fs from "node:fs";
import http from "node:http";
import path from "node:path";
import { freePort, own } from "../../web/test/testrun.mjs";
import { endOnSignal, stopChild } from "./process-stop.mjs";

// this run's: checks in several worktrees at once never meet (testrun.mjs)
export const PORT = await freePort();
export const TMUX_SOCKET = own("tm-test-panel");
const HERE = path.dirname(new URL(import.meta.url).pathname);
export const BIN = process.env.TM_BIN ?? path.resolve(HERE, "../../terminumd/target/debug/terminumd");
export const SB = fs.realpathSync(fs.mkdtempSync("/tmp/tm-e2e-")); // the sandbox: the only writable root
// short folders: the CLI socket's path must stay under 104 bytes
const HOME = fs.mkdtempSync("/tmp/tmh-");
const APP = fs.mkdtempSync("/tmp/tma-");
export const base = `http://127.0.0.1:${PORT}`;

/** terminumd's CLI socket: the password and health (`fetch` cannot use a Unix socket). */
export const SOCKET = path.join(APP, "terminumd.sock");
export const onSocket = (method, p, body) => new Promise((resolve, reject) => {
  const req = http.request({ socketPath: SOCKET, method, path: p, headers: { "Content-Type": "application/json" } }, (res) => {
    let text = ""; res.on("data", (d) => (text += d)); res.on("end", () => resolve({ status: res.statusCode, text }));
  });
  req.on("error", reject);
  req.end(body === undefined ? undefined : JSON.stringify(body));
});

// ── sandbox ──────────────────────────────────────────────────────────────────
fs.mkdirSync(`${SB}/src`); fs.mkdirSync(`${SB}/docs`);
fs.writeFileSync(`${SB}/README.md`, "# Sandbox\n\nSee [guide](docs/guide.md) and [web](https://example.com).\n\n![logo](docs/logo.png)\n\n<img src=x onerror=alert(1)>\n");
fs.writeFileSync(`${SB}/docs/guide.md`, "# Guide\n\nBack to [readme](../README.md#sandbox) or [the page](page.html#sec2).\n");
fs.writeFileSync(`${SB}/docs/page.html`, '<!doctype html><title>Page</title><h1>HTML page</h1><p><a href="../README.md#sandbox">to readme</a> <a href="guide.md">to guide</a></p><img src="logo.png"><img id="vec" src="mark.svg"><video id="clip" src="clip.html"></video><img src="http://leak.example.invalid/x.png"><img id="planted" data-raw="/etc/hosts"><link rel="stylesheet" data-raw="/etc/hosts"><meta name="referrer" content="unsafe-url"><style>img{background:url(http://leak.example.invalid/css)}</style><script>parent.document.title="PWNED"</script><div style="height:3000px"></div><p id="sec2">Section two</p>');
// what an HTML document may name to run script in the panel's origin, were it opened as a page (AC-09)
fs.writeFileSync(`${SB}/docs/mark.svg`, '<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4"><script>localStorage.setItem("pwned","svg")</script><rect width="4" height="4"/></svg>');
fs.writeFileSync(`${SB}/docs/clip.html`, '<!doctype html><script>localStorage.setItem("pwned","html")</script>');
// data files rendered (AC-94): a tree, a table, an error said
fs.mkdirSync(`${SB}/data`);
fs.writeFileSync(`${SB}/data/pkg.json`, JSON.stringify({ name: "demo", version: 2, private: true, none: null, deps: { a: "1.0", b: { c: [1, 2, 3] } }, list: ["x", "y"] }, null, 2));
fs.writeFileSync(`${SB}/data/conf.yaml`, "server:\n  port: 8080\n  hosts: [a, b]\n---\nsecond: yes\n");
fs.writeFileSync(`${SB}/data/table.csv`, 'name,count,note\nalpha,12,"with, comma"\nbeta,3.5,plain\n');
fs.writeFileSync(`${SB}/data/bad.json`, '{\n  "a": 1,\n  "b": }\n');
// sounds (AC-64): an AU Chrome does not play (decoded in the panel), a MIDI file terminumd renders with
// the SoundFont TM_TEST_SOUNDFONT names (linked into this TM_HOME), else none there
const au = Buffer.alloc(24 + 800, 0xff);
au.write(".snd", 0); au.writeUInt32BE(24, 4); au.writeUInt32BE(800, 8); au.writeUInt32BE(1, 12); au.writeUInt32BE(8000, 16); au.writeUInt32BE(1, 20);
for (let i = 0; i < 800; i++) au[24 + i] = i % 16 < 8 ? 0x20 : 0xa0;     // μ-law: a buzz, loud enough
fs.writeFileSync(`${SB}/docs/beep.au`, au);
const track = [0x00, 0xff, 0x51, 0x03, 0x07, 0xa1, 0x20, 0x00, 0x99, 36, 100, 0x60, 0x89, 36, 0, 0x00, 0x90, 60, 90, 0x83, 0x00, 0x80, 60, 0, 0x00, 0xff, 0x2f, 0x00];
fs.writeFileSync(`${SB}/docs/groove.mid`, Buffer.concat([Buffer.from("MThd\0\0\0\x06\0\0\0\x01\x01\xe0MTrk", "latin1"), Buffer.from([0, 0, 0, track.length]), Buffer.from(track)]));
export const SOUNDFONT = process.env.TM_TEST_SOUNDFONT ?? null;
if (SOUNDFONT) fs.symlinkSync(SOUNDFONT, `${HOME}/soundfont.sf2`);
// a PDF and a video for their tabs (AC-64): the browser's viewer and player
fs.writeFileSync(`${SB}/docs/spec.pdf`, "%PDF-1.4\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj 2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj 3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 200]>>endobj\ntrailer<</Root 1 0 R>>\n%%EOF\n");
fs.writeFileSync(`${SB}/docs/demo.mp4`, Buffer.alloc(64));
fs.writeFileSync(`${SB}/docs/diagram.md`, "# Diagrams\n\n```mermaid\nflowchart LR\n  A[\"terminumd\"] --> B[\"tmux\"]\n```\n\n```mermaid\nflowchart LR\n  A --> \n```\n\n```rust\nfn main() {}\n```\n\n```mermaid\nflowchart LR\n  H[\"<img src=x onerror=alert(1)>\"] --> J\n  click H call alert(2)\n```\n");
fs.writeFileSync(`${SB}/src/main.rs`, 'fn main() {\n    println!("hi");\n}\n');
fs.writeFileSync(`${SB}/app.py`, "print(1)\n");
for (const f of "abcde") fs.writeFileSync(`${SB}/${f}.log`, f);
for (const d of ["src/a/b", "node_modules/x", ".git/objects"]) fs.mkdirSync(`${SB}/${d}`, { recursive: true });
// a 1×1 PNG
fs.writeFileSync(`${SB}/docs/logo.png`, Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==", "base64"));

// ── the private tmux server: plain sh panes, so typed text is easy to read back ─
const TMUX = execFileSync("/bin/sh", ["-c", "command -v tmux"], { encoding: "utf8" }).trim();
const shellEnv = { PATH: "/usr/bin:/bin", HOME, TERM: "xterm-256color", LANG: "en_US.UTF-8", PS1: "$ ", ENV: "" };
export const tmux = (...args) => execFileSync(TMUX, ["-f", "/dev/null", "-L", TMUX_SOCKET, ...args], { env: shellEnv, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
const removeDirs = () => { for (const d of [SB, HOME, APP]) fs.rmSync(d, { recursive: true, force: true }); };
let panes;
try {
  try { tmux("kill-server"); } catch { /* none left over */ }
  tmux("new-session", "-d", "-s", "main", "-x", "120", "-y", "30", "-c", SB, "/bin/sh");
  tmux("new-window", "-t", "main", "-c", `${SB}/src`, "/bin/sh");
  panes = tmux("list-panes", "-s", "-t", "main", "-F", "#{pane_id}").trim().split("\n");
} catch (e) { removeDirs(); throw e; }
/** The state keys of the two panes (`tmux:<host>:<socket>:%N`): A in the sandbox, B in src. */
export const PANE_A = `tmux:local:${TMUX_SOCKET}:${panes[0]}`;
export const PANE_B = `tmux:local:${TMUX_SOCKET}:${panes[1]}`;
/** The panel's own workspace key of a pane (embed.ts pinKey). */
export const wsKey = (pane, host = "") => `web:${JSON.stringify([host, pane])}`;
/** What pane `pane` shows now (its typed line included). */
export const capture = (pane) => tmux("capture-pane", "-p", "-t", pane.split(":").pop());
/** Clears the pane's command line (typed text, never run). */
export const clearLine = (pane) => tmux("send-keys", "-t", pane.split(":").pop(), "C-u");
/** The panel pinned to `pane` in folder `cwd` (as the page names it in the frame's URL). */
export const panelUrl = (pane = PANE_A, cwd = SB, host = null) =>
  `${base}/fb/?${new URLSearchParams({ key: pane, cwd, ...(host ? { host } : {}) })}`;

// ── terminumd ────────────────────────────────────────────────────────────────
// terminumd plays the build the UI bundle was made for, or every panel would reload itself (AC-16)
const UI_BUILD = fs.readFileSync(path.resolve(HERE, "../.build-id"), "utf8").trim();
let server = null;
export const stopServer = async () => { if (server) await stopChild(server); };
/** Everything this harness started ends, once: from the run's `finally`, or from a signal (a
 *  timeout or Ctrl-C), which in Node skips every `finally`. */
export const cleanup = endOnSignal(async () => {
  await stopServer();
  try { tmux("kill-server"); } catch { /* gone */ }
  removeDirs();
});
async function start(env = {}) {
  // the child is noted at once: a signal while it starts still ends it
  server = spawn(BIN, [], {
    env: {
      ...process.env, TM_HOME: HOME, TM_APP_DIR: APP, TM_PORT: String(PORT), TM_TMUX_SOCKET: TMUX_SOCKET,
      TM_WRITABLE_ROOTS: SB, TM_LOG: "warn", TM_BUILD_ID: UI_BUILD, ...env,
    },
    stdio: ["ignore", "ignore", "inherit"],
  });
  for (let i = 0; i < 100; i++) {
    try { await fetch(base + "/"); if ((await onSocket("GET", "/health")).status === 200) break; } catch { /* starting */ }
    await new Promise((r) => setTimeout(r, 100));
  }
}
await start();
/** terminumd is back (`env`: as another build, TM_BUILD_ID). */
export async function bringBack(env = {}) { await start(env); }
/** terminumd goes away for `gapMs` (a restart); `during` runs meanwhile. */
export async function restartServer(gapMs, during = () => {}) {
  await stopServer();
  during();
  await new Promise((r) => setTimeout(r, gapMs));
  await bringBack();
}

/** Signs `page`'s browser context in as the password form does: a password set through the
 *  CLI socket (as `terminum password`), then POST /auth/login, whose answer is the sign-in; it
 *  goes into the origin's storage under "tm.session", where the page's signin.js puts it. Taken
 *  from the answer here, so the check does not depend on the page's scripts. Returns the secret. */
export const PASSWORD = "correct horse battery";
export async function signIn(page) {
  const set = await onSocket("POST", "/password", { password: PASSWORD });
  if (set.status !== 200) throw new Error(`no password: ${set.text}`);
  const r = await fetch(`${base}/auth/login`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ password: PASSWORD }) });
  const secret = (await r.json().catch(() => ({}))).secret;
  if (!/^[0-9a-f]{64}$/.test(secret ?? "")) throw new Error(`no sign-in from /auth/login (${r.status})`);
  await page.goto(`${base}/custom.css`);              // any document of the origin will do
  await page.evaluate((s) => localStorage.setItem("tm.session", s), secret);
  return secret;
}

export const results = [];
export const check = (name, ok, extra = "") => { results.push(ok); console.log(`${ok ? "PASS" : "FAIL"} ${name}${extra ? " — " + extra : ""}`); };
export const within = async (name, fn, ms = 5000) => {
  const t0 = Date.now();
  try { await fn(ms); check(name, true, `${Date.now() - t0} ms`); } catch (e) { check(name, false, e.message.split("\n")[0]); }
};

/** Polls `ok()` until it holds, else throws when `ms` passed. */
export async function waitFor(ok, ms = 4000) {
  for (const end = Date.now() + ms; !ok(); await new Promise((r) => setTimeout(r, 100))) {
    if (Date.now() > end) throw new Error("timed out");
  }
}

