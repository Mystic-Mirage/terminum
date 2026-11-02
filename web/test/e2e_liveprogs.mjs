// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// AC-06, AC-12, AC-36, AC-100 against a real private terminumd (e2e_live.mjs) with the real programs a user
// runs, where the other checks' stand-ins only imitate them: vim and mc (full-screen, their keys),
// and Claude Code in both of its TUIs, which copy and paste differently:
//   default     the normal screen, no mouse: the page selects and copies the rows itself;
//   fullscreen  the alternate screen and the mouse: Claude Code selects, and on the release copies
//               with pbcopy (here a stub) and OSC 52 twice, plain and in tmux's passthrough.
// Both take a pasted multi-line text as one paste, never as lines sent. A program not on PATH is a
// visible SKIP. Claude Code is the user's own install and login: it runs with --safe-mode (no
// CLAUDE.md, hooks, plugins or MCP servers), --tools '' and an unreachable ANTHROPIC_BASE_URL (no
// model call), in terminum-claude-check under the user's own temporary folder (os.tmpdir(): 0700
// on macOS), checked to be this user's and 0700 and emptied each run, which it is asked once to
// trust: what it leaves is that folder's trust entry in ~/.claude.json and its own state files.
// TM_TEST_NO_CLAUDE=1 leaves it out.
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { check } from "./check.mjs";

const found = (cmd) => { try { return execFileSync("sh", ["-c", `command -v ${cmd}`], { encoding: "utf8" }).trim(); } catch { return ""; } };
const skip = (what, why) => console.log(`SKIP real program ${what}: ${why}`);
const text = (page) => page.evaluate(() => document.getElementById("screen").innerText);
const waitText = (page, t, ms = 15000) => page.waitForFunction((t) => document.getElementById("screen").innerText.includes(t), t, { timeout: ms }).then(() => true, () => false);
const until = async (ok, ms = 8000) => { for (const end = Date.now() + ms; Date.now() < end; await new Promise((r) => setTimeout(r, 150))) if (await ok()) return true; return false; };
const SHELLS = /^-?(zsh|bash|sh|fish|dash)$/;
// the header's full-screen mark is in sight (AC-05), and beside it a coding agent's orange ! (turn its full screen off)
const inSight = (page, id) => page.evaluate((id) => document.getElementById(id).getBoundingClientRect().width > 0, id);
const marked = (page) => inSight(page, "curscreen");
const why = (page) => inSight(page, "curscreenwhy");

/** `cmd` in a new window of the private tmux, shown on the page as the user opens it: its pane. */
async function open(page, tmux, work, name, cmd) {
  const pane = tmux("new-window", "-P", "-F", "#{pane_id}", "-n", name, "-c", work);
  const row = `#sessions .item[data-id$=":${pane}"]`;
  await page.waitForSelector(row, { timeout: 8000 });
  await page.click(row);
  await page.waitForTimeout(500);
  tmux("send-keys", "-t", pane, cmd, "Enter");
  return pane;
}
const close = (tmux, pane) => tmux("kill-pane", "-t", pane);

async function vim(page, { tmux, work }) {
  const file = path.join(work, "real-vim.txt");
  fs.writeFileSync(file, "first line in vim\n");
  const pane = await open(page, tmux, work, "vim", `vim -u NONE -N -n ${file}`);
  const up = await waitText(page, "first line in vim");
  check("real vim: its full-screen grid is on the page", up && tmux("display", "-p", "-t", pane, "#{alternate_on}") === "1", (await text(page)).slice(0, 80));
  check("AC-05 live: the header marks vim's full screen, with no agent's !", await until(() => marked(page)) && !(await why(page)));
  // AC-100: the row's menu shows what terminumd and tmux see of the pane, here vim's; its Copy copies it
  await page.evaluate(() => { window.copied = []; navigator.clipboard.writeText = async (t) => { window.copied.push(t); }; });
  await page.click(`#sessions .item[data-id$=":${pane}"]`, { button: "right" });
  await page.click('[data-action="diag"]').catch(() => {});
  const shownDiag = await page.waitForFunction(() => document.getElementById("lastlines").open
    && document.getElementById("lastlinestext").textContent.startsWith("terminum diagnostics"), null, { timeout: 5000 }).then(() => true, () => false);
  const diag = shownDiag ? await page.textContent("#lastlinestext") : "";
  const meta = shownDiag ? await page.textContent("#lastlinesmeta") : "";
  if (shownDiag) await page.click("#lastlinescopy");
  const copied = await until(() => page.evaluate((d) => window.copied.includes(d), diag), 3000);
  const home = process.env.HOME ?? "";
  check("AC-100 live: Diagnostics… shows the pane's state as tmux has it (vim, the alternate screen, the folder with ~), its Copy copies it",
    /^  alternate_on: 1$/m.test(diag) && /^  pane_current_command: vim$/m.test(diag) && /^build: /m.test(diag) && /^  pane_current_path: \S/m.test(diag)
      && (!home || !diag.includes(home)) && meta.includes("folder, host and title") && copied,
    diag.split("\n").filter((l) => /alternate_on|pane_current_command|pane_current_path|build/.test(l)).join(" | ") + ` copied ${copied}`);
  await page.keyboard.press("Escape").catch(() => {});
  await page.focus("#kbd");                                     // the window took the keys: back to the terminal
  await page.keyboard.type("Oadded on the page");
  await page.keyboard.press("Escape");
  await page.keyboard.type(":wq");
  await page.keyboard.press("Enter");
  const saved = await until(() => fs.readFileSync(file, "utf8").startsWith("added on the page\nfirst line in vim"));
  check("real vim: keys typed on the page edit and save the file", saved, JSON.stringify(fs.readFileSync(file, "utf8")));
  check("real vim: :wq gives the shell back", await until(() => SHELLS.test(tmux("display", "-p", "-t", pane, "#{pane_current_command}"))));
  check("AC-05 live: the mark goes with vim", await until(async () => !(await marked(page))));
  close(tmux, pane);
}

async function mc(page, { tmux, work }) {
  const pane = await open(page, tmux, work, "mc", "mc -u -d");
  const up = await until(async () => /Left|File|Command/.test(await text(page)) && tmux("display", "-p", "-t", pane, "#{alternate_on}") === "1", 15000);
  check("real mc: its panels are on the page", up, (await text(page)).slice(0, 80));
  await page.keyboard.press("F10");
  const gone = await until(() => SHELLS.test(tmux("display", "-p", "-t", pane, "#{pane_current_command}")));
  check("real mc: F10 on the page quits it", gone, tmux("display", "-p", "-t", pane, "#{pane_current_command}"));
  close(tmux, pane);
}

const DIR = path.join(os.tmpdir(), "terminum-claude-check");
/** The folder Claude Code is asked to trust: this user's, 0700, empty (never one another user made). */
function ownDir() {
  fs.rmSync(DIR, { recursive: true, force: true });
  fs.mkdirSync(DIR, { mode: 0o700 });
  const st = fs.lstatSync(DIR);
  if (!st.isDirectory() || st.uid !== process.getuid() || (st.mode & 0o777) !== 0o700) throw new Error(`${DIR} is not this user's 0700 folder`);
}
const PASTE = "alpha one\nbeta two\ngamma three";

/** Claude Code in its `tui` mode in a new window, past the trust question: {pane, stubLog}. */
async function claude(page, { tmux, work }, tui) {
  ownDir();
  const stub = fs.mkdtempSync(path.join(work, "pb-")), stubLog = path.join(stub, "copied.txt");
  // the user's clipboard is never written: pbcopy is a stub that keeps what it was given
  fs.writeFileSync(path.join(stub, "pbcopy"), `#!/bin/sh\ncat >> '${stubLog}'\n`, { mode: 0o755 });
  const env = `PATH='${stub}':"$PATH" ANTHROPIC_BASE_URL=http://127.0.0.1:9 DISABLE_AUTOUPDATER=1 CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1`;
  const pane = await open(page, tmux, DIR, `claude-${tui}`, `env ${env} claude --safe-mode --tools '' --settings '{"tui":"${tui}"}'`);
  const ready = await until(async () => {
    const t = tmux("capture-pane", "-p", "-t", pane);
    if (t.includes("trust this folder") && t.includes("❯ No")) tmux("send-keys", "-t", pane, "Down", "Enter");
    return t.includes("Safe mode") && /^❯/m.test(t);
  }, 30000);
  return { pane, stubLog, ready };
}

/** Ctrl-C twice: Claude Code ends; the shell is back. */
async function quit(tmux, pane) {
  for (let i = 0; i < 3 && !SHELLS.test(tmux("display", "-p", "-t", pane, "#{pane_current_command}")); i++) {
    tmux("send-keys", "-t", pane, "C-c"); await new Promise((r) => setTimeout(r, 400));
    tmux("send-keys", "-t", pane, "C-c"); await until(() => SHELLS.test(tmux("display", "-p", "-t", pane, "#{pane_current_command}")), 3000);
  }
  return SHELLS.test(tmux("display", "-p", "-t", pane, "#{pane_current_command}"));
}

/** A paste of three lines from the page: all in Claude Code's input, none sent. */
async function pasted(page, tmux, pane, tui) {
  await page.evaluate((t) => {
    const dt = new DataTransfer(); dt.setData("text/plain", t);
    document.getElementById("kbd").dispatchEvent(new ClipboardEvent("paste", { clipboardData: dt, bubbles: true, cancelable: true }));
  }, PASTE);
  const inBox = () => {
    const t = tmux("capture-pane", "-p", "-t", pane);
    return /^\s*❯\s+alpha one/m.test(t) && t.includes("beta two") && t.includes("gamma three") && t.split("alpha one").length === 2;   // ❯ and a no-break space
  };
  const landed = await until(inBox, 6000);
  await new Promise((r) => setTimeout(r, 1500));   // a line sent would leave the input and draw an answer
  const t = tmux("capture-pane", "-p", "-t", pane);
  check(`real claude (${tui}): a pasted multi-line text lands in its input as one paste, nothing sent`,
    landed && inBox(), t.split("\n").filter((l) => l.trim()).slice(-9).join(" | "));
  tmux("send-keys", "-t", pane, "C-c");   // the input emptied, Claude Code still up
  await new Promise((r) => setTimeout(r, 400));
}

/** Where `words` stand on the page's row that holds them: {x, x2, y}, or null. */
const rowOf = (page, words) => page.evaluate((w) => {
  const l = [...document.querySelectorAll("#screen .ln")].find((e) => e.textContent.includes(w));
  if (!l) return null;
  // the row's text nodes in order (a row is runs of styled spans): the words' first and last character
  const walk = document.createTreeWalker(l, NodeFilter.SHOW_TEXT), nodes = [];
  for (let n; (n = walk.nextNode());) nodes.push(n);
  const all = nodes.map((n) => n.data).join(""), from = all.indexOf(w), to = from + w.length;
  const at = (i) => { for (const n of nodes) { if (i <= n.data.length) return [n, i]; i -= n.data.length; } return null; };
  const r = document.createRange();
  r.setStart(...at(from)); r.setEnd(...at(to));
  const rects = [...r.getClientRects()];
  return { x: rects[0].left + 1, x2: rects.at(-1).right - 1, y: rects[0].top + rects[0].height / 2 };
}, words);

async function claudeDefault(page, ctx) {
  const { pane, ready } = await claude(page, ctx, "default");
  const { tmux } = ctx;
  check("real claude (default): it runs on the normal screen with no mouse",
    ready && tmux("display", "-p", "-t", pane, "#{alternate_on}#{mouse_any_flag}") === "00", tmux("capture-pane", "-p", "-t", pane).slice(-200));
  if (!ready) return quit(tmux, pane).then(() => close(tmux, pane));
  check("AC-05 live: no full-screen mark for Claude Code's default TUI", !(await marked(page)));
  await pasted(page, tmux, pane, "default");
  // the page selects and copies itself (⌘C: the copy event the page fills): the row's words as shown
  await waitText(page, "Safe mode");
  const at = await rowOf(page, "Safe mode: all customizations");
  if (at) { await page.mouse.move(at.x, at.y); await page.mouse.down(); await page.mouse.move(at.x2, at.y, { steps: 5 }); await page.mouse.up(); }
  const got = await page.evaluate(() => {
    let text = null;
    const spy = (e) => { text = e.clipboardData.getData("text/plain"); };
    window.addEventListener("copy", spy);                      // after the page's own handler (document)
    document.execCommand("copy");
    window.removeEventListener("copy", spy);
    return text;
  });
  check("real claude (default): the page's own selection copies the row's words as shown",
    got === "Safe mode: all customizations", JSON.stringify({ at, got, sel: await page.evaluate(() => String(getSelection())) }));
  await page.evaluate(() => getSelection().removeAllRanges());
  check("real claude (default): Ctrl-C twice ends it", await quit(tmux, pane));
  close(tmux, pane);
}

async function claudeFullscreen(page, ctx) {
  const { pane, stubLog, ready } = await claude(page, ctx, "fullscreen");
  const { tmux } = ctx;
  check("real claude (fullscreen): it runs on the alternate screen with the mouse",
    ready && tmux("display", "-p", "-t", pane, "#{alternate_on}#{mouse_any_flag}") === "11", tmux("capture-pane", "-p", "-t", pane).slice(-200));
  if (!ready) return quit(tmux, pane).then(() => close(tmux, pane));
  check("AC-05 live: the header marks Claude Code's fullscreen TUI, an orange ! beside the mark", await until(() => marked(page)) && await until(() => why(page)));
  await pasted(page, tmux, pane, "fullscreen");
  // Claude Code selects on the mouse the page passes on, and copies on the release
  await waitText(page, "Safe mode");
  const at = await rowOf(page, "Safe mode");
  if (at) { await page.mouse.move(at.x, at.y); await page.mouse.down(); await page.mouse.move(at.x2, at.y, { steps: 5 }); await page.mouse.up(); }
  const copied = await until(() => fs.existsSync(stubLog) && fs.readFileSync(stubLog, "utf8").length > 0, 5000) ? fs.readFileSync(stubLog, "utf8") : "";
  const shown = await page.waitForSelector("#markadd:not([hidden])", { timeout: 4000 }).then(() => true, () => false);
  check("real claude (fullscreen): its own copy of the selection (OSC 52, plain and in tmux's passthrough) shows Copy, [+] and Ask",
    shown && copied.includes("Safe mode") && !(await page.evaluate(() => String(getSelection()))), JSON.stringify({ copied, shown }));
  if (shown) {
    await page.click("#markadd"); await page.waitForTimeout(300);
    const quote = await page.evaluate(() => document.getElementById("markbox").open && document.getElementById("markbox").textContent);
    const times = typeof quote === "string" ? quote.split(copied.trim()).length - 1 : 0;
    check("real claude (fullscreen): [+] holds Claude Code's text once (its two OSC 52 writes are one copy)",
      !!copied.trim() && times === 1, String(quote).replace(/\s+/g, " ").slice(0, 120));
    await page.keyboard.press("Escape"); await page.waitForTimeout(200);
  }
  check("real claude (fullscreen): Ctrl-C twice ends it", await quit(tmux, pane));
  close(tmux, pane);
  fs.rmSync(path.dirname(stubLog), { recursive: true, force: true });
}

export async function livePrograms(page, ctx) {
  const back = await page.evaluate(() => document.querySelector("#sessions .item.on")?.dataset.id);
  if (found("vim")) await vim(page, ctx); else skip("vim", "not on PATH");
  if (found("mc")) await mc(page, ctx); else skip("mc", "not on PATH");
  if (process.env.TM_TEST_NO_CLAUDE === "1") skip("claude", "TM_TEST_NO_CLAUDE=1");
  else if (!found("claude")) skip("claude", "not on PATH");
  else { await claudeDefault(page, ctx); await claudeFullscreen(page, ctx); }
  if (back) await page.click(`#sessions .item[data-id="${back}"]`).catch(() => {});
}
