// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// Files to the shown pane (AC-52): a pasted image, files from "Upload" on the hot keys or Attach
// (a phone's row under the input), files dropped on the page. Each is saved where the pane's shell runs (this Mac or its host), keeping
// a file's name, and its path is pasted as text without Enter: programs such as Claude Code take
// a file by its path. Dropped files go one at a time (the bridge takes two at once) and their
// paths are pasted together, one per line in a coding agent's box; so do picked ones. A file over
// the limit is not sent: the page says to give its path instead (a browser never gives a page a
// file's path). A drop on a pane of this Mac in this Mac's browser puts the files' own paths
// instead, nothing uploaded at any size: terminumd finds them on macOS's drag pasteboard (dragged.rs).
import { NOT_PASTED, shellWord } from "./input.js";

// what terminumd takes as a pasted image; another type the browser can draw goes as a PNG
const TAKEN = ["image/png", "image/jpeg", "image/gif", "image/webp"];
const MAX_BYTES = 100 << 20;            // terminumd's upload limit (page/upload.rs MAX_BYTES)

/** Why a file over the upload limit was not sent. */
export const tooLarge = (name) => `${name} is larger than the 100 MB an upload may be: type or paste the path to the file instead (in Finder ⌥⌘C copies it).`;

/** The image as terminumd takes it: as it is, else drawn into a PNG (TIFF from Preview, HEIC). */
export async function asTaken(blob) {
  if (TAKEN.includes(blob.type)) return blob;
  const bitmap = await createImageBitmap(blob).catch(() => null);
  if (!bitmap) return null;
  const canvas = document.createElement("canvas");
  canvas.width = bitmap.width;
  canvas.height = bitmap.height;
  canvas.getContext("2d").drawImage(bitmap, 0, 0);
  bitmap.close();
  return new Promise((resolve) => canvas.toBlob(resolve, "image/png"));
}

// Rows dragged from the Files panel (ui/src/tree-drag.ts): {host, paths}, put as paths, nothing uploaded.
const PATHS = "application/x-terminum-paths";

/** The paths a drag from Files carries, or null: absolute, no control characters (each would be a key). */
export function draggedPaths(raw) {
  let m;
  try { m = JSON.parse(raw); } catch { return null; }
  const paths = Array.isArray(m?.paths) ? m.paths.filter((p) => typeof p === "string" && p.startsWith("/") && !/[\x00-\x1f\x7f]/.test(p)) : [];
  return paths.length ? { host: typeof m.host === "string" ? m.host : null, paths } : null;
}

/** Paths as pasted: one per line (`lines`), else separated by spaces. */
export const pathList = (paths, lines) => paths.map(shellWord).join(lines ? "\n" : " ");

/** sid(): the shown pane; host(): its host (null: this Mac); paste(text) at its input; lines():
 *  paths one per line: a coding agent's box with a bracketed paste (in a shell the lines would
 *  run as one command each at Enter). */
/** atMac: this is the Mac's own browser, as terminumd says (its build message's `mac`): /api/os
 *  answers it alone. */
export function uploads({ sid, host = () => null, atMac = () => false, paste, lines = () => false, toast, hideToast }) {
  /** Saves one file: {path} or {error}. */
  async function save(blob, what, url, from) {
    const r = await fetch(url + encodeURIComponent(from), { method: "POST", headers: { "Content-Type": blob.type || "application/octet-stream" }, body: blob })
      .catch(() => null);
    const text = r ? await r.text().catch(() => "") : "";
    let m = {};
    try { m = JSON.parse(text); } catch { m = { error: text.trim() || undefined }; }   // the server's own refusals are plain text
    if (r?.status === 413) return { error: tooLarge(blob.name ?? `The ${what}`) };   // never the server's own words
    if (!r?.ok || !m.path) return { error: m.error ?? (r ? `The ${what} was not uploaded (${r.status}).` : "The Mac does not answer.") };
    return { path: m.path };
  }

  // The paths saved are pasted; a failure stops the rest and stays on screen. `why`: why the
  // files' own paths were not taken, said first (never a silent fallback).
  async function send(list, what, why = "", from = sid()) {
    const said = why ? `${why} ` : "";
    if (!from) return toast("Open a session first.");
    if (sid() !== from) return toast(NOT_PASTED, 6000);
    const paths = [];
    let error = null;
    for (const [i, [blob, url]] of list.entries()) {
      if (blob.size > MAX_BYTES) { error = tooLarge(blob.name ?? `The ${what}`); break; }   // not sent at all
      toast(said + (list.length > 1 ? `Uploading ${i + 1} of ${list.length}…` : `Uploading the ${what}…`), 60000);
      ({ path: paths[i], error } = await save(blob, what, url, from));
      if (error) { paths.pop(); break; }
    }
    if (paths.length && sid() !== from) return toast(`The ${what} was saved, but another session is shown now: nothing was pasted.`, 6000);
    if (paths.length) paste(pathList(paths, lines()));
    if (!error) return why ? toast(`${said}Copies were uploaded and their paths pasted.`, 6000) : hideToast();
    toast(said + (list.length > 1 ? `${paths.length} of ${list.length} uploaded (their paths pasted); ${list[paths.length][0].name}: ${error} The rest were not sent.` : error), 8000);
  }

  function putPaths(got) {
    if (!got) return;
    if (!sid()) return toast("Open a session first.");
    const on = host() ?? null;
    if (got.host !== on) return toast(`These files are on ${got.host ?? "this Mac"}; the session runs on ${on ?? "this Mac"}: nothing was put.`, 6000);
    paste(got.paths.map(shellWord).join(" "));
  }

  /** The dropped files' own paths ({file, dir} each) from terminumd: {paths} in their order, or {why}. */
  async function ownPaths(from, got) {
    // names composed (NFC), as terminumd has the disk's names (dragged.rs)
    const files = got.map(({ file, dir }) => ({ name: file.name.normalize("NFC"), size: dir ? 0 : file.size, mtime: file.lastModified, dir }));
    const r = await fetch("/api/os/dropped", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ sid: from, files }) })
      .catch(() => null);
    const m = r ? await r.json().catch(() => ({})) : {};
    if (r?.ok && Array.isArray(m.paths) && m.paths.length === files.length) return { paths: m.paths };
    return { why: m.message ?? (r ? `terminumd answered ${r.status}` : "the Mac does not answer") };
  }

  // Dropped files: on a pane of this Mac in its browser their own paths, else copies uploaded (AC-36).
  async function dropped(got) {
    let why = "";
    if (atMac() && !host()) {
      const from = sid();
      if (!from) return toast("Open a session first.");
      toast("Finding the files…", 60000);
      const own = await ownPaths(from, got);
      if (sid() !== from) return toast(NOT_PASTED, 6000);
      if (own.paths) { hideToast(); return paste(pathList(own.paths, lines())); }
      why = `Their own paths were not found: ${own.why.replace(/\.$/, "")}.`;
    }
    if (got.some((g) => g.dir)) return toast(`${why} A folder cannot be uploaded: drop its files.`.trim(), 6000);
    return send(got.map(({ file }) => [file, fileUrl(file)]), "file", why);
  }

  const fileUrl = (f) => `/upload?name=${encodeURIComponent(f.name)}&sid=`;
  return {
    async pasteImage(blob, from = sid()) {
      const image = await asTaken(blob);
      if (!image) return toast(`This browser cannot read a ${blob.type.replace("image/", "").toUpperCase()} image: nothing was pasted.`, 6000);
      return send([[image, "/paste?sid="]], "image", "", from);
    },
    uploadFiles: (list) => send(list.map((f) => [f, fileUrl(f)]), "file"),
    /** Files dropped anywhere on the page (a drop the page did not take would open the file in
     *  the browser and leave the page, with any unsaved edits in View); rows dragged from Files
     *  put their paths (AC-36). */
    bindDrop(target) {
      const kind = (e) => { const t = [...(e.dataTransfer?.types ?? [])]; return t.includes(PATHS) ? "paths" : t.includes("Files") ? "files" : null; };
      document.addEventListener("dragover", (e) => {
        if (!kind(e)) return;
        e.preventDefault();
        e.dataTransfer.dropEffect = "copy";
        target.classList.add("dropping");
      });
      document.addEventListener("dragleave", (e) => { if (!e.relatedTarget) target.classList.remove("dropping"); });
      document.addEventListener("drop", (e) => {
        const k = kind(e);
        if (!k) return;
        e.preventDefault();
        target.classList.remove("dropping");
        if (k === "paths") return putPaths(draggedPaths(e.dataTransfer.getData(PATHS)));
        // read now: the drop's items are gone once the event is over
        const got = [...e.dataTransfer.items].filter((it) => it.kind === "file")
          .map((it) => { const entry = it.webkitGetAsEntry?.(), dir = !!entry?.isDirectory;   // a folder may come with no File
            return { file: it.getAsFile() ?? (dir ? { name: entry.name, size: 0, lastModified: 0 } : null), dir }; }).filter((g) => g.file);
        if (got.length) void dropped(got);
      });
    },
  };
}
