// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// What the viewer shows of a document, as HTML from its state: the per-file document
// record, the tab strip, the banner above the content and the tool buttons.

import type { EditorState, Text } from "@codemirror/state";
import { basename, esc, fmtSize, PROXIED, type FileView, type Tab } from "./api";
import { fileIcon } from "./icons";
import { isRenderable } from "./docview";

export interface Doc {
  file?: FileView;
  error?: string;
  loading?: Promise<void>;
  state?: EditorState;
  saved?: Text;                    // document as last loaded or saved
  dirty: boolean;
  diskChanged: boolean;            // changed on disk while dirty
  saving?: Promise<boolean>;       // one save per file at a time
  deleted?: boolean;               // gone on disk (content kept; a rename may bring it back)
  scroll: Record<string, number>;  // per view mode
  host: string | null;             // where the file lives: every read and save goes there (AC-37)
}

/** Rendered or source: Markdown and the like render unless the tab asks for its source. */
export function modeOf(tab: Tab) {
  return isRenderable(tab.path) && tab.view !== "source" ? "rendered" : "source";
}

export function tabsHtml(tabs: Tab[], active: number | null, docs: Map<string, Doc>): string {
  return tabs.map((t, i) => {
    const dirty = docs.get(t.path)?.dirty;
    return `<div class="tab${i === active ? " active" : ""}${dirty ? " dirty" : ""}" data-i="${i}" title="${esc(t.path)}">` +
      `${fileIcon(basename(t.path))}<span class="tname">${esc(basename(t.path))}</span>` +
      `<span class="close" title="Close">${dirty ? "●" : "×"}</span></div>`;
  }).join("");
}

/** The line above the content: deleted, changed on disk, or why editing is off. */
export function bannerHtml(f: FileView, d: Doc): string {
  if (d.deleted) return `<div class="banner">Deleted on disk — Save recreates it${f.writable ? "" : " (read-only here)"}</div>`;
  if (d.diskChanged)
    return `<div class="banner">Changed on disk <button class="btn sm" data-disk="reload">Reload</button> <button class="btn sm" data-disk="keep">Keep mine</button></div>`;
  const notes: string[] = [];
  if (f.truncated) notes.push(`${fmtSize(f.size)} — showing first 1 MB, editing disabled`);
  else if (!f.binary && !f.utf8) notes.push("Not UTF-8 — editing disabled to avoid corruption");
  else if (!f.binary && !f.writable) notes.push("Read-only: outside writable folders or no permission");
  return notes.length ? `<div class="banner">${notes.map(esc).join(" · ")}</div>` : "";
}

/** The tool buttons of the active tab. */
export function toolsHtml(tab: Tab, d: Doc, canReveal: boolean): string {
  const f = d.file;
  const md = isRenderable(tab.path) && f && !f.binary;
  const mode = modeOf(tab);
  return (
    (d.dirty ? `<button class="btn sm primary" data-act="save" title="Save (⌘S)">Save</button>` : "") +
    (md ? `<div class="seg"><button data-act="rendered" class="${mode === "rendered" ? "on" : ""}" title="Rendered">Rendered</button>` +
      `<button data-act="source" class="${mode === "source" ? "on" : ""}" title="Source${f?.writable ? " (editable)" : ""}">Source</button></div>` : "") +
    (!canReveal ? "" : `<button class="icon" data-act="reveal" title="Reveal in tree"><svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5"><circle cx="8" cy="8" r="2.5"/><path d="M8 1.5v3M8 11.5v3M1.5 8h3M11.5 8h3"/></svg></button>`) +
    (PROXIED ? "" : `<button class="icon" data-act="open-app" title="Open with default app"><svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M9 2.5h4.5V7M13.5 2.5 7 9M12 10v3.5H2.5V4H6"/></svg></button>`));
}
