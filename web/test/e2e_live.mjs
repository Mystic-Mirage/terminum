// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The page against a real private terminumd and a real private tmux server (never the
// user's): sign-in with the password, the list following tmux, a pane's screen, typing,
// sizing for the view, a full-screen program as its grid and what it copies, Files docked beside the
// terminal, custom.css, settings and themes changed live, and a terminumd restart.
// Chromium and WebKit, at a wide (1400×900) and a phone (390×844) size.
//
//   node web/test/e2e_live.mjs [chromium|webkit]
//     (needs terminumd built: cd terminumd && cargo build; and ui built: cd ui && npm run build)

import { execFileSync, spawn } from "node:child_process";
import fs from "node:fs";
import http from "node:http";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { check, playwright, results, runningIn, within } from "./check.mjs";
import { freePort, own } from "./testrun.mjs";
import { endOnSignal, NO_SIGNALS, stopChild } from "../../ui/test/process-stop.mjs";
import { liveDrop, livePaste } from "./e2e_livepaste.mjs";
import { liveQuick } from "./e2e_livequick.mjs";
import { liveSplit } from "./e2e_livesplit.mjs";
import { liveView } from "./e2e_liveview.mjs";
import { liveFolds } from "./e2e_livefolds.mjs";
import { liveJitter } from "./e2e_livejitter.mjs";
import { liveLinks } from "./e2e_livelinks.mjs";
import { liveLimit } from "./e2e_livelimit.mjs";
import { liveFocus } from "./e2e_livefocus.mjs";
import { liveListPlace } from "./e2e_livelistplace.mjs";
import { liveClip } from "./e2e_liveclip.mjs";
import { livePrograms } from "./e2e_liveprogs.mjs";
import { liveKeys } from "./e2e_livekeys.mjs";
import { livePlans } from "./e2e_liveplans.mjs";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const BIN = process.env.TERMINUMD_BIN ?? path.join(ROOT, "terminumd/target/debug/terminumd");
// this run's: checks in several worktrees at once never meet, and purge() ends only this run's server
const PORT = await freePort(), SOCKET = own("tm-test-live");
const BASE = `http://127.0.0.1:${PORT}`;
const tmp = fs.mkdtempSync("/tmp/tml-");
const HOME = path.join(tmp, "h"), APP = path.join(tmp, "a"), WORK = fs.realpathSync(fs.mkdtempSync("/tmp/tmlw-"));
fs.mkdirSync(HOME);
for (const f of ["alpha.txt", "beta.md"]) fs.writeFileSync(path.join(WORK, f), `${f}\n`);
// macOS's drag pasteboard played by a stub (dragged.rs runs TM_OSASCRIPT): it answers drag.json, else no paths; the user's own drag is never read
const DRAG = path.join(tmp, "drag.json"), OSASCRIPT = path.join(tmp, "osascript");
fs.writeFileSync(OSASCRIPT, `#!/bin/sh\ncat '${DRAG}' 2>/dev/null || echo '[]'\n`, { mode: 0o755 });
const UI_BUILD = fs.readFileSync(path.join(ROOT, "ui/.build-id"), "utf8").trim();
const tmux = (...a) => { try { return execFileSync("tmux", ["-L", SOCKET, ...a], { encoding: "utf8", timeout: 5000 }).trim(); } catch { return ""; } };   // a hung server answers nothing, never holds the run
/** The pids of this test's tmux server and terminumd's control clients, noted while they answer:
    a server that hangs does not take kill-server, so it is ended by them (never by a name or pattern). */
const ours = new Set();
const note = () => {
  for (const p of [tmux("display", "-p", "#{pid}"), ...tmux("list-clients", "-F", "#{client_pid}").split("\n")]) if (Number(p) > 0) ours.add(Number(p));
};
const ps = (pid) => { try { return execFileSync("ps", ["-o", "command=", "-p", String(pid)], { encoding: "utf8" }); } catch { return ""; } };
const nap = (ms) => Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);
/** This test's tmux server gone (after terminumd stopped: running, it would start it again); one
    left after kill-server is ended by its noted pid, said aloud, and fails the run if it stays. */
const purge = () => {
  note();
  tmux("kill-server");
  for (const pid of ours) {
    const ourTmux = () => ps(pid).includes(`-L ${SOCKET}`);   // still ours, not a reused pid
    for (let i = 0; i < 20 && ourTmux(); i++) nap(100);
    for (const sig of ["SIGTERM", "SIGKILL"]) {
      if (!ourTmux()) break;
      console.error(`purge: tmux pid ${pid} (-L ${SOCKET}) did not end on kill-server; sending ${sig}`);
      try { process.kill(pid, sig); } catch {}
      for (let i = 0; i < 20 && ourTmux(); i++) nap(100);
    }
    if (ourTmux()) check(`this test's tmux pid ${pid} ended`, false, ps(pid).trim());
  }
  ours.clear();
};

let proc = null;
const onSocket = (method, p, body) => new Promise((resolve, reject) => {
  const req = http.request({ socketPath: path.join(APP, "terminumd.sock"), method, path: p, headers: { "Content-Type": "application/json" } }, (res) => {
    let t = ""; res.on("data", (d) => (t += d)); res.on("end", () => resolve(JSON.parse(t || "null")));
  });
  req.on("error", reject);
  req.end(body === undefined ? undefined : JSON.stringify(body));
});
let ended = false;
async function start() {
  if (ended) throw new Error("the run was ended");   // a signal ended it while a browser ran
  // as the build the Files panel was bundled for: another one makes the panel reload itself (AC-34)
  const env = { ...process.env, TM_HOME: HOME, TM_APP_DIR: APP, TM_PORT: String(PORT), TM_TMUX_SOCKET: SOCKET, TM_LOG: "warn", TM_BUILD_ID: UI_BUILD, TM_OSASCRIPT: OSASCRIPT };
  delete env.TMUX;
  proc = spawn(BIN, [], { env, stdio: ["ignore", "ignore", "inherit"] });
  for (let i = 0; i < 100; i++) {
    try { await onSocket("GET", "/health"); } catch { await new Promise((r) => setTimeout(r, 100)); continue; }
    for (let j = 0; j < 30 && !tmux("list-clients"); j++) await new Promise((r) => setTimeout(r, 100));   // its control clients attached
    note();
    return;
  }
  throw new Error("terminumd did not start");
}
const stop = () => (proc ? stopChild(proc) : Promise.resolve());   // SIGTERM, then SIGKILL
/** Everything this run started ends, once: from its `finally`, or from a signal (a timeout or
 *  Ctrl-C), which in Node skips every `finally`. */
const teardown = endOnSignal(async () => {
  ended = true;
  runningIn("");
  await stop();
  purge();
  fs.rmSync(tmp, { recursive: true, force: true });
  fs.rmSync(WORK, { recursive: true, force: true });
});
// The password, set once as `terminum password` does (a new one would sign every page out).
const PASSWORD = "correct horse battery";
let passwordSet = false;
/** Signs in as a person does: the page's password form. */
const signIn = async (page) => {
  if (!passwordSet) passwordSet = (await onSocket("POST", "/password", { password: PASSWORD }))?.ok === true;
  await page.goto(`${BASE}/`);
  await page.waitForSelector("#login:not([hidden])", { timeout: 10000 });
  await page.fill("#password", PASSWORD);
  await page.press("#password", "Enter");
  // a debug build checks the password at PBKDF2's 600,000 rounds in about a second, longer on a busy Mac
  await page.waitForFunction(() => document.querySelectorAll("#sessions .item").length > 0, null, { timeout: 30000 });
};
const screenText = (page) => page.evaluate(() => document.getElementById("screen").innerText);

async function wide(browser, name) {
  runningIn(`${name} 1400×900`);
  const ctx = await browser.newContext({ viewport: { width: 1400, height: 900 } });
  const page = await ctx.newPage();
  const errors = [];
  // WebKit reports a load that failed as the page's error, "Fetch API cannot load … due to access control
  // checks", though the code caught it (the Panel's reads while terminumd restarts: AC-14, AC-34); said, not counted
  page.on("pageerror", (e) => {
    const said = String(e.stack ?? e);
    if (/^(Error: )?Fetch API cannot load \S+ due to access control checks\.?$/.test(String(e.message ?? said))) console.log(`NOTE [${name}] WebKit's own report of a failed load: ${e.message}`);
    else errors.push(said);
  });
  await signIn(page);
  check("AC-09 the page signs in with the password; its address holds no secret", new URL(page.url()).pathname === "/" && !new URL(page.url()).search);
  check("AC-09 the sign-in is kept in the origin's storage", await page.evaluate(() => /^[0-9a-f]{64}$/.test(localStorage.getItem("tm.session") ?? "")));
  // a page that sent its secret would be refused at once, not leak it unseen (the browser sends Sec-Fetch-Site)
  // (an XMLHttpRequest: signin.js puts the credential on every fetch)
  const bySecret = await page.evaluate(async () => {
    const s = localStorage.getItem("tm.session");
    const r = await new Promise((done) => { const x = new XMLHttpRequest(); x.open("GET", "/fb/api/prefs"); x.setRequestHeader("X-TM-Session", s); x.onloadend = () => done(x.status); x.send(); });
    const ws = await new Promise((done) => { const w = new WebSocket(`ws://${location.host}/ws`, ["tm", s]); w.onopen = () => { w.close(); done("open"); }; w.onerror = w.onclose = () => done("refused"); });
    return { r, ws };
  });
  // Chromium sends no Sec-Fetch-Site on a WebSocket's upgrade, WebKit does (that the page never puts the secret there: e2e_signin.mjs)
  check(`AC-09 terminumd refuses the secret itself from the browser: a request with it 401${name === "webkit" ? ", a WebSocket with it never opens" : ""}`,
    bySecret.r === 401 && (name !== "webkit" || bySecret.ws === "refused"), JSON.stringify(bySecret));
  check("AC-01 the list shows the session main", (await page.textContent("#sessions")).includes("main"));
  await within("AC-02 the shown pane's screen is drawn", () => page.waitForFunction(() => document.querySelectorAll("#screen .ln").length > 3, null, { timeout: 5000 }));
  await page.keyboard.type(`cd ${WORK} && echo live-$((6*7))`);
  await page.keyboard.press("Enter");
  await within("AC-03 typed keys reach the pane and echo", () => page.waitForFunction(() => document.getElementById("screen").innerText.includes("live-42"), null, { timeout: 5000 }));
  await livePaste(page, { home: HOME, name });
  // a drop's own paths come from Finder's drag pasteboard (dragged.rs): macOS only, elsewhere terminumd says so
  if (process.platform === "darwin") await liveDrop(page, { home: HOME, work: WORK, drag: DRAG });
  else console.log("SKIP AC-36 files dropped from Finder: their own paths are found on macOS only (dragged.rs)");
  const size = tmux("list-windows", "-F", "#{window_width}x#{window_height} #{window-size}");
  check("AC-13 the pane is sized for this view (window-size manual)", /^\d+x\d+ manual/.test(size) && Number(size.split("x")[0]) > 80, size);
  await liveQuick(page, { tmux, work: WORK });
  await liveSplit(page, { tmux, work: WORK });
  // its own name per browser: the tmux server is both runs' (the first run's window and its marks stay)
  const byhandName = `byhand-${name}`;
  tmux("new-window", "-d", "-n", byhandName);
  await within("AC-01 a window made by hand shows in the list", () => page.waitForFunction((n) => document.getElementById("sessions").innerText.includes(n), byhandName, { timeout: 3000 }));
  await within("AC-70 Files is docked and shows the pane's folder", async () => {
    const frame = await (await page.waitForSelector("#files:not([hidden])", { timeout: 5000 })).contentFrame();
    await frame.waitForFunction(() => document.body.innerText.includes("alpha.txt"), null, { timeout: 8000 });
  });
  const files = await (await page.$("#files")).contentFrame();
  await files.click("#new-file");
  // typed into the name's field once it is there (under load it came after 3 s, and the name went elsewhere)
  const named = await files.waitForSelector("#tree input", { timeout: 10000 });
  await named.type(`fresh-${name}.txt`);                        // the folder is shared by both browsers
  await named.press("Enter");
  await within("AC-70 a new file opens in View, where a click opens files, not under the tree", async () => {
    await page.waitForSelector("#filewin:not([hidden])", { timeout: 5000 });
    const view = await (await page.$("#file")).contentFrame();
    await view.waitForSelector(`.tab.active .tname:text("fresh-${name}.txt")`, { timeout: 5000 });
    if (await files.$(".tab")) throw new Error("a tab opened in Files too");
  });
  await liveView(page, { work: WORK });
  if (await page.isVisible("#fileclose")) await page.click("#fileclose");   // liveView may have hidden it (Esc)
  await liveFolds(page, { tmux, work: WORK });
  await liveLinks(page, { work: WORK });
  await liveLimit(page, { tmux, work: WORK });
  await liveFocus(page, { tmux, work: WORK });
  await liveListPlace(page, { tmux, work: WORK, app: APP });
  await liveKeys(page, { tmux, work: WORK });
  await liveClip(page, { tmux, work: WORK });
  await livePrograms(page, { tmux, work: WORK });
  await livePlans(page, { tmux, work: WORK, bin: BIN, home: HOME, app: APP, stop, start, signIn, name });
  await page.click("#term");
  await page.keyboard.type("vim -u NONE -N");
  await page.keyboard.press("Enter");
  await within("AC-05 a full-screen program hides the history (its grid alone)", () => page.waitForFunction(() => document.getElementById("term").classList.contains("fullscreen"), null, { timeout: 5000 }));
  await page.keyboard.type(":q");
  await page.keyboard.press("Enter");
  await within("AC-05 the history is back when the program ends", () => page.waitForFunction(() => !document.getElementById("term").classList.contains("fullscreen"), null, { timeout: 5000 }));
  fs.writeFileSync(path.join(HOME, "custom.css"), "#nav { --nav-w: 333px }\n");
  await within("AC-19 custom.css applies without a reload", () => page.waitForFunction(() => getComputedStyle(document.getElementById("nav")).getPropertyValue("--nav-w").trim() === "333px", null, { timeout: 5000 }));
  fs.writeFileSync(path.join(HOME, "settings.json"), JSON.stringify({ page: { textSize: 300 } }));
  await within("AC-19 a wrong setting is said once as a toast", () => page.waitForFunction(() => document.getElementById("toast").innerText.includes("page.textSize must be 8–40"), null, { timeout: 5000 }));
  fs.writeFileSync(path.join(HOME, "settings.json"), "{}");
  const theme = JSON.parse(fs.readFileSync(path.join(HOME, "themes/local.json"), "utf8"));
  fs.writeFileSync(path.join(HOME, "themes/local.json"), JSON.stringify({ ...theme, bg: "#102030" }));
  await within("AC-12 a changed theme file reaches the page without a reload", () => page.waitForFunction(() => getComputedStyle(document.documentElement).getPropertyValue("--t-bg").trim() === "#102030", null, { timeout: 5000 }));
  // a device without the theme's font (an iPhone): the shipped one draws the text; its icons file stays unfetched
  fs.writeFileSync(path.join(HOME, "themes/local.json"), JSON.stringify({ ...theme, bg: "#102030", font: "NoSuchFont-Regular" }));
  await within("AC-12 without the theme's font the terminal is drawn in the shipped one; the icons are not fetched", () => page.waitForFunction(() => {
    const faces = [...document.fonts].filter((f) => f.family.replace(/"/g, "") === "TermFontShipped");
    const icons = faces.find((f) => f.unicodeRange.toLowerCase().includes("f0000"));
    return faces.some((f) => f.status === "loaded" && f.weight === "400") && icons?.status === "unloaded";
  }, null, { timeout: 5000 }));
  fs.writeFileSync(path.join(HOME, "themes/local.json"), JSON.stringify({ ...theme, bg: "#102030" }));
  // a color scheme picked in one browser is every browser's (settings.json page.colorScheme)
  const other = await browser.newContext({ viewport: { width: 1000, height: 700 } });
  const page2 = await other.newPage();
  await signIn(page2);
  const bg = (p) => p.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue("--t-bg").trim());
  const kept = () => JSON.parse(fs.readFileSync(path.join(HOME, "settings.json"), "utf8")).page?.colorScheme ?? null;
  await page.click("#menubtn");
  await page.click('#schemes .swatch[data-scheme="Nord"]');
  await within("AC-12 a color scheme picked in one browser shows in another at once and is kept in settings.json", async () => {
    await page2.waitForFunction(() => getComputedStyle(document.documentElement).getPropertyValue("--t-bg").trim() === "#2e3440", null, { timeout: 5000 });
    if (await bg(page) !== "#2e3440" || kept() !== "Nord") throw new Error(`${await bg(page)} ${kept()}`);
  });
  await page.click('#schemes .swatch[data-scheme=""]');
  await within("AC-12 Theme goes back to the theme's colors in every browser", async () => {
    await page2.waitForFunction(() => getComputedStyle(document.documentElement).getPropertyValue("--t-bg").trim() === "#102030", null, { timeout: 5000 });
    if (kept() !== null) throw new Error(String(kept()));
  });
  await page.keyboard.press("Escape");
  await other.close();
  check("AC-08 terminumd marks the tmux server it started with its own executable",
    tmux("show-options", "-gqv", "@terminum-started-by") === fs.realpathSync(BIN), tmux("show-options", "-gqv", "@terminum-started-by"));
  // what a restart of terminumd (an upgrade) keeps: a row marked unread (AC-93), a color scheme picked (AC-12)
  const byhand = page.locator("#sessions .item", { hasText: byhandName });
  await byhand.click({ button: "right" });
  await page.click('#rowmenu button[data-action="unread"]');
  const unreadRow = () => page.waitForFunction((n) => [...document.querySelectorAll("#sessions .item.unread")].some((r) => r.innerText.includes(n)), byhandName, { timeout: 5000 })
    .then(() => "", async (e) => `${e.message} — rows: ${JSON.stringify(await page.evaluate(() => [...document.querySelectorAll("#sessions .item")].map((r) => [r.innerText.split("\n")[0], r.className])))}`);
  const marked = await unreadRow();
  check("AC-93 Mark as Unread marks the row", !marked, marked);
  await page.click("#menubtn");
  await page.click('#schemes .swatch[data-scheme="Nord"]');
  await page.keyboard.press("Escape");
  await stop();
  await start();
  await within("AC-14 the page reconnects after terminumd restarts and shows the pane", () => page.waitForFunction(() => document.querySelectorAll("#screen .ln").length > 3 && !document.getElementById("status").innerText.includes("not running"), null, { timeout: 8000 }));
  const still = await unreadRow();
  check("AC-93 a row marked unread is still unread after terminumd restarts", !still, still);
  check("AC-12 the color scheme picked is kept across the restart", kept() === "Nord" && await bg(page) === "#2e3440", `${kept()} ${await bg(page)}`);
  await page.click("#menubtn");
  await page.click('#schemes .swatch[data-scheme=""]');
  await page.keyboard.press("Escape");
  await liveJitter(page, { tmux, work: WORK });
  check("no page errors", errors.length === 0, errors.slice(0, 3).join(" | "));
  return ctx;
}

async function phone(browser, name) {
  runningIn(`${name} 390×844`);
  const ctx = await browser.newContext({ viewport: { width: 390, height: 844 }, isMobile: name === "chromium", hasTouch: true });
  const page = await ctx.newPage();
  await signIn(page);
  await page.waitForFunction(() => document.querySelectorAll("#screen .ln").length > 3, null, { timeout: 8000 });
  const before = tmux("display", "-p", "#{window_width}");
  await page.evaluate(() => { const k = document.getElementById("kbd"); k.focus(); });
  await page.keyboard.type("echo phone");
  await page.keyboard.press("Enter");
  await within("AC-13 a phone that types sizes the pane to its narrow view", async () => {
    for (let i = 0; i < 50; i++) {
      const w = Number(tmux("display", "-p", "#{window_width}"));
      if (w > 0 && w < 60) return;
      await new Promise((r) => setTimeout(r, 100));
    }
    throw new Error(`width ${tmux("display", "-p", "#{window_width}")} (was ${before})`);
  });
  await ctx.close();
}

try {
  purge();
  await start();
  const pw = await playwright();
  const which = process.argv.slice(2).filter((a) => ["chromium", "webkit"].includes(a));
  for (const name of which.length ? which : ["chromium", "webkit"]) {
    const browser = await pw[name].launch(NO_SIGNALS);
    try {
      const ctx = await wide(browser, name);
      await phone(browser, name);
      await ctx.close();
    } catch (e) {
      check("ran to its end", false, String(e.stack ?? e).split("\n").slice(0, 3).join(" | "));
    } finally { await browser.close(); }
    await stop();
    purge();
    fs.rmSync(path.join(HOME, "themes"), { recursive: true, force: true });
    fs.rmSync(path.join(HOME, "custom.css"), { force: true });
    await start();
  }
} finally {
  await teardown();
}
const failed = results.filter((r) => !r.ok).length;
console.log(failed ? `FAIL (${failed} of ${results.length})` : `PASS (${results.length} checks)`);
process.exit(failed ? 1 : 0);
