// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// AC-03 against a real private terminumd (e2e_live.mjs): a long run of keys (a coding agent's
// input moved by lines, held) reaches a busy program whole: no read of it ends inside a key's
// bytes. As one run tmux handed it on in parts of 1022 bytes, ESC in one, "[D" in the next,
// and Claude Code typed "[D" into its input. A long typed text (a quote Ask types) reaches it
// in reads of at most 800 characters, which Claude Code would fold as a paste.
import fs from "node:fs";
import path from "node:path";
import { check } from "./check.mjs";

const KEYS = 1000;

export async function liveKeys(page, { tmux, work }) {
  const shown = tmux("list-panes", "-a", "-F", "#{pane_id} #{pane_current_path}").split("\n").map((l) => l.split(" ")).find((p) => p[1] === work)?.[0];
  let r = await typedInto(page, { tmux, work }, "\x1b[D".repeat(KEYS));
  check("AC-03 a long run of keys reaches a busy program whole: no read ends inside a key (\"[D\" typed)", !!r.id && r.total === KEYS * 3 && r.cut === 0, JSON.stringify(r));
  const quote = Array.from({ length: 40 }, (_, i) => `> line ${i} of the quote Ask types, as long as an agent's answer`).join("\n");
  r = await typedInto(page, { tmux, work }, quote);
  check("AC-03 a long typed text (a 2.7 KB quote) reaches a program in reads of at most 512 bytes: none Claude Code would fold as a paste",
    !!r.id && r.total === quote.length && r.most <= 512, JSON.stringify(r));
  if (shown) await page.click(`#sessions .item[data-id$=":${shown}"]`).catch(() => {});
}

// `data` typed by the page into a program that reads raw input and draws after each read,
// slowly (as an agent redraws its box): {id, total, cut: reads ending inside a key, most: the largest read}
async function typedInto(page, { tmux, work }, data) {
  const reader = path.join(work, "reader.py"), log = path.join(work, "reads.txt");
  fs.writeFileSync(reader, [
    "import os, sys, termios, tty, time",
    "tty.setraw(0); total = cut = most = 0",
    `while total < ${Buffer.byteLength(data)}:`,
    "    b = os.read(0, 4096); total += len(b); most = max(most, len(b)); cut += b.endswith(b'\\x1b') or b.endswith(b'\\x1b[')",
    "    os.write(1, b'.'); time.sleep(0.02)",
    "    open(sys.argv[1], 'w').write(f'{total} {cut} {most}')",
  ].join("\n") + "\n");
  fs.rmSync(log, { force: true });
  const pane = tmux("new-window", "-d", "-P", "-F", "#{pane_id}", "-n", "keys", "-c", work, `python3 -I ${reader} ${log}`);
  await page.waitForSelector(`#sessions .item[data-id$=":${pane}"]`, { timeout: 5000 }).catch(() => {});
  const id = await page.evaluate((pane) => document.querySelector(`#sessions .item[data-id$=":${pane}"]`)?.dataset.id, pane);
  // as the page sends it (boxnav.js stepKeys, ask.js), on a socket of its own
  await page.evaluate(([id, data]) => new Promise(async (done) => {
    const ws = new WebSocket(`ws://${location.host}/ws`, await import("/signin.js").then((m) => m.protocols()));
    ws.onopen = () => { ws.send(JSON.stringify({ t: "sub", id })); setTimeout(() => { ws.send(JSON.stringify({ t: "in", data })); setTimeout(() => { ws.close(); done(); }, 300); }, 500); };
    ws.onerror = () => done();
  }), [id, data]);
  let got = "";
  const want = `${Buffer.byteLength(data)} `;
  for (let i = 0; i < 150 && !(got = fs.existsSync(log) ? fs.readFileSync(log, "utf8") : "").startsWith(want); i++) await new Promise((r) => setTimeout(r, 100));
  const [total, cut, most] = got.split(" ").map(Number);
  tmux("kill-window", "-t", pane);
  fs.rmSync(reader, { force: true }); fs.rmSync(log, { force: true });
  return { id, total, cut, most };
}
