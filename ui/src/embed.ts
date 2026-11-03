// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The Files panel and the file viewer inside terminum's page (AS-51, AC-70): the panel is
// pinned to the pane the page shows and keeps a workspace of its own; files open in the
// page's View, which receives them as messages.

import { PROXIED, type Theme } from "./api";

const params = new URLSearchParams(location.search);

/** The pane the panel shows, as the page names it in the frame's URL (null: none named). */
export const PIN = PROXIED ? params.get("key") : null;
/** That pane's folder now, and the host its files are on (null: this Mac). */
export const PIN_CWD = params.get("cwd");
export const PIN_HOST = params.get("host");
/** The panel's workspace of that pane: `web:` + JSON `[host, key]`, the format terminumd reads
 *  it in (watch_web.rs, panel_api.rs). */
export const pinKey = (key: string, host: string | null) => `web:${JSON.stringify([host ?? "", key])}`;

const around = PROXIED && window.parent !== window ? window.parent : null;
// Files dropped here are not taken (the page uploads files dropped on its terminal), but the
// browser must not open them either: it would leave the page, and unsaved edits with it.
if (around) for (const type of ["dragover", "drop"]) {
  addEventListener(type, (e) => { if ((e as DragEvent).dataTransfer?.types.includes("Files")) e.preventDefault(); });
}

function tell(message: { type: string; path: string; host?: string | null; window?: boolean; caption?: string }): boolean {
  if (!around) return false;
  around.postMessage(message, location.origin);
  return true;
}

/** A file opens in the page's View (`window`: the user asked for a window of its own, ⌘-click);
 *  false when there is no page around. */
export const openOutside = (path: string, host: string | null, window = false) =>
  tell({ type: "fb-open", path, host, ...(window ? { window } : {}) });
/** "Reveal in tree" in the File view shows the file in the page's Files view. */
export const revealOutside = (path: string) => tell({ type: "fb-reveal", path });
/** An image tab's picture full screen in the page's viewer (zoom, pan; `path` is its URL here). */
export const zoomOutside = (src: string, caption: string) => tell({ type: "fb-zoom", path: src, caption });
/** The last file of the File view was closed: the page goes back to the terminal. */
export const emptiedOutside = () => tell({ type: "fb-empty", path: "" });
/** Esc in View: the page hides it and shows the terminal (AC-70); false without a page around. */
export const escapedOutside = () => tell({ type: "fb-esc", path: "" });

/** Take what the page around sends (files to open or reveal, its theme, Files' folder, View parked or shown), and nothing from anywhere else. */
export function acceptFromPage(handlers: { open: (path: string) => void; reveal: (path: string) => void; theme: (t: Theme) => void; root: (path: string, host: string | null) => void; park: (on: boolean) => void }) {
  if (!around) return;
  addEventListener("message", (e) => {
    if (e.source !== around || e.origin !== location.origin) return;
    const d = e.data;
    if (d?.type === "fb-theme" && d.theme && typeof d.theme === "object") return handlers.theme(d.theme);
    if (d?.type === "fb-park" && typeof d.park === "boolean") return handlers.park(d.park);
    if (typeof d?.path !== "string") return;
    if (d.type === "fb-open") handlers.open(d.path);
    if (d.type === "fb-reveal") handlers.reveal(d.path);
    if (d.type === "fb-root") handlers.root(d.path, typeof d.host === "string" ? d.host : null);
  });
}
