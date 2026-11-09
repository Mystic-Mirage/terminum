// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// A fake terminumd for the page's checks of terminum's own UI (e2e_signin, e2e_tmux,
// e2e_screen, e2e_move): web/static served at 127.0.0.1 (a secure context, as the real one),
// /auth/login and /auth/logout, /api/hello's proof, /auth/check, /custom.css, and a
// WebSocket that answers `sub` with a theme, an empty history and a screen.

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const STATIC = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../static");
export const ORIGIN = "http://127.0.0.1:47999";
const TYPES = { html: "text/html", css: "text/css", svg: "image/svg+xml", js: "text/javascript" };
export const WIDE = { viewport: { width: 1400, height: 900 } };
export const PHONE = { viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true };
/** The sizes a check runs at: [label, newPage options]. */
export const SIZES = [["1400×900", WIDE], ["390×844", PHONE]];

export const THEME = { bg: "#1e1e1e", fg: "#d4d4d4", cursor: "#ffffff", cursorText: "#000000", bold: "#ffffff", useBold: false,
  ansi: Array.from({ length: 16 }, () => "#808080"), font: null, size: 13, dark: true };
export const item = (id, title, extra = {}) => ({ id, title, sub: ["zsh", "~/p"], index: "1", pane: 0, host: null, profile: "local",
  agent: null, shell: true, state: null, window: "@1", alt: false, ...extra });
export const group = (name, items, extra = {}) => ({ kind: "tmux", label: name, session: null, where: "", host: null,
  wid: extra.host ? `${extra.host}/terminum` : `local/${name}`,
  new: "New window", pool: null, items, ...extra });
export const LAYOUT = [group("main", [item("tmux:local:terminum:%1", "api", { sub: ["claude", "~/work/api"] }),
  item("tmux:local:terminum:%2", "docs", { index: "2" })]), group("notes", [item("tmux:local:terminum:%3", "notes")])];
export const line = (text, flags = 0) => ({ r: [[text, null, null, flags]], e: true });
export const SCREEN = [line("$ ls"), line("a.txt b.txt"), { r: [["$ ", null, null, 0], [" ", null, null, 256]], e: true }];

/** The proof terminumd gives for nonce n: HMAC(HMAC(secret, "tm-proof-key"), "tm-hello-v1:" + n). */
export function proofOf(secret, n) {
  const key = crypto.createHmac("sha256", secret).update("tm-proof-key").digest();
  return crypto.createHmac("sha256", key).update(`tm-hello-v1:${n}`).digest("hex");
}

/** The page's credential for boot b: "<id>.<HMAC(HMAC(secret, "tm-proof-key"), "tm-cred-v1:" + b)>". */
export function credentialOf(secret, b) {
  const key = crypto.createHmac("sha256", secret).update("tm-proof-key").digest();
  return `${crypto.createHash("sha256").update(secret).digest("hex").slice(0, 16)}.${crypto.createHmac("sha256", key).update(`tm-cred-v1:${b}`).digest("hex")}`;
}

/** Opens the page. secret: what the fake server knows (its proof and credential; "unknown_signin": none, it
 *  answers 404 unknown_signin); stored: a sign-in already in the origin's storage; auth:
 *  /auth/check's answer (204, or {login}); login(body): /auth/login's answer as [status, json];
 *  hook(msg, say): every message the page sends; onSub(msg, say): answers a "sub" in place of the
 *  screen (true: answered). */
export async function openPage(browser, { device = WIDE, secret = null, stored = null, auth = 204, login = null, layout = LAYOUT,
  screen = SCREEN, cols = 80, hook = null, onSub = null, url = "/", css = "", settings = null } = {}) {
  // reduced motion: boxes measured at once are at their place and size (e2e_motion.mjs checks the motion)
  const context = await browser.newContext({ reducedMotion: "reduce", ...device });
  const page = await context.newPage();
  const errors = [], sent = [], requests = [], sockets = [];
  // this fake terminumd's boot (a restart draws another: restart) and the credential it accepts
  let boot = crypto.randomBytes(32).toString("hex");
  const knows = (t) => typeof secret === "string" && t === credentialOf(secret, boot);
  page.on("pageerror", (e) => errors.push(String(e)));
  if (stored) await page.addInitScript((s) => { if (!sessionStorage.getItem("seeded")) { localStorage.setItem("tm.session", s); sessionStorage.setItem("seeded", "1"); } }, stored);
  await page.route(`${ORIGIN}/**`, (route) => {
    const req = route.request(), u = new URL(req.url()), p = u.pathname;
    requests.push({ path: p, query: u.search, headers: req.headers(), body: req.postData() ?? "" });
    if (p === "/auth/check") {
      const a = typeof auth === "function" ? auth() : auth;
      return typeof a === "number" ? route.fulfill({ status: a, body: "" })
        : route.fulfill({ status: 401, contentType: "application/json", body: JSON.stringify({ error: "bad_session", ...a }) });
    }
    if (p === "/auth/login" || p === "/auth/logout") {
      const [status, body] = p === "/auth/login" && login ? login(JSON.parse(req.postData() || "{}")) : [204, null];
      return route.fulfill({ status, contentType: "application/json", body: body ? JSON.stringify(body) : "" });
    }
    if (p === "/api/hello") {
      const n = u.searchParams.get("n");
      if (secret === "unknown_signin") return route.fulfill({ status: 404, contentType: "application/json", body: JSON.stringify({ error: "unknown_signin" }) });
      return route.fulfill({ contentType: "application/json", body: JSON.stringify({ proof: secret ? proofOf(secret, n) : "0".repeat(64), boot }) });
    }
    // a request with a credential of another boot is refused, as terminumd does (401)
    const sign = req.headers()["x-tm-session"];
    if (sign && !knows(sign)) return route.fulfill({ status: 401, contentType: "application/json", body: JSON.stringify({ error: "bad_session" }) });
    if (p === "/custom.css") return route.fulfill({ contentType: "text/css", body: typeof css === "function" ? css() : css });
    if (p.startsWith("/fb/")) return route.fulfill({ status: 404, body: "" });
    // the tasks made in the session shown (terminum-kanban.md AC-150): none here
    if (p === "/api/tasks" && u.searchParams.has("made_in")) return route.fulfill({ contentType: "application/json", body: JSON.stringify({ tasks: [] }) });
    const f = path.join(STATIC, p === "/" ? "index.html" : path.normalize(p));
    if (!f.startsWith(STATIC) || !fs.existsSync(f)) return route.fulfill({ status: 404, body: "" });
    let body = fs.readFileSync(f);
    if (f.endsWith("index.html")) body = body.toString().replace("__BUILD__", "test");
    return route.fulfill({ contentType: TYPES[f.split(".").pop()] ?? "application/octet-stream", body });
  });
  let say = () => {};
  await page.routeWebSocket(`${ORIGIN.replace("http", "ws")}/ws`, (ws) => {
    sockets.push({ protocols: ws.protocols(), close: () => ws.close() });
    // a server that knows a secret refuses a socket without its credential, as terminumd does (401)
    if (typeof secret === "string" && secret !== "unknown_signin" && !knows(ws.protocols()[1])) return void ws.close();
    say = (m) => ws.send(JSON.stringify(m));
    ws.onMessage((m) => {
      const msg = JSON.parse(String(m));
      sent.push(msg);
      if (msg.t === "sub" && !onSub?.(msg, (m) => ws.send(JSON.stringify(m)))) {
        say({ t: "theme", sid: msg.id, theme: THEME });
        say({ t: "hist", mode: "reset", sid: msg.id, first: 0, oldest: 0, truncated: false, lines: [] });
        say({ t: "screen", sid: msg.id, full: true, n: screen.length, cols, rows: screen.length, alt: false, ch: screen.map((l, i) => [i, l]) });
      }
      hook?.(msg, say);
    });
    say({ t: "build", id: "test", mac: true });   // the Mac's own browser, as at 127.0.0.1
    if (settings) say({ t: "settings", ...settings });
    say({ t: "layout", groups: layout });
  });
  await page.goto(ORIGIN + url);
  return { page, context, sent, requests, sockets, errors, credential: () => credentialOf(secret, boot), restart: () => { boot = crypto.randomBytes(32).toString("hex"); }, say: (m) => say(m), close: () => context.close() };
}

/** Presses a floating button; on a phone a folded one once ⟨…⟩ unfolds the column (fold.js, AC-18). */
export async function pressFab(page, sel, how = "click") {
  const folded = await page.evaluate((s) => document.body.classList.contains("folding") && !document.body.classList.contains("unfolded")
    && getComputedStyle(document.querySelector(s)).visibility === "hidden", sel);
  if (folded) { await page[how]("#foldfab"); await page.waitForTimeout(60); }
  await page[how](sel);
}

/** Presses a button of the list's foot; on a computer once the pointer unfolded it (foot.css, AC-07). */
export async function pressFoot(page, sel) {
  if (await page.isVisible("#foottoggle") && (await page.getAttribute("#foottoggle", "aria-expanded")) === "false") {
    await page.click("#foottoggle");   // the foot folded on a computer: unfolded by its toggle (foot.js)
    await page.waitForFunction(() => document.querySelector(".footmenu").getBoundingClientRect().height > 100, null, { timeout: 2000 });
  }
  await page.click(sel);
}

/** Folds the foot a pressFoot left unfolded (it stays so until its toggle is clicked again). */
export async function foldFoot(page) {
  if (await page.isVisible("#foottoggle") && (await page.getAttribute("#foottoggle", "aria-expanded")) === "true") await page.click("#foottoggle");
}

/** Waits until the page drew the shown pane's screen. */
export const drawn = (page) => page.waitForFunction(() => document.querySelectorAll("#screen .ln").length > 0, null, { timeout: 10000 });
export const pause = (page, ms = 150) => page.waitForTimeout(ms);
/** What the page puts on the clipboard from now on, in `window.copied`: by the clipboard API or
 *  the older copy command (an http page, touchcopy.js). */
export const spyCopies = (page) => page.evaluate(() => {
  window.copied = [];
  document.addEventListener("copy", (e) => window.copied.push(e.clipboardData?.getData("text/plain") || String(getSelection())), true);
  if (navigator.clipboard) navigator.clipboard.writeText = async (t) => { window.copied.push(t); };
});
