// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// AC-56 checks of what programs print, drawn by the web app's widgets on the fake bridge of
// e2e_widgets.mjs: an indented document and a Markdown table in an agent's tool output, a path the
// program broke at the row's edge (one path again), a table Claude Code drew 120 columns wide read
// on a phone, and a pipe table wrapped at a phone's width.

import { check } from "./check.mjs";
import { act, line, open } from "./e2e_widgets.mjs";

export async function webWidgetTables(browser, name = "") {
  const at = `AC-56 web app${name}:`;
  // a document a coding agent's tool printed is indented: rendered as Markdown, not as one code block
  const tool = ["⏺ Bash(sed -n 1,20p docs/RELEASING.md)", "  ⎿  # Releasing", "", "     How releases are signed.", "",
    "     ## Signed releases", "", "     Installs check `SHA256SUMS`:", "", "     ```bash", "     make signing-key", "     ```", "",
    "     - one", "     - two", "", "⏺ Done."].map((t) => line(t));
  const ind = await open(browser, { history: tool, screen: [] });
  await ind.page.waitForFunction(() => document.querySelector("#hist .ln.wg-host")?.shadowRoot.querySelector(".body.markdown h2"), null, { timeout: 8000 }).catch(() => {});
  const md2 = await ind.page.evaluate(() => {
    const b = document.querySelector("#hist .ln.wg-host")?.shadowRoot.querySelector(".body.markdown");
    return { h2: b?.querySelector("h2")?.textContent, li: b?.querySelectorAll("li").length, pres: b?.querySelectorAll("pre").length };
  });
  check(`${at} a document a coding agent's tool printed (indented) is rendered as Markdown, its fence as code`,
    md2.h2 === "Signed releases" && md2.li === 2 && md2.pres === 1, JSON.stringify(md2));
  await ind.page.close();

  // a Markdown table in Claude Code's diff of an edit: drawn as a table, removed rows struck
  const diff = ["⏺ Update(docs/spec.md)", "  ⎿  Updated docs/spec.md (+1 -1)", "      5   | Field | Value |", "      6   |---|---|",
    "      7 - | Version | 0.11.1 |", "      7 + | Version | 0.12.0 |", "      8   | Date | 2026-10-19 |", "", "⏺ Done."].map((t) => line(t));
  const dt = await open(browser, { history: diff, screen: [] });
  await dt.page.waitForFunction(() => document.querySelector("#hist .ln.wg-host")?.shadowRoot.querySelector("table"), null, { timeout: 8000 }).catch(() => {});
  const tbl = await dt.page.evaluate(() => {
    const t = document.querySelector("#hist .ln.wg-host")?.shadowRoot.querySelector("table");
    return { head: [...(t?.querySelectorAll("th") ?? [])].map((c) => c.textContent), rows: [...(t?.querySelectorAll("tbody tr") ?? [])].map((r) => `${r.className}:${r.cells[1]?.textContent}`) };
  });
  check(`${at} a Markdown table in an agent's diff is drawn as a table: its header, a removed row struck, an added one marked`,
    tbl.head.join() === "Field,Value" && tbl.rows.join() === "del:0.11.1,add:0.12.0,:2026-10-19", JSON.stringify(tbl));
  await dt.page.close();

  // a path longer than the row, broken by the program at its edge: one path again
  const dir = "/tmp/" + "deep-folder-name-".repeat(5) + "x", long = `${dir}/drill-results.png`;
  const cut = 78, broken = await open(browser, { screen: ["Here it is:", `  ${long.slice(0, cut)}`, `  ${long.slice(cut)}`, "done"],
    files: { [dir]: ["drill-results.png"] } });
  await broken.page.waitForFunction(() => CSS.highlights?.get("lnk")?.size, null, { timeout: 8000 }).catch(() => {});
  // measured while the terminal is in sight: the file opened below takes its place (View), and the
  // underlines of a hidden terminal are dropped as soon as they are looked at again
  const glue = await broken.page.evaluate(() => {
    const rows = [...document.querySelectorAll("#screen .ln")];
    const lnk = [...(CSS.highlights?.get("lnk") ?? [])].map((r) => [rows.indexOf(r.startContainer.parentElement.closest(".ln")), rows.indexOf(r.endContainer.parentElement.closest(".ln"))]);
    return { glue: rows[1].classList.contains("glue"), lnk, shadow: rows.filter((el) => el.shadowRoot).length };
  });
  const second = await broken.page.evaluate(() => {
    const row = [...document.querySelectorAll("#screen .ln")][2], walk = document.createTreeWalker(row, NodeFilter.SHOW_TEXT);
    let t = walk.nextNode();
    while (t && !t.data.includes("drill")) t = walk.nextNode();
    const r = document.createRange(), i = t.data.indexOf("drill");
    r.setStart(t, i + 2); r.setEnd(t, i + 3);
    const b = r.getBoundingClientRect();
    return { x: b.left + b.width / 2, y: b.top + b.height / 2 };
  });
  const mod = process.platform === "darwin" ? "Meta" : "Control";
  await broken.page.keyboard.down(mod);
  await broken.page.mouse.click(second.x, second.y);
  await broken.page.keyboard.up(mod);
  await broken.page.waitForFunction(() => document.getElementById("file").getAttribute("src"), null, { timeout: 5000 }).catch(() => {});
  const openedLong = await broken.page.evaluate(() => new URLSearchParams(new URL(document.getElementById("file").src || "http://x/").search).get("view"));
  // the two rows selected and copied as one path
  await broken.page.evaluate(() => {
    const rows = [...document.querySelectorAll("#screen .ln")], sel = getSelection(), r = document.createRange();
    r.setStart(rows[1], 0); r.setEnd(rows[2], rows[2].childNodes.length);
    sel.removeAllRanges(); sel.addRange(r);
  });
  await broken.page.evaluate(() => { window.copied = []; document.addEventListener("copy", (e) => window.copied.push(e.clipboardData?.getData("text/plain"))); });
  await broken.page.keyboard.press(process.platform === "darwin" ? "Meta+C" : "Control+C");
  const copiedLong = await broken.page.evaluate(() => window.copied.at(-1));
  check(`${at} a path the program broke at the row's edge is one path again: one underline (no button after it), a click on its second half, copied without a space`,
    glue.glue && glue.shadow === 0 && glue.lnk.some(([a, b]) => a === 1 && b === 2) && openedLong === long && copiedLong?.trim() === long,
    JSON.stringify({ glue, openedLong, copiedLong }));
  await broken.page.close();
  // a tool's output Claude Code cut by characters at cols - 5 (measured): one path again, though shorter than the row
  const row = "  ⎿ \u00a0-rw-r----- 1 root app 1041 Oct 14 14:51 /etc/devbox-example-consoles/e";
  const cutOut = await open(browser, { screen: ["⏺ Bash(ls -l /etc/devbox-example-consoles/env)", row, "     nv", "     … +3 lines (ctrl+o to expand)"] });
  const cutRows = await cutOut.page.evaluate(() => { const r = [...document.querySelectorAll("#screen .ln")]; return { glue: r[1].classList.contains("glue"), text: r[1].parentElement.textContent }; });
  check(`${at} a tool's output cut inside a path at cols - 5 joins without a space`,
    row.length === 75 && cutRows.glue && /consoles\/env/.test(cutRows.text.replace(/\s{2,}/g, "")), JSON.stringify(cutRows));
  await cutOut.page.close();

  // a table Claude Code drew 120 columns wide (its code colored), read on a phone: an HTML table
  const pad = (t, n) => t + " ".repeat(n - [...t].length);
  const rule = (l, j, r) => `  ${l}${"─".repeat(11)}${j}${"─".repeat(106)}${r}`;
  const cellRow = (a, b, color) => ({ r: [["  │ ", null, null, 0], [pad(a, 10), color ? 8 : null, null, 0], ["│ ", null, null, 0], [pad(b, 105), null, null, 0], ["│", null, null, 0]], e: true });
  const drawn = [line("⏺ The commits:"), line(""), line(rule("┌", "┬", "┐")), cellRow("Commit", "What"), line(rule("├", "┼", "┤")),
    cellRow("eecb6df", "Web access: port 48080 by default, and an honest setup behind tailscale serve; a README section", true),
    cellRow("", "about Tailscale | and its services"), line(rule("├", "┼", "┤")), cellRow("7962f12", "Host and attach actions", true),
    line(rule("└", "┴", "┘")), line(""), line("Done.")];
  const tb = await open(browser, { history: drawn, screen: [], device: { viewport: { width: 390, height: 844 }, hasTouch: true } });
  await tb.page.waitForFunction(() => document.querySelector("#hist .ln.wg-host")?.shadowRoot.querySelector(".body.table td"), null, { timeout: 8000 }).catch(() => {});
  const table = await tb.page.evaluate(() => {
    const host = document.querySelector("#hist .ln.wg-host"), b = host?.shadowRoot.querySelector(".body.table"), t = document.getElementById("term");
    return { head: [...(b?.querySelectorAll("th") ?? [])].map((c) => c.textContent), cells: [...(b?.querySelectorAll("tbody tr") ?? [])].map((r) => [...r.cells].map((c) => c.textContent)),
      hidden: document.querySelectorAll("#hist .ln.wg-hid").length, name: host?.shadowRoot.querySelector(".name").textContent,
      // Wrap's overflow-wrap: anywhere would cut "eecb6df" letter by letter
      whole: (() => { const r = document.createRange(); r.selectNodeContents(b.querySelector("td")); return new Set([...r.getClientRects()].map((x) => Math.round(x.top))).size; })(),
      fits: (b?.getBoundingClientRect().right ?? 1e9) <= t.getBoundingClientRect().right + 1 && t.scrollWidth <= t.clientWidth + 1 };
  });
  check(`${at} a table Claude Code drew (its code colored) is an HTML table on a phone: its head, a wrapped cell one cell, its other rows hidden, a word never cut, within the view`,
    table.name === "Table" && table.head.join("|") === "Commit|What" && table.cells.length === 2 && table.cells[0][0] === "eecb6df"
    && table.cells[0][1] === "Web access: port 48080 by default, and an honest setup behind tailscale serve; a README section about Tailscale | and its services"
    && table.hidden === 7 && table.whole === 1 && table.fits, JSON.stringify(table));
  // the design code inside: no frame around the card or the table, no column lines; a hairline between rows only
  const look = await tb.page.evaluate(() => {
    const r = document.querySelector("#hist .ln.wg-host").shadowRoot, cs = (e) => getComputedStyle(e);
    const card = cs(r.querySelector(".wg")), td = cs(r.querySelector("tbody td")), last = cs(r.querySelector("tbody tr:last-child td"));
    return { card: card.borderTopWidth, side: [td.borderLeftWidth, td.borderRightWidth, td.borderTopWidth], row: td.borderBottomWidth, last: last.borderBottomWidth,
      fill: card.backgroundColor !== cs(document.getElementById("term")).backgroundColor };
  });
  check(`${at} a table widget has no frame and no column lines: the card stands on its own surface, rows apart by a hairline`,
    look.card === "0px" && look.side.every((w) => w === "0px") && look.row === "1px" && look.last === "0px" && look.fill, JSON.stringify(look));
  await tb.page.evaluate(() => { window.copied = []; document.addEventListener("copy", () => window.copied.push(String(getSelection()) || document.activeElement?.value)); });
  await act(tb.page, "table", '[data-act="copy"]');
  const tcopied = await tb.page.evaluate(() => window.copied);
  check(`${at} … Copy copies it as a Markdown table`, tcopied.some((c) => c?.startsWith("| Commit | What |\n| --- | --- |\n| eecb6df | Web access:") && c.includes("Tailscale \\| and")), JSON.stringify(tcopied));
  await act(tb.page, "table", '[data-act="raw"]');
  await tb.page.waitForFunction(() => document.querySelector("#hist .ln.wg-host.wg-raw"), null, { timeout: 5000 }).catch(() => {});
  check(`${at} … Raw shows the drawn lines`, await tb.page.evaluate(() => document.querySelectorAll("#hist .ln.wg-hid").length === 0 && !!document.querySelector("#hist .ln.wg-raw")));
  await tb.page.close();

  // a pipe table Claude Code printed at a phone's 44 columns: each padded row wrapped at a bar in its middle
  // and into blank lines, the separator over four lines (the owner's phone, 0.6.17): one HTML table
  const narrowRows = ["⏺ Статусы:", "", "  | Статус                           |", "  Что видит пользователь", "", "           |",
    "  |----------------------------------|-------", "  ------------------------------------------", "  ----------|",
    "  | защищено                         |", "  «Защищено», время последней проверки", "", "", "           |",
    "  | неподдерживаемый Node | «Обновите Node.js до 16+» |", "", "Done."].map(line);
  const pt = await open(browser, { history: narrowRows, screen: [], device: { viewport: { width: 390, height: 844 }, hasTouch: true } });
  await pt.page.waitForFunction(() => document.querySelector("#hist .ln.wg-host")?.shadowRoot.querySelector(".body.table td"), null, { timeout: 8000 }).catch(() => {});
  const piped = await pt.page.evaluate(() => { const b = document.querySelector("#hist .ln.wg-host")?.shadowRoot.querySelector(".body.table");
    return { head: [...(b?.querySelectorAll("th") ?? [])].map((c) => c.textContent), cells: [...(b?.querySelectorAll("tbody tr") ?? [])].map((r) => [...r.cells].map((c) => c.textContent)),
      hidden: document.querySelectorAll("#hist .ln.wg-hid").length }; });
  check(`${at} a pipe table Claude Code wrapped at a phone's width is one HTML table: rows joined at their bars, blank lines and all`,
    piped.head.join("|") === "Статус|Что видит пользователь" && JSON.stringify(piped.cells) === JSON.stringify([["защищено", "«Защищено», время последней проверки"], ["неподдерживаемый Node", "«Обновите Node.js до 16+»"]])
    && piped.hidden === 12, JSON.stringify(piped));
  await pt.page.close();
}
