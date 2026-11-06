// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// AC-31 a table widget copied as a PNG, on the fake bridge of e2e_widgets.mjs. The user's clipboard
// is never written: `clipboard.write` and ClipboardItem (absent on this http test page) are spies
// that keep the blob; e2e_livewidgetpng.mjs goes through the browser's own on a real terminumd.

import { check } from "./check.mjs";
import { line, open } from "./e2e_widgets.mjs";

const table = (...rows) => {
  const rule = (l, m, r) => l + rows[0].slice(1, -1).split("│").map((c) => "─".repeat(c.length)).join(m) + r;
  return [rule("┌", "┬", "┐"), ...rows.flatMap((r, i) => (i ? [rule("├", "┼", "┤"), r] : [r])), rule("└", "┴", "┘"), ""].map((t) => line(t));
};
const TABLE = table("│ Server │ Apps │ Notes │", "│ alpha  │ 1000 │ first │", "│ beta   │    3 │ " + "word ".repeat(60).trim() + " │");
const NUMBERS = table("│ Name │ Count │", "│ a    │ 1111 │", "│ b    │    3 │", "│ c    │ 8888 │");
const DIFF = ["⏺ Update(docs/spec.md)", "  ⎿  Updated docs/spec.md (+1 -1)", "      5   | Field | Value |", "      6   |---|---|",
  "      7 - | Version | 0.11.1 |", "      7 + | Version | 0.12.0 |", "      8   | Date | 2026-10-19 |", "", "⏺ Done."].map((t) => line(t));
const LIGHT = { bg: "#ffffff", fg: "#1f2328", cursor: "#000000", cursorText: "#ffffff", bold: "#000000", useBold: false, ansi: Array(16).fill("#333333"), font: null, size: null, dark: false };

/** Clipboard spies: every picture written is kept in window.pngs; refuse: the write rejects. */
const spy = (page, refuse = "") => page.evaluate((refuse) => {
  window.pngs = [];
  Object.defineProperty(window, "isSecureContext", { value: true, configurable: true });
  window.ClipboardItem = class { constructor(parts) { this.parts = parts; } };
  Object.defineProperty(navigator, "clipboard", { configurable: true, value: { write: async ([item]) => {
    window.written = Object.keys(item.parts);
    const png = await item.parts["image/png"];
    if (refuse) throw new Error(refuse);
    window.pngs.push(png);
  } } });
}, refuse);
const press = (page, act = "png") => page.evaluate((act) => document.querySelector("#term .ln.wg-host")?.shadowRoot.querySelector(`[data-act="${act}"]`)?.click(), act);
const shown = (page) => page.waitForFunction(() => document.querySelector("#term .ln.wg-host")?.shadowRoot.querySelector("table"), null, { timeout: 8000 });
const toast = (page) => page.evaluate(() => document.getElementById("toast")?.textContent ?? "");
const says = (page, re) => page.waitForFunction((s) => new RegExp(s).test(document.getElementById("toast")?.textContent ?? ""), re, { timeout: 3000 }).then(() => true, () => false);

/** PNG pressed in `f`, and what the picture holds: per band of rows with ink, its extent; its tints and its strikes. */
const picture = async (f, n = 1, ms = 5000) => {
  const t0 = Date.now();
  await press(f.page);
  await f.page.waitForFunction((n) => window.pngs.length === n, n, { timeout: ms }).catch(() => {});
  const took = Date.now() - t0;
  return { took, ...await f.page.evaluate(async () => {
    const png = window.pngs.at(-1);
    if (!png) return { w: 0, h: 0, bands: [], rows: 1, inked: 0, count: 0 };
    const bitmap = await createImageBitmap(png), { width: w, height: h } = bitmap;
    const ctx = Object.assign(document.createElement("canvas"), { width: w, height: h }).getContext("2d");
    ctx.drawImage(bitmap, 0, 0);
    const { data } = ctx.getImageData(0, 0, w, h), at = (x, y) => (y * w + x) * 4;
    const [r0, g0, b0] = [data[0], data[1], data[2]];       // its paper: ink is far from it, in any theme
    const off = (i) => Math.abs(data[i] - r0) + Math.abs(data[i + 1] - g0) + Math.abs(data[i + 2] - b0);
    const dark = (x, y) => off(at(x, y)) > 250;
    let green = 0, red = 0, inked = 0, strike = 0;
    const bands = [];
    for (let y = 0; y < h; y++) {
      let first = -1, last = -1, run = 0, longest = 0;
      for (let x = 0; x < w; x++) {
        const i = at(x, y);
        if (data[i + 1] - data[i] > 12) green++;
        if (data[i] - data[i + 1] > 8) red++;
        // the struck line is a little lighter than ink (its row's opacity)
        if (off(i) > 120) { run++; longest = Math.max(longest, run); } else run = 0;
        if (dark(x, y)) { if (first < 0) first = x; last = x; }
      }
      strike = Math.max(strike, longest);
      if (first >= 0) inked++;
      const band = bands.at(-1);
      if (first >= 0 && band && band.end === y - 1) { band.end = y; band.first = Math.min(band.first, first); band.last = Math.max(band.last, last); }
      else if (first >= 0) bands.push({ start: y, end: y, first, last });
    }
    const url = await new Promise((resolve) => { const r = new FileReader(); r.onload = () => resolve(r.result); r.readAsDataURL(png); });
    return { count: window.pngs.length, type: png.type, w, h, corner: [...data.slice(0, 4)], bands, inked, rows: h, green, red, strike, url };
  }) };
};

export async function webWidgetPng(browser, name = "") {
  const at = `AC-31 web app${name}:`;
  const opened = async (history, options) => {
    const f = await open(browser, { history, screen: [], ...options });
    await shown(f.page);
    await spy(f.page);
    return f;
  };
  const errors = [];
  const closed = async (f) => { errors.push(...f.errors); await f.page.close(); };

  const f = await opened(TABLE);
  const before = await f.page.evaluate(() => {
    const root = document.querySelector("#term .ln.wg-host").shadowRoot;
    return { kids: root.querySelector(".wg").childElementCount, top: document.getElementById("term").scrollTop,
      bar: [...root.querySelectorAll(".bar button")].map((b) => b.dataset.act).join(), title: root.querySelector('[data-act="png"]')?.title };
  });
  check(`${at} a table widget's bar has PNG right after Copy`, before.bar === "raw,on,copy,png" && !!before.title, JSON.stringify(before));
  const clear = await f.page.evaluate(() => {
    const last = document.querySelector("#term .ln.wg-host").shadowRoot.querySelector('[data-act="png"]').getBoundingClientRect();
    const fabs = ["foldfab", "planfab", "findfab", "splitfab", "sentfab", "attachfab", "marksfab", "remindfab"].map((id) => document.getElementById(id)).filter((e) => e && !e.hidden && e.offsetParent)
      .map((e) => e.getBoundingClientRect()).filter((r) => r.top < last.bottom && r.bottom > last.top);
    return { right: Math.round(last.right), lefts: fabs.map((r) => Math.round(r.left)), h: Math.round(last.height) };
  });
  check(`${at} the bar's last button stands clear of the floating buttons`, clear.lefts.every((x) => clear.right <= x), JSON.stringify(clear));
  const wide = await picture(f);
  check(`${at} PNG puts a PNG on the clipboard, nothing else`, wide.type === "image/png" && (await f.page.evaluate(() => window.written.join())) === "image/png", wide.type);
  const paper = await f.page.evaluate(() => { const c = document.createElement("canvas").getContext("2d"); c.fillStyle = getComputedStyle(document.querySelector("#term .ln.wg-host")).getPropertyValue("--t-bg").trim(); c.fillRect(0, 0, 1, 1); return [...c.getImageData(0, 0, 1, 1).data].join(); });
  check(`${at} the picture is opaque, on the shown theme's background`, wide.corner.join() === paper && wide.corner[3] === 255, JSON.stringify([wide.corner, paper]));
  check(`${at} the picture holds the table's text, far from its background`, wide.inked > wide.rows / 4, `${wide.inked} of ${wide.rows} rows`);
  check(`${at} a long cell wraps at 800 px, twice over: the picture is 1600 px wide`, wide.w === 1600, `${wide.w}×${wide.h}`);
  check(`${at} the toast says so`, /^Copied the table as PNG\.$/.test(await toast(f.page)), await toast(f.page));
  const after = await f.page.evaluate(() => {
    const root = document.querySelector("#term .ln.wg-host").shadowRoot;
    return { kids: root.querySelector(".wg").childElementCount, top: document.getElementById("term").scrollTop };
  });
  check(`${at} the widget and the view do not change`, after.kids === before.kids && after.top === before.top, JSON.stringify(after));

  await press(f.page, "raw");
  await f.page.waitForFunction(() => document.querySelector("#term .ln.wg-host")?.classList.contains("wg-raw"), null, { timeout: 3000 }).catch(() => {});
  const raw = await picture(f, 2);
  check(`${at} in Raw it gives the same picture`, raw.count === 2 && raw.url === wide.url, `${raw.count}: ${raw.w}×${raw.h}`);
  await closed(f);

  const phone = await opened(TABLE, { device: { viewport: { width: 390, height: 844 } } });
  check(`${at} a phone's bar buttons are a finger's: 40 px`, (await phone.page.evaluate(() => document.querySelector("#term .ln.wg-host").shadowRoot.querySelector('[data-act="copy"]').getBoundingClientRect().height)) >= 40);
  const narrow = await picture(phone);
  check(`${at} a phone's pane gives the same picture as a computer's`, narrow.url === wide.url, `${narrow.w}×${narrow.h}`);
  await closed(phone);

  const light = await opened(TABLE, { hook: (m, say) => { if (m.t === "sub") say({ t: "theme", sid: "S1", theme: LIGHT }); } });
  const bright = await picture(light);
  check(`${at} a light theme gives the same table on its own background`, bright.corner.join() === "255,255,255,255" && bright.url !== wide.url && bright.w === wide.w && bright.h === wide.h && bright.inked > bright.rows / 4, JSON.stringify([bright.corner, bright.w, bright.h]));
  await closed(light);

  // geometry: numbers to the right whatever their width, in their cells (the head is lighter than this counts)
  const n = await opened(NUMBERS);
  const nums = await picture(n), rows = nums.bands.slice(-3);     // the head stands above them
  check(`${at} numbers end at one right edge, however many digits`, rows.length === 3 && rows.every((b) => Math.abs(b.last - rows[0].last) <= 6), JSON.stringify(rows));
  check(`${at} its text keeps inside the picture, with its margin`, rows[0]?.first >= 16 && rows[0]?.last <= nums.w - 16, `${rows[0]?.first}…${rows[0]?.last} of ${nums.w}`);
  await closed(n);

  // words too long for 800 px: the picture is as wide as the table, its right margin kept
  const word = "abcdefghijklmnop";
  const w = await opened(table(`│ ${Array(9).fill("Head").join(" │ ")} │`, `│ ${Array(9).fill(word).join(" │ ")} │`));
  const wider = await picture(w), lastRow = wider.bands.at(-1);
  check(`${at} a table wider than 800 px keeps its right margin`, wider.w > 1600 && lastRow?.last <= wider.w - 16, `${lastRow?.last} of ${wider.w}`);
  await closed(w);

  // a diff's table: a removed row struck, an added one tinted
  const d = await opened(DIFF);
  const diff = await picture(d);
  check(`${at} a diff's rows keep their tints in the picture`, diff.green > 500 && diff.red > 500, JSON.stringify({ green: diff.green, red: diff.red }));
  check(`${at} a removed row's text is struck through, its whole width`, diff.strike > 60, `${diff.strike} px`);
  await closed(d);

  const long = (count) => opened(table("│ Name │ N │ Notes │", ...Array.from({ length: count }, (_, i) => `│ r${i} │ ${i} │ ${"word ".repeat(60).trim()} │`)));
  // too long for twice its size: less, whole, within the canvas's limit
  const tall = await long(110);
  const fewer = await picture(tall, 1, 30000);
  check(`${at} a table too long for twice its size is drawn smaller, whole`, fewer.w > 800 && fewer.w < 1600 && fewer.w * fewer.h <= 16.1e6 && fewer.inked > fewer.rows / 4, `${fewer.w}×${fewer.h}, ${fewer.took} ms`);
  check(`${at} still larger than its size: the toast says nothing of it`, await says(tall.page, "^Copied the table as PNG\\.$"), await toast(tall.page));
  await closed(tall);
  // too long even for its size: the canvas's side holds, and the toast says it is smaller
  const large = await long(330);
  const big = await picture(large, 1, 30000);
  check(`${at} a table too long for its size is drawn smaller still, whole`, big.w > 300 && big.w < 800 && big.h <= 16384 && big.inked > big.rows / 4, `${big.w}×${big.h}, ${big.took} ms`);
  check(`${at} and the toast says it is smaller`, await says(large.page, "smaller than its size"), await toast(large.page));
  await closed(large);

  // refused, and no clipboard at all: the toast says so
  const refused = await open(browser, { history: TABLE, screen: [] });
  await shown(refused.page);
  await press(refused.page);
  check(`${at} a page without the clipboard's API says so`, await says(refused.page, "HTTPS"), await toast(refused.page));
  await spy(refused.page, "denied by the test");
  await press(refused.page);
  check(`${at} a refused picture says why`, await says(refused.page, "not copied: denied by the test"), await toast(refused.page));
  await closed(refused);

  // only the table widget: code keeps its Copy alone
  const c = await open(browser, { history: ["```python", "print(1)", "```", ""].map((t) => line(t)), screen: [] });
  await c.page.waitForFunction(() => document.querySelector("#term .ln.wg-host")?.shadowRoot.querySelector(".bar"), null, { timeout: 8000 }).catch(() => {});
  check(`${at} code has no PNG`, await c.page.evaluate(() => { const r = document.querySelector("#term .ln.wg-host")?.shadowRoot; return !!r?.querySelector(".bar") && !r.querySelector('[data-act="png"]'); }));
  await closed(c);
  check(`${at} no page errors`, !errors.length, errors.join(" | "));
}
