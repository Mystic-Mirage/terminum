// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// AC-35 against a real private terminumd and tmux (e2e_live.mjs): a link a program printed with a
// text of its own (OSC 8, as Claude Code prints "⧉ Attack and Compromise Detection — MR !318")
// reaches the page with its address, is underlined and opens that address with ⌘ (Ctrl elsewhere);
// a javascript: one stays plain text. AC-102: Links lists a Claude Code session's addresses from
// its transcript.
import fs from "node:fs";
import path from "node:path";
import { check } from "./check.mjs";

const MR = "https://git.example/a/-/merge_requests/318";
const LONG = Array.from({ length: 40 }, (_, i) => `part${i}`).join(" ");   // wider than the pane: tmux wraps it

export async function liveLinks(page, { work, onSocket, socket, claude }) {
  const file = path.join(work, "links.txt");
  fs.writeFileSync(file, `see \x1b]8;id=a1;${MR}\x1b\\Attack and Compromise Detection — MR !318\x1b]8;;\x1b\\ and \x1b]8;;javascript:alert(1)\x1b\\evil\x1b]8;;\x1b\\ end\n\x1b]8;;${MR}\x1b\\${LONG}\x1b]8;;\x1b\\ after\n`);
  await page.evaluate(() => { window.opened = []; window.open = (u) => { window.opened.push(u); return null; }; });
  await page.click("#term");
  await page.keyboard.type(`clear; cat ${file}`);
  await page.keyboard.press("Enter");
  const link = await page.waitForFunction(() => {
    const e = [...document.querySelectorAll("#term .ln .olink")].find((s) => s.textContent.includes("MR !318"));
    if (!e) return null;
    const b = e.getBoundingClientRect();
    return { text: e.textContent, href: e.dataset.href, title: e.title, x: b.left + b.width / 2, y: b.top + b.height / 2 };
  }, null, { timeout: 5000 }).then((h) => h.jsonValue(), () => null);
  const halves = await page.evaluate((mr) => [...document.querySelectorAll("#term .ln .olink")].filter((s) => s.dataset.href === mr).map((s) => s.textContent), MR);
  const evil = await page.evaluate(() => [...document.querySelectorAll("#term .ln span")].filter((s) => s.textContent === "evil").map((s) => s.classList.contains("olink") || !!s.title));
  const mod = process.platform === "darwin" ? "Meta" : "Control";
  // where the link rests: the view may still move after the output (the prompt drawn, the view kept at its bottom)
  const at = link && await page.waitForFunction(() => {
    const e = [...document.querySelectorAll("#term .ln .olink")].find((s) => s.textContent.includes("MR !318")), b = e?.getBoundingClientRect();
    const now = b && `${b.left},${b.top}`;
    if (!now || now !== window.linkWas) { window.linkWas = now; window.linkSince = performance.now(); return null; }
    return performance.now() - window.linkSince > 300 ? { x: b.left + b.width / 2, y: b.top + b.height / 2 } : null;
  }, null, { timeout: 5000, polling: 50 }).then((h) => h.jsonValue(), () => null);
  if (at) { await page.keyboard.down(mod); await page.mouse.click(at.x, at.y); await page.keyboard.up(mod); }
  const opened = await page.evaluate(() => window.opened);
  check("AC-35 a program's OSC 8 link through tmux: its text underlined with its address on hover, ⌘-click opens that address; a javascript: one is plain text",
    link?.text === "Attack and Compromise Detection — MR !318" && link.href === MR && link.title === MR && JSON.stringify(opened) === JSON.stringify([MR])
      && JSON.stringify(evil) === "[false]", JSON.stringify({ link, at, opened, evil }));
  // tmux 3.6a ends a link at a row's end and does not open it again on the next (AS-03): its first row only
  check("AC-35 an OSC 8 link wider than the pane is a link on its first row; the text after it is not",
    halves.length === 2 && LONG.startsWith(halves[1]) && halves[1].length > 20 && !halves.some((t) => t.includes("after")), JSON.stringify(halves));
  await liveLinkList(page, { work, onSocket, socket, claude });
  fs.rmSync(file, { force: true });
}

/** A full-screen Claude Code leaves tmux no history: the session's transcript (CLAUDE_CONFIG_DIR,
 *  this run's own) gives its links. */
async function liveLinkList(page, { work, onSocket, socket, claude }) {
  const LOST = "https://lost.example/scrolled-away", DOCS = "https://docs.example/a";
  const where = path.join(work, "pane.txt");
  await page.click("#term");
  await page.keyboard.type(`clear; echo $TMUX_PANE > ${where}`);
  await page.keyboard.press("Enter");
  for (let i = 0; i < 50 && !fs.existsSync(where); i++) await new Promise((r) => setTimeout(r, 100));
  const pane = fs.readFileSync(where, "utf8").trim();
  await page.click("#linksfab");
  const none = await page.waitForFunction(() => /Claude Code/.test(document.getElementById("linkn").textContent) && document.getElementById("linkn").textContent, null, { timeout: 8000 }).then((h) => h.jsonValue(), () => null);
  check("AC-102 a pane with no Claude Code session says so", none === "No Claude Code session runs in this pane.", JSON.stringify(none));
  await page.click("#linksfab");
  await page.waitForSelector("#linkspane", { state: "hidden", timeout: 2000 }).catch(() => {});
  fs.mkdirSync(path.join(claude, "projects/-work"), { recursive: true });
  await liveProcessRecord(page, { work, claude });
  const said = (type, text) => JSON.stringify({ type, timestamp: new Date().toISOString(), message: { content: [{ type: "text", text }] } }) + "\n";
  fs.writeFileSync(path.join(claude, "projects/-work/s-links.jsonl"),
    said("assistant", `Opened ${LOST}.`) + said("user", `again ${LOST}`) + said("assistant", `docs: ${DOCS}.`));
  const ev = (event) => onSocket("POST", "/hook", { agent: "claude", socket, pane, event, session: "s-links", turn: "t1", cwd: work });
  await ev("UserPromptSubmit");
  try {
    await page.click("#linksfab");
    const got = await page.waitForFunction(() => {
      const rows = [...document.querySelectorAll("#linkrows li a.lk")];
      if (rows.length < 2) return null;
      return { n: document.getElementById("linkn").textContent, head: document.querySelector("#linkspane .pwtitle").textContent, button: !!document.querySelector("#linkrows button"),
        rows: rows.map((a) => ({ href: a.getAttribute("href"), target: a.target, rel: a.rel, text: a.querySelector(".lkt").textContent, more: a.querySelector(".count")?.textContent ?? "",
          after: a.querySelector(".count")?.previousElementSibling === a.querySelector(".lkt"), when: a.querySelector(".lkw")?.textContent, full: a.querySelector(".lkw")?.title,
          two: a.getBoundingClientRect().height > 2.2 * parseFloat(getComputedStyle(a).lineHeight) })) };
    }, null, { timeout: 8000 }).then((h) => h.jsonValue(), () => null);
    const [docs, lost] = got?.rows ?? [];
    check("AC-102 Links, under the sent history's button, lists the transcript's addresses once each, the newest first, its final \".\" left out, ×2 after the address named twice; a row says when, today its hour, its whole date and time on hover",
      got?.head === "Links" && got.n === "2 links" && !got.button && docs?.href === DOCS && docs.text === DOCS && lost?.href === LOST && lost.more === "×2" && lost.after
        && lost.target === "_blank" && lost.rel.includes("noopener") && /^\d\d:\d\d$/.test(lost.when ?? "") && /\d{4}/.test(lost.full ?? "") && !got.rows.some((r) => r.two), JSON.stringify(got));
    await page.fill("#linkq", "DOCS");
    const filtered = await page.evaluate(() => ({ n: document.getElementById("linkn").textContent, rows: document.querySelectorAll("#linkrows li").length }));
    check("AC-102 the filter keeps the rows whose address holds it, case aside", filtered.rows === 1 && filtered.n === "1 of 2 links", JSON.stringify(filtered));
    await page.fill("#linkq", "");
    await page.click('#linkby [data-by="count"]');
    const byCount = await page.waitForFunction((l) => document.querySelector("#linkrows a.lk")?.getAttribute("href") === l || null, LOST, { timeout: 8000 }).then(() => true, () => false);
    await page.click('#linkby [data-by="new"]');
    const byNew = await page.waitForFunction((d) => document.querySelector("#linkrows a.lk")?.getAttribute("href") === d || null, DOCS, { timeout: 8000 }).then(() => true, () => false);
    check("AC-102 Most named puts the address named twice first; Newest the last named", byCount && byNew, JSON.stringify({ byCount, byNew }));
    await page.click("#linksfab");
    check("AC-102 the button pressed again closes the window", await page.waitForSelector("#linkspane", { state: "hidden", timeout: 2000 }).then(() => true, () => false), "");
  } finally {
    await ev("SessionEnd");
    fs.rmSync(where, { force: true });
  }
}

/** Before its first hook event, a Claude Code session is the one its process's record names
 *  (`sessions/<pid>.json`); a node process the pane started stands in for it. Ended, the record
 *  names no session. */
async function liveProcessRecord(page, { work, claude }) {
  const PROC = "https://proc.example/from-record", got = path.join(work, "child.txt");
  await page.click("#term");
  await page.keyboard.type(`clear; node -e "setTimeout(() => {}, 60000)" & echo $! > ${got}`);
  await page.keyboard.press("Enter");
  for (let i = 0; i < 50 && !fs.existsSync(got); i++) await new Promise((r) => setTimeout(r, 100));
  const pid = Number(fs.readFileSync(got, "utf8").trim());
  const alive = () => { try { process.kill(pid, 0); return true; } catch { return false; } };
  try {
    // Linux: the process's start as /proc says it (Claude Code's procStart); elsewhere the program's name says it
    const stat = (() => { try { return fs.readFileSync(`/proc/${pid}/stat`, "utf8"); } catch { return null; } })();
    const procStart = stat ? stat.slice(stat.lastIndexOf(")") + 1).trim().split(/\s+/)[19] : undefined;
    fs.mkdirSync(path.join(claude, "sessions"), { recursive: true });
    fs.writeFileSync(path.join(claude, `sessions/${pid}.json`), JSON.stringify({ pid, sessionId: "s-proc", procStart }));
    fs.writeFileSync(path.join(claude, "projects/-work/s-proc.jsonl"),
      JSON.stringify({ type: "user", timestamp: new Date().toISOString(), message: { content: `see ${PROC}` } }) + "\n");
    // the window open again shows the rows it had until the answer comes: the count line `want` is waited for
    const rows = async (want) => {
      await page.click("#linksfab");
      const r = await page.waitForFunction((w) => {
        const n = document.getElementById("linkn").textContent, hrefs = [...document.querySelectorAll("#linkrows a.lk")].map((a) => a.getAttribute("href"));
        return n === w ? { n, hrefs } : null;
      }, want, { timeout: 8000 }).then((h) => h.jsonValue(), () => page.evaluate(() => ({ n: document.getElementById("linkn").textContent })));
      await page.click("#linksfab");
      await page.waitForSelector("#linkspane", { state: "hidden", timeout: 2000 }).catch(() => {});
      return r;
    };
    const first = await rows("1 link");
    check("AC-102 with no hook event yet, the session its process's record names (a process under the pane's shell)",
      JSON.stringify(first?.hrefs) === JSON.stringify([PROC]), JSON.stringify(first));
    process.kill(pid);
    for (let i = 0; i < 50 && alive(); i++) await new Promise((r) => setTimeout(r, 100));
    const ended = await rows("No Claude Code session runs in this pane.");
    check("AC-102 its process ended, the session found before is not kept", ended?.n === "No Claude Code session runs in this pane.", JSON.stringify(ended));
  } finally {
    if (alive()) process.kill(pid);
    fs.rmSync(got, { force: true });
    fs.rmSync(path.join(claude, `sessions/${pid}.json`), { force: true });
  }
}
