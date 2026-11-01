// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// AC-06 against a fake terminumd (fake.mjs), with a mouse: what a full-screen program that has the
// mouse (Claude Code's fullscreen mode) copied of its own selection after this page's release shows
// Copy, [+] and Ask by the selection's upper row with the program's text, and stays while the
// program redraws; a copy of another pane or with no release shows nothing; Shift selects in the browser
// from the press (Chrome and Firefox extended the keyboard field's caret and selected nothing).
import { check, runningIn } from "./check.mjs";
import { drawn, group, item, line, openPage, pause, spyCopies, WIDE } from "./fake.mjs";

const SID = "tmux:local:terminum:%1";
const LAYOUT = [group("main", [item(SID, "Claude Code", { agent: "claude", shell: false, sub: ["claude", "~/work/api"] })])];
const TEXT = ["  Messages: 3.3k tokens (0.3%)", "  Free space: 931.7k (93.2%)", "  Autocompact buffer: 33k tokens", "> /context", "", "  Opus 5.5",
  ...Array.from({ length: 14 }, (_, i) => `  row ${i + 6}`), "  Skills: 4.9k tokens (0.5%)", "  Memory files: 1.1k tokens", "  Custom agents: 73 tokens"];

TEXT.splice(8, 3, "⏺ The tests pass now.", "  ⎿  first line of the output", "     second line goes here");   // Claude Code's unselectable marks
TEXT[12] = "  rm\u200b -rf\u2060 build";     // zero-width characters: drawn as nothing, pasted as bytes

export async function webProgCopy(browser, name) {
  runningIn(`${name} 1400×900`);
  const f = await openPage(browser, { device: WIDE, layout: LAYOUT });
  const { page, sent } = f;
  await drawn(page);
  let tick = 0;
  // the program redraws its rows (a spinner): new row elements, selectionchange in the page
  const frame = () => f.say({ t: "screen", sid: SID, full: true, n: TEXT.length, cols: 80, rows: TEXT.length, alt: true, mouse: true,
    ch: TEXT.map((t, i) => [i, line(i === 5 ? `${t} ${"·✻"[tick++ % 2]}` : t)]) });
  frame();
  await pause(page, 300);
  const cell = (x, y) => page.evaluate(([x, y]) => {
    const ln = document.querySelectorAll("#screen .ln")[y], r = ln.getBoundingClientRect(), w = r.width / Math.max(1, ln.textContent.length);
    return { x: r.left + w * (x + 0.5), y: r.top + r.height / 2 };
  }, [x, y]);
  const drag = async (from, to, shift = false) => {
    const a = await cell(...from), b = await cell(...to);
    if (shift) await page.keyboard.down("Shift");
    await page.mouse.move(a.x, a.y); await page.mouse.down(); await page.mouse.move(b.x, b.y, { steps: 5 }); await page.mouse.up();
    if (shift) await page.keyboard.up("Shift");
  };
  const buttons = () => page.evaluate(() => {
    const on = (id) => !document.getElementById(id).hidden;
    const ask = document.getElementById("askfab").getBoundingClientRect(), add = document.getElementById("markadd").getBoundingClientRect();
    const rows = [...document.querySelectorAll("#screen .ln")].map((r) => r.getBoundingClientRect());
    const copy = document.getElementById("markcopy").getBoundingClientRect();
    return { copy: on("markcopy"), add: on("markadd"), ask: on("askfab"), copyMid: copy.top + copy.height / 2, rowMids: rows.map((r) => r.top + r.height / 2), sel: String(getSelection()) };
  });
  const mice = () => sent.filter((m) => m.t === "mouse").length;

  // a drag over rows 0-1: the program gets it and copies what it selected
  let n = mice();
  await drag([2, 0], [12, 1]);
  const copied = "Messages: 3.3k tokens (0.3%)\n  Free space: 931.7k";
  f.say({ t: "copied", sid: SID, text: copied });
  await pause(page, 200);
  const shown = await buttons();
  check(`AC-06 (${name}) a drag still goes to the program (press, moves, release)`, mice() - n >= 3, String(mice() - n));
  check(`AC-06 (${name}) what the program copied after the release shows Copy, [+] and Ask, no browser selection`,
    shown.copy && shown.add && shown.ask && shown.sel === "", JSON.stringify(shown));
  const frames = async (k) => { for (let i = 0; i < k; i++) { frame(); await pause(page, 120); } };
  await frames(8);
  const kept = await buttons();
  check(`AC-06 (${name}) the buttons stay while the program redraws (1 s of frames)`, kept.copy && kept.add && kept.ask, JSON.stringify(kept));
  await page.click("#markadd");
  await pause(page, 300);
  const dlg = await page.evaluate(() => ({ open: document.getElementById("markbox").open, quote: document.getElementById("markbox").textContent }));
  check(`AC-06 (${name}) [+] bookmarks the program's text`, dlg.open && dlg.quote.includes("931.7k"), JSON.stringify(dlg).slice(0, 200));
  await page.keyboard.press("Escape");
  await pause(page, 200);

  // the same again, then a key: they go
  await drag([2, 0], [12, 1]);
  f.say({ t: "copied", sid: SID, text: copied });
  await pause(page, 200);
  await page.keyboard.press("q");
  await pause(page, 200);
  check(`AC-06 (${name}) a key after it takes the buttons away`, !(await buttons()).add);

  // a drag upwards: by its upper row, never the press's (the lower)
  await drag([24, 22], [4, 20]);
  f.say({ t: "copied", sid: SID, text: "Skills: 4.9k tokens (0.5%)\n  Memory files: 1.1k tokens\n  Custom" });
  await pause(page, 200);
  const up = await buttons();
  check(`AC-06 (${name}) a drag upwards: Copy level with its upper row`, up.add && Math.abs(up.copyMid - up.rowMids[20]) < 6, JSON.stringify({ copy: up.copyMid, upper: up.rowMids[20], press: up.rowMids[22] }));
  await page.mouse.click(5, 5);
  await page.keyboard.press("Escape");
  await pause(page, 200);

  // writes nobody asked for: another pane, no release lately
  await drag([2, 0], [12, 0]);
  f.say({ t: "copied", sid: "tmux:local:terminum:%9", text: "Messages" });
  await pause(page, 200);
  check(`AC-06 (${name}) a copy of another pane shows nothing`, !(await buttons()).add);
  await page.keyboard.press("Escape");
  await pause(page, 3200);
  f.say({ t: "copied", sid: SID, text: "Messages" });
  await pause(page, 200);
  check(`AC-06 (${name}) a copy 3 s after the last release shows nothing`, !(await buttons()).add);

  // the program's text as it copied it, never compared with the screen: Claude Code leaves out what
  // it draws unselectable (⏺, ⎿), a drag may scroll its text away
  await drag([2, 8], [12, 10]);
  f.say({ t: "copied", sid: SID, text: "The tests pass now.\nfirst line of the output\nsecond line" });
  await pause(page, 200);
  check(`AC-06 (${name}) a copy without the program's unselectable ⏺ and ⎿ shows the buttons`, (await buttons()).add);
  await page.keyboard.press("Escape");
  // the latest copy within the 2 s is the one held (a triple click copies a word, then the line)
  const away = "  The tests pass now.\na line scrolled away\n";
  await drag([2, 8], [12, 10]);
  f.say({ t: "copied", sid: SID, text: "tests" });
  f.say({ t: "copied", sid: SID, text: away });
  await pause(page, 200);
  check(`AC-06 (${name}) a copy with a line no longer on the screen shows the buttons`, (await buttons()).add);
  // what Copy takes, said before the press (its label) and after (the toast): it is not drawn as it is
  const label = await page.evaluate(() => [document.getElementById("markcopy").title, document.getElementById("markcopy").getAttribute("aria-label")]);
  check(`AC-06 (${name}) Copy says what it takes: the line count and the first line`,
    label.every((l) => l === "Copy 2 lines: The tests pass now."), JSON.stringify(label));
  await spyCopies(page);
  await page.click("#markcopy");
  await pause(page, 200);
  const put = await page.evaluate(() => ({ text: window.copied.at(-1) ?? null, toast: document.getElementById("toast").textContent }));
  check(`AC-06 (${name}) Copy puts the latest copy, exactly as the program made it, and the toast says what`,
    put.text === away && put.toast === "Copied 2 lines: The tests pass now.", JSON.stringify(put));
  await page.click("#markadd");
  await pause(page, 300);
  const box = await page.evaluate(() => document.getElementById("markbox").textContent);
  check(`AC-06 (${name}) [+] holds that copy`, box.includes("scrolled away"), box.slice(0, 200));
  await page.keyboard.press("Escape");
  await pause(page, 200);
  // a copy longer than a bookmark keeps: the dialog shows what is kept, and says it is cut
  await drag([2, 8], [12, 10]);
  f.say({ t: "copied", sid: SID, text: "The tests pass now.\n" + "y".repeat(5000) });
  await pause(page, 200);
  await page.click("#markadd");
  await pause(page, 300);
  const long = await page.evaluate(() => document.getElementById("marktext").textContent);
  check(`AC-06 (${name}) [+] of a long copy shows its first 2000 characters and says so`,
    long.startsWith("The tests pass now.") && long.endsWith("\n… (the first 2,000 characters are kept)") && long.length === 2000 + "\n… (the first 2,000 characters are kept)".length, String(long.length));
  await page.keyboard.press("Escape");
  await pause(page, 200);

  // Shift: the browser's own selection, from the press, in every browser
  await page.click("#filter");
  await page.mouse.click((await cell(4, 3)).x, (await cell(4, 3)).y);   // the terminal has the keys: the caret in its field
  await pause(page, 200);
  check(`AC-06 (${name}) before Shift the terminal has the keys (the caret in its keyboard field)`, await page.evaluate(() => document.activeElement?.id === "kbd"));
  n = mice();
  await drag([2, 0], [12, 1], true);
  await pause(page, 300);
  const sh = await buttons();
  check(`AC-06 (${name}) Shift and a drag select from the press in the browser, nothing sent, the buttons shown`,
    sh.sel.replace(/\s+/g, " ").startsWith("Messages: 3.3k") && sh.sel.includes("Free") && mice() === n && sh.add && sh.ask, JSON.stringify({ sel: sh.sel, sentMice: mice() - n }));
  // a selection over what hides what a line runs: Copy says it leaves it out, and does
  await drag([0, 12], [60, 12], true);
  await pause(page, 300);
  const hid = await page.evaluate(() => document.getElementById("markcopy").title);
  await spyCopies(page);
  await page.click("#markcopy");
  await pause(page, 200);
  const hidPut = await page.evaluate(() => ({ text: window.copied.at(-1) ?? null, toast: document.getElementById("toast").textContent }));
  check(`AC-37 (${name}) a browser selection's Copy leaves out zero-width characters and says so, before and after`,
    hid === "Copy 1 line: rm -rf build (2 hidden characters left out)" && hidPut.text === "  rm -rf build"
    && hidPut.toast === "Copied 1 line: rm -rf build (2 hidden characters left out)", JSON.stringify({ hid, ...hidPut }));
  // the next press goes to the program and ends that selection: the keys come back, the copy is the program's
  n = mice();
  await drag([2, 20], [12, 21]);
  f.say({ t: "copied", sid: SID, text: "Skills: 4.9k tokens (0.5%)\n  Memory files" });
  await pause(page, 300);
  const after = await page.evaluate(() => ({ sel: String(getSelection()), keys: document.activeElement?.id }));
  check(`AC-06 (${name}) a press after a Shift selection goes to the program and ends it; the keyboard field has the keys again`,
    mice() - n >= 2 && after.sel === "" && after.keys === "kbd", JSON.stringify({ ...after, sentMice: mice() - n }));
  await frames(4);
  await page.click("#markadd");
  await pause(page, 300);
  const held = await page.evaluate(() => document.getElementById("markbox").textContent);
  check(`AC-06 (${name}) then [+] holds the program's copy, never the old Shift selection`, held.includes("Memory files") && !held.includes("Free space"), held.replace(/\s+/g, " ").slice(0, 160));
  await page.keyboard.press("Escape");
  check(`AC-06 (${name}) no page errors`, !f.errors.length, f.errors.join(" | "));
  await f.close();
  runningIn(name);
}
