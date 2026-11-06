// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// View against a real private terminumd (e2e_live.mjs), wide: its head, path bar and tab row on
// one grid (text 12 px from the left, keys 24 px, 6 px above, below and right of them), no Terminal
// key (its × closes it); an image tab's picture in the middle, the checkerboard behind it alone, a
// click opens it full screen in the page's viewer and Esc closes it; Reveal in tree expands the
// file's folders and selects it, and is not offered for a file outside the tree's folder (AC-64, AC-67, AC-70).
import fs from "node:fs";
import path from "node:path";
import zlib from "node:zlib";
import { check, within } from "./check.mjs";
import { shortTmp } from "./testrun.mjs";

/** A small PNG (w×h, one gray), written by hand: no image library here. */
function png(w, h) {
  const table = Array.from({ length: 256 }, (_, n) => { let c = n; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; return c >>> 0; });
  const crc = (b) => { let c = 0xffffffff; for (const x of b) c = table[(c ^ x) & 255] ^ (c >>> 8); return (c ^ 0xffffffff) >>> 0; };
  const chunk = (type, data) => { const td = Buffer.concat([Buffer.from(type), data]), n = Buffer.alloc(4), c = Buffer.alloc(4); n.writeUInt32BE(data.length); c.writeUInt32BE(crc(td)); return Buffer.concat([n, td, c]); };
  const head = Buffer.alloc(13);
  head.writeUInt32BE(w, 0); head.writeUInt32BE(h, 4); head[8] = 8; head[9] = 0;   // 8-bit gray
  const rows = Buffer.alloc((w + 1) * h, 128);
  for (let y = 0; y < h; y++) rows[y * (w + 1)] = 0;
  return Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk("IHDR", head), chunk("IDAT", zlib.deflateSync(rows)), chunk("IEND", Buffer.alloc(0))]);
}

export async function liveView(page, { work }) {
  fs.mkdirSync(path.join(work, "sub/deep"), { recursive: true });
  fs.writeFileSync(path.join(work, "sub/deep/note.txt"), "deep\n");
  fs.writeFileSync(path.join(work, "pic.png"), png(320, 200));
  // a picture that holds script: run in the page's origin (opened as a page) it would reach the sign-in
  fs.writeFileSync(path.join(work, "mark.svg"), '<svg xmlns="http://www.w3.org/2000/svg" width="40" height="40"><script>localStorage.setItem("pwned","svg")</script><rect width="40" height="40"/></svg>');
  const away = fs.realpathSync(fs.mkdtempSync(`${shortTmp()}/tmlo-`)), outside = path.join(away, "outside.txt");
  fs.writeFileSync(outside, "x\n");
  try {
    const files = await (await page.$("#files")).contentFrame();
    let view = null;
    /** A file into View, as a click in Files does. */
    const open = async (p) => {
      await files.evaluate((p) => parent.postMessage({ type: "fb-open", path: p }, location.origin), p);
      view = await (await page.$("#file")).contentFrame();
      await view.waitForFunction((n) => document.querySelector(".tab.active .tname")?.textContent === n, path.basename(p), { timeout: 5000 });
    };
    await within("AC-64 a picture opens in a View tab", async () => {
      await open(path.join(work, "pic.png"));
      await view.waitForFunction(() => document.querySelector(".imgview img")?.naturalWidth > 0, null, { timeout: 5000 });
    });
    const head = await page.evaluate(() => {
      const win = document.getElementById("filewin").getBoundingClientRect(), h = document.querySelector("#filewin .winhead").getBoundingClientRect();
      const t = document.querySelector("#filewin .winhead span").getBoundingClientRect(), x = document.getElementById("fileclose").getBoundingClientRect();
      return { text: Math.round(t.left - win.left), key: [x.width, x.height], right: Math.round(win.right - x.right), above: Math.round(x.top - h.top), below: Math.round(h.bottom - x.bottom) };
    });
    const rows = await view.evaluate(() => {
      const r = (s) => document.querySelector(s).getBoundingClientRect(), W = innerWidth, bar = r("#pathbar"), copy = r("#pathbar button"), tabs = r(".vbar"), tool = r('#tools [data-act="reveal"]');
      return { path: { text: Math.round(r("#pathbar .path").left), key: [copy.width, copy.height], right: Math.round(W - copy.right), above: Math.round(copy.top - bar.top), below: Math.round(bar.bottom - copy.bottom) },
        tabs: { text: Math.round(r(".tab > :first-child").left), key: [tool.width, tool.height], right: Math.round(W - tool.right), above: Math.round(tool.top - tabs.top), below: Math.round(tabs.bottom - tool.bottom), oneRow: tool.top < r(".tab.active").bottom } };
    });
    const grid = [head, rows.path, rows.tabs];
    const back = await page.evaluate(() => getComputedStyle(document.getElementById("backterm")).display);
    check("AC-70 View's head, path bar and tab row are on one grid: text 12 px in, keys 24 px with 6 px above, below and right; Reveal beside the tabs; no Terminal key",
      grid.every((g) => g.text === 12 && g.key[0] === 24 && g.key[1] === 24 && g.right === 6 && g.above === 6 && g.below === 6) && rows.tabs.oneRow && back === "none", JSON.stringify({ grid, back }));
    const pic = await view.evaluate(() => {
      const v = document.querySelector(".imgview"), i = v.querySelector("img"), a = v.getBoundingClientRect(), b = i.getBoundingClientRect();
      return { left: Math.round(b.left - a.left), right: Math.round(a.right - b.right), behind: getComputedStyle(i).backgroundImage.startsWith("repeating-conic"), view: getComputedStyle(v).backgroundImage };
    });
    check("AC-64 the picture stands in the middle, the checkerboard behind it alone (its transparent parts within its edges)",
      Math.abs(pic.left - pic.right) <= 1 && pic.behind && pic.view === "none", JSON.stringify(pic));
    await view.click(".imgview img");
    const zoom = await page.waitForFunction(() => { const z = document.getElementById("zoom"); return z && !z.hidden && z.querySelector("img")?.naturalWidth > 0 && z.querySelector(".zcap").textContent; }, null, { timeout: 3000 })
      .then((h) => h.jsonValue(), () => null);
    // the sign-in goes only in a header: both pictures stand at blob: addresses, none holds it (AC-09)
    const secret = await page.evaluate(() => localStorage.getItem("tm.session"));
    const srcs = [await view.evaluate(() => document.querySelector(".imgview img").getAttribute("src")), await page.evaluate(() => document.querySelector("#zoom img")?.getAttribute("src"))];
    check("AC-09 a picture's address holds no sign-in: the tab's and the full screen's are blob: addresses",
      srcs.every((s) => s?.startsWith("blob:") && !s.includes(secret)), JSON.stringify(srcs));
    await page.keyboard.press("Escape");
    const closed = await page.evaluate(() => document.getElementById("zoom").hidden);
    check("AC-64 a click on the picture opens it full screen in the page's viewer (zoom, pan); Esc closes it", zoom === "pic.png" && closed, JSON.stringify({ zoom, closed }));
    // an SVG (a picture full screen, a terminal's image button: signin.js blobUrl) is shown from a data:
    // address, an origin of its own; opened as a page (Open Image in New Tab) it runs nothing in the page's (AC-09)
    const svg = await page.evaluate((p) => import("/signin.js").then((m) => m.blobUrl(`/fb/api/raw?path=${encodeURIComponent(p)}`)), path.join(work, "mark.svg"));
    if (svg?.url) {
      const tab = await page.context().newPage();
      await tab.goto(svg.url).catch(() => {});
      await tab.waitForTimeout(300);
      await tab.close();
    }
    const pwned = await page.evaluate(() => localStorage.getItem("pwned"));
    check("AC-09 an SVG picture stands at a data: address, and opened as a page runs nothing in the page's origin",
      !!svg?.url?.startsWith("data:image/svg+xml") && !pwned, JSON.stringify({ svg: svg?.url?.slice(0, 30) ?? svg, pwned }));

    await open(path.join(work, "sub/deep/note.txt"));
    await view.click('#tools [data-act="reveal"]');
    const shown = await files.waitForFunction(() => {
      const names = [...document.querySelectorAll("#tree .row")].map((e) => e.querySelector(".name")?.textContent ?? e.textContent);
      const sel = document.querySelector("#tree .row.sel");
      return sel && /note\.txt/.test(sel.textContent) && names.some((n) => /^deep/.test(n)) ? names.slice(0, 4) : null;
    }, null, { timeout: 5000 }).then((h) => h.jsonValue(), () => null);
    check("AC-67 Reveal in tree expands the file's folders and selects it", !!shown, JSON.stringify(shown));
    await open(outside);
    const offered = await view.evaluate(() => !!document.querySelector('#tools [data-act="reveal"]'));
    await open(path.join(work, "pic.png"));
    const back2 = await view.evaluate(() => !!document.querySelector('#tools [data-act="reveal"]'));
    check("AC-67 Reveal in tree is not offered for a file outside the tree's folder, and is again for one in it", !offered && back2, JSON.stringify({ offered, back2 }));
    // Esc in View hides it as its × does and gives the terminal the keys; an Esc the editor's search panel takes closes only that (AC-70)
    await open(path.join(work, "sub/deep/note.txt"));
    const shownView = () => page.evaluate(() => getComputedStyle(document.getElementById("filewin")).display !== "none" && !document.getElementById("filewin").hidden);
    await view.click(".cm-content");
    await page.keyboard.press(process.platform === "darwin" ? "Meta+f" : "Control+f");
    await view.waitForSelector(".cm-panels .cm-search", { timeout: 3000 }).catch(() => {});
    await page.keyboard.press("Escape");
    await page.waitForTimeout(200);
    const afterSearch = { panel: await view.evaluate(() => !!document.querySelector(".cm-panels .cm-search")), shown: await shownView() };
    await view.click(".cm-content");
    await page.keyboard.press("Escape");
    await page.waitForFunction(() => getComputedStyle(document.getElementById("filewin")).display === "none" || document.getElementById("filewin").hidden, null, { timeout: 3000 }).catch(() => {});
    const afterEsc = { shown: await shownView(), focus: await page.evaluate(() => document.activeElement?.id ?? null), tabs: await view.evaluate(() => document.querySelectorAll(".tab").length) };
    check("AC-70 Esc in View's search panel closes only the panel; another Esc hides View and the terminal takes the keys, its tabs kept",
      !afterSearch.panel && afterSearch.shown && !afterEsc.shown && afterEsc.focus === "kbd" && afterEsc.tabs > 0, JSON.stringify({ afterSearch, afterEsc }));
    // a file opened from the tree, the keys still there: Esc in the tree hides View and leaves them there
    await open(path.join(work, "sub/deep/note.txt"));
    await files.focus("#tree");
    await page.keyboard.press("Escape");
    await page.waitForFunction(() => getComputedStyle(document.getElementById("filewin")).display === "none" || document.getElementById("filewin").hidden, null, { timeout: 3000 }).catch(() => {});
    const fromTree = { shown: await shownView(), page: await page.evaluate(() => document.activeElement?.id ?? null), tree: await files.evaluate(() => !!document.activeElement?.closest("#tree")) };
    check("AC-70 Esc in the Files tree hides View, the keys staying in the tree", !fromTree.shown && fromTree.page === "files" && fromTree.tree, JSON.stringify(fromTree));
  } finally {
    fs.rmSync(away, { recursive: true, force: true });
  }
}
