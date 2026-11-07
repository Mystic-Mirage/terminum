// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// AC-36 against a real private terminumd (e2e_live.mjs): a screenshot pasted into the
// terminal is uploaded to ~/.terminum/tmp, never typed as text, and its path is pasted into
// the shell, which then has it as one word; files dropped from Finder put their own paths; a file
// over 100 MB not found so is not sent: the page says why.
import fs from "node:fs";
import path from "node:path";
import { check, within } from "./check.mjs";

// a 1×1 PNG, and a 1×1 baseline TIFF (what Preview and Safari may put on the clipboard)
const PNG = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
function tiff() {
  const tags = [[256, 3, 1, 1], [257, 3, 1, 1], [258, 3, 3, 134], [259, 3, 1, 1], [262, 3, 1, 2], [273, 4, 1, 140],
    [277, 3, 1, 3], [278, 3, 1, 1], [279, 4, 1, 3]];
  const b = Buffer.alloc(143);
  b.write("II", 0); b.writeUInt16LE(42, 2); b.writeUInt32LE(8, 4); b.writeUInt16LE(tags.length, 8);
  tags.forEach(([tag, type, count, value], i) => {
    const at = 10 + i * 12;
    b.writeUInt16LE(tag, at); b.writeUInt16LE(type, at + 2); b.writeUInt32LE(count, at + 4);
    if (type === 3 && count === 1) b.writeUInt16LE(value, at + 8); else b.writeUInt32LE(value, at + 8);
  });
  b.writeUInt32LE(0, 118);                                 // no next IFD
  [8, 8, 8].forEach((v, i) => b.writeUInt16LE(v, 134 + i * 2));
  b.set([200, 40, 40], 140);                                // one red pixel
  return b.toString("base64");
}
const TIFF = tiff();

/** A paste event carrying one file, as Safari and Chrome give one for a screenshot. */
const pasteFile = (page, b64, type, name) => page.evaluate(([b64, type, name]) => {
  const bytes = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
  const dt = new DataTransfer();
  dt.items.add(new File([bytes], name, { type }));
  document.getElementById("kbd").focus();
  document.getElementById("kbd").dispatchEvent(new ClipboardEvent("paste", { clipboardData: dt, bubbles: true, cancelable: true }));
}, [b64, type, name]);

const shots = (home) => { try { return fs.readdirSync(path.join(home, "tmp")).filter((f) => !f.endsWith(".part")); } catch { return []; } };

/** Types `printf '<%s>\n' `, pastes the picture, presses Enter: the shell prints the one word. */
async function pasteInto(page, home, b64, type, name, tag) {
  const before = shots(home).length;
  await page.keyboard.type(`printf '${tag}<%s>\\n' `);
  await pasteFile(page, b64, type, name);
  for (let i = 0; i < 50 && shots(home).length === before; i++) await new Promise((r) => setTimeout(r, 100));
  // Enter only once its path stands on the line, however long a busy machine takes to echo it
  await page.waitForFunction((tag) => new RegExp(`${tag}<%s>.*/tmp/`).test(document.getElementById("screen").innerText), tag, { timeout: 15000 }).catch(() => {});
  await page.keyboard.press("Enter");
  return page.waitForFunction((tag) => {
    const m = document.getElementById("screen").innerText.match(new RegExp(`${tag}<(/[^>\\n]*)>`));
    return m && m[1];
  }, tag, { timeout: 15000 }).then((h) => h.jsonValue()).catch(() => null);
}

/** A drop on the page of files [name, size in bytes, modified at (ms)]. */
const dropFiles = (page, files) => page.evaluate((files) => {
  const dt = new DataTransfer();
  for (const [fname, size, mtime] of files) dt.items.add(new File([new Uint8Array(size)], fname, { lastModified: mtime }));
  for (const type of ["dragover", "drop"]) document.getElementById("term").dispatchEvent(new DragEvent(type, { dataTransfer: dt, bubbles: true, cancelable: true }));
}, files);
const dropFile = (page, fname, size, mtime) => dropFiles(page, [[fname, size, mtime]]);
const NOT_OWN = "Their own paths were not found: the drag held no file on this Mac's disk (a drag from Mail, a browser or an app's window has none).";

/** Files dropped (AC-36): on this Mac's pane their own paths, at any size, when the drag pasteboard
 *  has them (`drag`: the stub's answer); else uploaded, a file over 100 MB not sent, the page saying why. */
export async function liveDrop(page, { home, work, drag }) {
  // two files dragged from Finder, one over 100 MB: their own paths, nothing uploaded
  const own = [path.join(work, "Drums Initial.zip"), path.join(work, "Drums Grade 1.zip")];
  fs.writeFileSync(own[0], "PK");
  fs.writeFileSync(own[1], ""); fs.truncateSync(own[1], (100 << 20) + 1);
  fs.writeFileSync(drag, JSON.stringify(own.map((p) => [p, path.basename(p)])));   // [path, its name composed], as dragged.rs reads it
  const at = own.map((p) => [path.basename(p), fs.statSync(p).size, Math.round(fs.statSync(p).mtimeMs)]);
  const none = shots(home).length;
  await dropFiles(page, at);
  const pasted = await page.waitForFunction((want) => document.getElementById("screen").innerText.replace(/\n/g, "").includes(want),
    own.map((p) => `'${p}'`).join(" "), { timeout: 5000 }).then(() => true).catch(() => false);
  await page.keyboard.press("Control+U");
  check("AC-36 two files dropped from Finder on a pane of this Mac put their own paths (one over 100 MB), nothing uploaded",
    pasted && shots(home).length === none, JSON.stringify({ pasted, uploaded: shots(home).length - none }));
  // the pasteboard holds an earlier drag's paths: a copy uploaded, the toast naming the case first
  await dropFile(page, "Other Notes.txt", 5, 1_792_000_000_000);
  const earlier = await page.waitForFunction(() => document.getElementById("toast").innerText.startsWith("Their own paths"), null, { timeout: 5000 })
    .then(() => page.evaluate(() => document.getElementById("toast").innerText)).catch(() => "");
  const copied = await page.waitForFunction(() => document.getElementById("screen").innerText.replace(/\s/g, "").includes("Other_Notes.txt"), null, { timeout: 5000 })
    .then(() => true).catch(() => false);   // the copy's path typed before the line is cleared
  await page.keyboard.press("Control+U");
  check("AC-36 a file not among the drag's files is uploaded, the toast saying so and naming what the drag held",
    copied && earlier.startsWith("Their own paths were not found: Other Notes.txt is not among the files the drag held (it held 2, Drums Initial.zip first), perhaps an earlier drag's.") && !earlier.includes("?."), JSON.stringify({ copied, earlier }));
  // the drag held no file (Mail, a browser): copies uploaded, the toast saying why first
  fs.writeFileSync(drag, "[]");

  // 22 MB (the owner's zip was refused at the old 20 MB): uploaded, its path pasted
  const was = shots(home).length;
  await dropFile(page, "Cat Package.zip", 22 << 20, 1_792_000_000_000);
  for (let i = 0; i < 100 && shots(home).length === was; i++) await page.waitForTimeout(100);
  const saved = shots(home).find((f) => f.endsWith("Cat_Package.zip") || f.endsWith("Cat-Package.zip") || /Cat.Package\.zip$/.test(f));
  const size = saved ? fs.statSync(path.join(home, "tmp", saved)).size : 0;
  await page.waitForTimeout(300);
  const typed = await page.evaluate(() => document.getElementById("screen").innerText.replace(/\s/g, "").includes("Package.zip"));   // a long path wraps
  await page.keyboard.press("Control+U");
  const why = await page.evaluate(() => document.getElementById("toast").innerText);
  check("AC-36 a 22 MB file not on the drag pasteboard is uploaded (100 MB at most), its path pasted, the toast saying why",
    size === 22 << 20 && typed && why.startsWith(NOT_OWN), JSON.stringify({ saved, size, typed, why }));
  const before = shots(home).length;
  await dropFile(page, "Big Package.zip", (100 << 20) + 1, 1_792_000_000_000);
  await page.waitForTimeout(800);
  const said = await page.evaluate(() => document.getElementById("toast").innerText);
  check("AC-36 a file over 100 MB is not sent: the page says to give its path instead",
    said === `${NOT_OWN} Big Package.zip is larger than the 100 MB an upload may be: type or paste the path to the file instead (in Finder ⌥⌘C copies it).` && shots(home).length === before, said);
}

export async function livePaste(page, { home, name }) {
  const dir = fs.realpathSync(home);
  const printed = await pasteInto(page, home, PNG, "image/png", "Screenshot.png", "png");
  const file = printed && path.join(dir, "tmp", path.basename(printed));
  const mode = file && fs.existsSync(file) ? (fs.statSync(file).mode & 0o777).toString(8) : null;
  check("AC-36 a pasted screenshot is saved in ~/.terminum/tmp (0600) and its path reaches the shell as one word",
    !!printed && fs.realpathSync(path.dirname(printed)) === path.join(dir, "tmp") && /\.png$/.test(printed) && mode === "600"
      && fs.readFileSync(file).subarray(1, 4).toString() === "PNG", JSON.stringify({ printed, mode }));
  // Chromium decodes no TIFF, nor does WebKit on Linux; Safari on the Mac is where it comes from
  if (name === "webkit" && process.platform === "darwin") {
    const tiff = await pasteInto(page, home, TIFF, "image/tiff", "Image.tiff", "tif");
    const saved = tiff && path.join(dir, "tmp", path.basename(tiff));
    check("AC-36 a TIFF on the clipboard is pasted as a PNG", !!saved && /\.png$/.test(saved) && fs.existsSync(saved)
      && fs.readFileSync(saved).subarray(1, 4).toString() === "PNG", JSON.stringify({ tiff }));
  }
  await within("AC-36 a pasted screenshot is never typed as text", async () => {
    const text = await page.evaluate(() => document.getElementById("screen").innerText);
    if (text.includes("iVBOR") || text.includes("too large")) throw new Error("typed as text");
  });
}
