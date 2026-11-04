// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// AC-06 against a real private terminumd (e2e_live.mjs): a full-screen program with the mouse that
// selects on its own and copies with OSC 52 on the release (as Claude Code's fullscreen mode does,
// played by a stand-in): terminumd reads the write from tmux's %output and the page shows Copy, [+]
// and Ask with its text; the same write printed with no release before it shows nothing.
import fs from "node:fs";
import path from "node:path";
import { check, shownPane, within } from "./check.mjs";

// the stand-in: the alternate screen, every mouse event (SGR); on a left release it writes its
// selection as OSC 52, on "x" it writes one unasked, on "q" it ends
const PROGRAM = `import base64, os, sys, termios, tty
fd = sys.stdin.fileno(); old = termios.tcgetattr(fd); tty.setraw(fd)
out = lambda s: (os.write(1, s.encode()), None)
out("\\x1b[?1049h\\x1b[?1003h\\x1b[?1006h\\x1b[H\\x1b[2Jstand-in ready\\r\\nselected words here\\r\\nunasked words\\r\\n")
clip = lambda t: out("\\x1b]52;c;" + base64.b64encode(t.encode()).decode() + "\\x07")
buf = ""
try:
    while True:
        c = os.read(fd, 64).decode(errors="replace"); buf += c
        if "q" in c and "\\x1b" not in c: break
        if c == "x": clip("unasked words")
        if "m" in buf and "\\x1b[<0;" in buf: clip("selected words"); out("\\x1b[4;1Hcopied")
        if "m" in buf or "M" in buf: buf = ""
finally:
    termios.tcsetattr(fd, termios.TCSADRAIN, old); out("\\x1b[?1003l\\x1b[?1006l\\x1b[?1049l")
`;

export async function liveClip(page, { tmux, work }) {
  const file = path.join(work, "clip.py");
  fs.writeFileSync(file, PROGRAM);
  const pane = await shownPane(page, { tmux, work });
  tmux("send-keys", "-t", pane, `python3 ${file}`, "Enter");
  await within("AC-06 the stand-in has the alternate screen and the mouse", () => page.waitForFunction(() => document.getElementById("screen").innerText.includes("selected words here"), null, { timeout: 8000 }));
  await page.waitForTimeout(300);
  const state = () => page.evaluate(() => ({ add: !document.getElementById("markadd").hidden, ask: !document.getElementById("askfab").hidden, sel: String(getSelection()) }));

  // a write with no release before it: nothing
  tmux("send-keys", "-t", pane, "x");
  await page.waitForTimeout(800);
  const unasked = await state();
  check("AC-06 live: a clipboard write with no release before it shows nothing", !unasked.add, JSON.stringify(unasked));

  const row = await page.evaluate(() => {
    const r = [...document.querySelectorAll("#screen .ln")].find((l) => l.textContent.includes("selected words here")).getBoundingClientRect();
    return { x: r.left + 4, x2: r.left + 120, y: r.top + r.height / 2 };
  });
  await page.mouse.move(row.x, row.y); await page.mouse.down(); await page.mouse.move(row.x2, row.y, { steps: 4 }); await page.mouse.up();
  const shown = await page.waitForSelector("#markadd:not([hidden])", { timeout: 4000 }).then(() => true, () => false), s = await state();
  check("AC-06 live: what the program copied on the release (OSC 52 through tmux and terminumd) shows Copy, [+] and Ask",
    shown && s.ask && !s.sel, JSON.stringify(s));
  await page.click("#markadd");
  await page.waitForTimeout(300);
  const quote = await page.evaluate(() => document.getElementById("markbox").open && document.getElementById("markbox").textContent);
  check("AC-06 live: [+] holds the program's text", typeof quote === "string" && quote.includes("selected words"), String(quote).replace(/\s+/g, " ").slice(0, 120));
  await page.keyboard.press("Escape");
  await page.waitForTimeout(200);
  tmux("send-keys", "-t", pane, "q");
  await page.waitForTimeout(300);
  fs.rmSync(file, { force: true });
}
