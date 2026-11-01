// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// A viewer document against the disk: saving (one save per file at a time, If-Match with the
// held etag, the Overwrite / Reload / Cancel question on a conflict) and the 304 re-check.

import { api, ApiError, basename, toast, type FileView } from "./api";
import { ask } from "./dialogs";
import type { Doc } from "./viewer-parts";

/** What a save asks of the viewer. */
export interface SaveHost {
  revalidate(path: string, d: Doc): Promise<void>;   // re-check the file on disk
  tabsChanged(): void;                               // the dirty marks changed: draw the tabs
  redraw(path: string, d: Doc): void;                // show the document again
  discard(path: string, d: Doc): void;               // drop edits and reload from disk
}

/** Save the file (If-Match etag); on conflict ask Overwrite / Reload / Cancel. */
export function saveDoc(path: string, d: Doc, force: boolean, h: SaveHost): Promise<boolean> {
  // a save asked while one is in flight runs after it, for the text typed meanwhile (AC-56)
  if (d.saving) return d.saving.then((ok) => ok && d.dirty ? saveDoc(path, d, force, h) : ok);
  d.saving = doSave(path, d, force, h).finally(() => { d.saving = undefined; });
  return d.saving;
}

async function doSave(path: string, d: Doc, force: boolean, h: SaveHost): Promise<boolean> {
  if (!d.file || !d.state || !d.file.writable) return false;
  path = d.file.path;                // a rename may have happened during a conflict prompt
  const doc = d.state.doc;           // what we send; typing may continue meanwhile
  const text = d.state.sliceDoc();
  try {
    const r = await api<{ etag: string }>("PUT", "/api/file", {
      query: { path }, body: { text }, host: d.host, headers: { "If-Match": `"${force ? "*" : d.file.etag}"` },
    });
    if (d.file.path !== path) {
      await h.revalidate(d.file.path, d);
      toast("Not saved: file was renamed — save again");
      return false;
    }
    d.file = { ...d.file, etag: r.etag, size: new TextEncoder().encode(text).length, text };
    d.saved = doc;
    d.dirty = !d.state.doc.eq(doc);  // edits typed during the request stay unsaved
    d.diskChanged = false;
    h.tabsChanged();
    h.redraw(path, d);
    toast(`Saved ${basename(path)}`);
    return true;
  } catch (e) {
    if (e instanceof ApiError && e.code === "conflict") {
      const choice = await ask(e.message, "The file was changed by another program since you opened it.", [
        { id: "overwrite", label: "Overwrite", danger: true },
        { id: "reload", label: "Reload (discard mine)" },
        { id: "cancel", label: "Cancel", primary: true },
      ]);
      if (choice === "overwrite") return doSave(path, d, true, h);
      if (choice === "reload") { h.discard(path, d); return false; }
      return false;
    }
    toast(e instanceof ApiError && e.status === 0 ? "Not saved: server restarting — save again" : e instanceof Error ? e.message : String(e));
    return false;
  }
}

/** Check disk state without replacing edits; 304 (same etag) costs no content. `redraw`
 *  applies an update to the shown document and shows it again. */
export async function revalidateDoc(
  path: string, d: Doc, redraw: (path: string, update: () => void, d: Doc) => void,
): Promise<void> {
  if (!d.file) return;
  path = d.file.path;
  try {
    const f = await api<FileView | undefined>("GET", "/api/file", { query: { path }, host: d.host, headers: { "If-None-Match": `"${d.file.etag}"` } });
    if (d.file?.path !== path) return;
    if (!f && !d.deleted) return; // 304: unchanged
    redraw(path, () => {
      d.deleted = false;
      if (f && d.dirty && f.etag !== d.file?.etag) d.diskChanged = true;
      if (f && !d.dirty) { d.file = f; d.state = undefined; d.saved = undefined; }
    }, d);
  } catch (e) {
    if (d.file?.path !== path) return;
    // keep the content: the file may have been renamed (the rename event can arrive first)
    if (e instanceof ApiError && e.status === 404 && e.code !== "no_agent" && !d.deleted) redraw(path, () => { d.deleted = true; }, d);
  }
}
