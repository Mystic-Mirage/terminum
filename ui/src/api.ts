// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// Thin client for terminumd's Files API under /fb/api (terminum-files.md §9). The loopback
// page's sign-in comes from this origin's storage, where the page put it (AC 09); the web
// listener's page holds none: its HttpOnly cookie goes by itself.

import { basename } from "./paths";
import { hello } from "../../web/static/proof.js";
import { shownUrl } from "../../web/static/shown.js";

/** This tab's storage, which survives in-page reloads; private mode may refuse it. */
export const session = {
  get: (k: string) => { try { return sessionStorage.getItem(k); } catch { return null; } },
  set: (k: string, v: string | null) => {
    try { if (v == null) sessionStorage.removeItem(k); else sessionStorage.setItem(k, v); } catch { /* private mode */ }
  },
};

/** The panel lives under /fb/ beside the page; its API is /fb/api/*. */
export const BASE = typeof location === "undefined" ? "" : location.pathname.replace(/\/[^/]*$/, "");
export const PROXIED = BASE !== "";
/** The Mac's own browser (the loopback listener answers only to 127.0.0.1): Finder, default
 *  apps and typing into the terminal are offered there only (terminumd refuses them elsewhere). */
export const AT_MAC = typeof location !== "undefined" && location.hostname === "127.0.0.1";
/** terminumd runs where Finder and default apps are (macOS), as its proof said; a Linux server's
 *  page offers neither (it would refuse them). */
export let SERVER_APPS = true;

/** The loopback sign-in the page stored (null: the web listener's cookie signs in). */
const SECRET: string | null = (() => { try { return localStorage.getItem("tm.session"); } catch { return null; } })();
/** terminumd proved, since the last time it was out of reach, that it knows the sign-in:
 *  requests then carry a credential for its boot (signin.rs), never the secret. While terminumd
 *  is not running another program may hold its port; it cannot answer the proof, and a
 *  credential it gets is for a boot no terminumd will have again (AC 09). */
let proven: Promise<string | null> | null = null;
let token: string | null = null;
/** The sign-in header of credential `cred` (proveServer's answer). */
const signin = (cred: string | null): Record<string, string> => (cred ? { "X-TM-Session": cred } : {});
let refusal = "";  // why the last proof failed, as the user is told
/** The credential of the current proof (null: no sign-in here). */
export function proveServer(): Promise<string | null> {
  if (!SECRET) return Promise.resolve(null);
  if (!proven) {
    const mine: Promise<string | null> = hello(SECRET, fetch).then((r) => {
      if (r.state === "unknown") throw new ApiError(401, "signed_out", "Signed out: run terminum open", null);
      if (r.state === "down") throw new TypeError("terminumd not reachable");   // a network error: asked again
      if (r.state !== "ok") throw new ApiError(401, "unproven", "terminumd does not prove it knows this page's sign-in, or another program holds its port", null);
      if (proven === mine) token = r.cred ?? null;
      SERVER_APPS = r.apps !== false;
      return r.cred ?? null;
    });
    mine.catch((e) => { if (e instanceof ApiError) refusal = e.message; if (proven === mine) forgetProof(); });
    proven = mine;
  }
  return proven;
}
/** terminumd went out of reach: prove it again, and make a new credential, before a request goes out. */
export function forgetProof() { proven = null; token = null; }
/** Why the last proof failed, as the user is told. */
export const proofRefusal = () => refusal;
/** Names this panel in workspace events, so it can ignore the echo of its own writes. */
export const CLIENT = Math.random().toString(36).slice(2, 10);

export interface Row { n: string; k: "d" | "f" | "l" | "L"; s?: number }
export interface Page {
  status: "ready" | "loading" | "error"; total: number; writable: boolean;
  gen: number; entries: Row[]; error?: string; located?: number;
}
/** A changed file and its etag now (null: gone), as sent in fs-change events. */
export interface Stamp { path: string; etag: string | null }
export interface FsChange { dirs: string[]; files: Stamp[]; moved: { from: string; to: string }[]; host?: string }
export interface Tab { path: string; view: "auto" | "rendered" | "source" }
export interface Pane {
  rev: number; root: string; expanded: string[]; selected: string[]; scroll: number;
  tabs: Tab[]; active_tab: number | null; updated: number;
}
export interface Theme {
  bg?: string; fg?: string; sel?: string; selfg?: string; cursor?: string; link?: string;
  ansi?: string[]; font?: string | null; size?: number | null; dark?: boolean;
}
export interface TermState {
  version: number; bridge: boolean; key?: string; session?: string; title?: string;
  mode?: string; note?: string; job?: string; busy?: boolean; cwd?: string | null; stale?: boolean; theme?: Theme;
  build?: string;   // terminumd's build: another one than this page's means an upgrade (AC-16)
  host?: string;    // a remote pane's host: its files are reached through that host's agent (AC 82)
  /** a remote pane's host as offered to the user (AC 83): key = its name */
  remote?: {
    key: string; name: string; state: "ask" | "dismissed" | "enabling" | "connecting" | "updating" | "up" | "down";
    updated?: string;  // the build its helper was just updated to (AC-84)
    why?: string;      // why the enabling ticked in Add host failed (AC 83)
  };
}
export interface FileView {
  path: string; size: number; etag: string; binary: boolean; mime: string | null;
  truncated: boolean; utf8: boolean; writable: boolean; text?: string;
}
/** The tree's order (AC-52), folders first in each: terminumd sorts, the tree pages it. */
export type Sort = "name" | "recent" | "ext";
export const SORTS: { id: Sort; label: string }[] = [
  { id: "recent", label: "Recent First" }, { id: "name", label: "Alphabetical" }, { id: "ext", label: "By Extension" }];
export interface Prefs { hidden: boolean; split: number; sort: Sort }
export const DEFAULT_PREFS: Prefs = { hidden: true, split: 0.5, sort: "name" };

export class ApiError extends Error {
  constructor(public status: number, public code: string, message: string, public body: any) { super(message); }
}

type Query = Record<string, string | number | boolean | undefined>;

/** `host`: the machine the request is for (null: this Mac); default the shown pane's. An
 *  action that waited for the user (a dialog) passes the host it started on (AC-37). */
export async function api<T>(method: string, path: string, opts: { query?: Query; body?: unknown; headers?: Record<string, string>; retry?: boolean; host?: string | null } = {}): Promise<T> {
  const qs = new URLSearchParams();
  for (const [k, v] of Object.entries(opts.query ?? {})) if (v !== undefined) qs.set(k, String(v));
  const url = BASE + path + (qs.size ? `?${qs}` : "");
  const host = opts.host === undefined ? scope : opts.host;
  const headers: Record<string, string> = { "X-TM-Client": CLIENT, ...(host ? { "X-TM-Host": host } : {}), ...opts.headers };
  if (opts.body !== undefined) headers["Content-Type"] = "application/json";
  let res: Response;
  for (let waited = 0, step = 250, renewed = false; ; waited += step, step *= 2) {
    try {
      const mine = proveServer(), cred = await mine;
      res = await fetch(url, { method, headers: { ...headers, ...signin(cred) }, body: opts.body === undefined ? undefined : JSON.stringify(opts.body) });
      // a terminumd of another boot (an upgrade, a restart) refused it before any handler ran: proved
      // again (unless another request did meanwhile), sent once more
      if (res.status === 401 && cred && !renewed && (await res.clone().json().catch(() => null))?.error === "bad_session") {
        renewed = true;
        if (proven === mine) forgetProof();
        continue;
      }
      break;
    } catch (e) {
      if (e instanceof ApiError) throw e;  // not proven: the secret is never sent, nor retried
      forgetProof();
      // reads ride out a backend restart (AC-34); writes fail at once, never repeated behind the user
      if (method !== "GET" || opts.retry === false || waited >= 3000) throw new ApiError(0, "network", "terminumd not reachable (restarting?)", null);
      await new Promise((r) => setTimeout(r, step));
    }
  }
  if (res.status === 204 || res.status === 304) return undefined as T;
  const body = await res.json().catch(() => null);
  if (!res.ok) throw new ApiError(res.status, body?.error ?? "http", body?.message ?? `HTTP ${res.status}`, body);
  return body as T;
}

/** The host whose files the panel shows (a remote pane, AC-37), or null for this Mac. Every
 *  file request names it; terminumd sends such a request to that host's agent and nowhere else. */
export let scope: string | null = null;
export const setScope = (host: string | null) => { scope = host || null; };

/** A file's raw address (no sign-in in it: the page checks it before it fetches, fb-zoom). */
export const rawUrl = (path: string) => `${BASE}/api/raw?path=${encodeURIComponent(path)}${scope ? `&host=${encodeURIComponent(scope)}` : ""}`;

/** A file's bytes behind a blob: (an SVG's a data:) address, fetched with the sign-in in its
 *  header: an <img>, a <video> or a download sends none, and an address with it would be copied
 *  away (Copy Image Address) and keep the 30-day secret (AC-09). Only a picture, a video or a
 *  sound keeps its type (shown.js: opened as a page, anything else would run in this origin).
 *  The caller revokes it. */
export async function rawBlob(path: string): Promise<string> {
  return shownUrl(await (await raw(path)).blob());
}

/** Download in the web app (AC-53): fetched with the sign-in, saved from a blob: of bytes. */
export async function downloadRaw(path: string) {
  try {
    const url = URL.createObjectURL(new Blob([await (await raw(path)).blob()], { type: "application/octet-stream" }));
    Object.assign(document.createElement("a"), { href: url, download: basename(path) }).click();
    setTimeout(() => URL.revokeObjectURL(url), 60_000);
  } catch (e: any) { toast(`${basename(path)}: ${e.message}`); }
}

/** A file's bytes, fetched with the sign-in (a sound decoded here, a MIDI file read here). */
export const rawBytes = async (path: string) => (await raw(path)).arrayBuffer();

/** A MIDI file's bytes rendered by terminumd with the user's SoundFont (AC-64): a WAV's blob:
 *  address (the caller revokes it) and the bank's name; an error as terminumd says it. */
export async function midiWav(bytes: ArrayBuffer, signal?: AbortSignal): Promise<{ url: string; font: string }> {
  const cred = await proveServer();
  const res = await fetch("/api/midi", { method: "POST", body: bytes, signal, headers: { "Content-Type": "audio/midi", ...signin(cred) } });
  if (!res.ok) { const body = await res.json().catch(() => null); throw new ApiError(res.status, body?.error ?? "http", body?.message ?? `HTTP ${res.status}`, body); }
  return { url: URL.createObjectURL(new Blob([await res.arrayBuffer()], { type: "audio/wav" })), font: res.headers.get("x-tm-soundfont") ?? "" };
}

/** The most a file dropped on the tree may be (terminumd's page::upload::MAX_BYTES). */
export const UPLOAD_MAX = 100 << 20;

/** A file dropped on the tree saved as a new file `name` in `dir` (AC-95) by `host` (the tree's);
 *  `both`: under the next free number when the name is taken (else refused: "exists"). */
export async function uploadFile(dir: string, name: string, file: Blob, both: boolean, host: string | null): Promise<{ path: string }> {
  const qs = new URLSearchParams({ dir, name, ...(both ? { both: "true" } : {}) });
  const cred = await proveServer();
  const headers = { "Content-Type": file.type || "application/octet-stream", "X-TM-Client": CLIENT, ...signin(cred), ...(host ? { "X-TM-Host": host } : {}) };
  const res = await fetch(`${BASE}/api/fs/upload?${qs}`, { method: "POST", body: file, headers }).catch(() => null);
  if (!res) throw new ApiError(0, "network", "terminumd not reachable (restarting?)", null);
  if (res.status === 413) throw new ApiError(413, "too_large", "Larger than the 100 MB an upload may be", null);
  const body = await res.json().catch(() => null);
  if (!res.ok) throw new ApiError(res.status, body?.error ?? "http", body?.message ?? `HTTP ${res.status}`, body);
  return body;
}

/** A file's raw answer, fetched with the sign-in; an error as terminumd says it. */
async function raw(path: string): Promise<Response> {
  const url = rawUrl(path);            // the host shown now, not after the proof (a pane switched meanwhile)
  const cred = await proveServer();
  const res = await fetch(url, { headers: signin(cred) });
  if (res.ok) return res;
  const body = await res.json().catch(() => null);
  throw new ApiError(res.status, body?.error ?? "http", body?.message ?? `HTTP ${res.status}`, body);
}

// A picture's, a video's, a sound's or a PDF's address, kept while it is shown: revoked once its
// element left the page or its document's frame closed (looked at on each change of the page and
// every SWEEP ms while there is one); a player is stopped then: a detached one plays on.
const playing = new Map<string, [HTMLElement, () => void]>();
const SWEEP = 5000;
let sweeping = 0, watching: MutationObserver | null = null;
function sweep() {
  for (const [url, [el, gone]] of playing) {
    if (el.isConnected && el.ownerDocument.defaultView) continue;
    if (el.tagName === "VIDEO" || el.tagName === "AUDIO") {   // tagName: a frame's elements are of its own classes
      const media = el as HTMLMediaElement;
      media.pause();
      media.removeAttribute("src");
      media.load();                                // its decoder let go
    }
    gone();
    playing.delete(url);
  }
  if (!playing.size) { clearInterval(sweeping); sweeping = 0; watching?.disconnect(); watching = null; }
}
/** `gone()` once `el` left the page (its address revoked, its player stopped); `id` names it. */
export function whileShown(id: string, el: HTMLElement, gone: () => void) {
  playing.set(id, [el, gone]);
  sweeping ||= window.setInterval(sweep, SWEEP);
  if (!watching) { watching = new MutationObserver(sweep); watching.observe(document, { childList: true, subtree: true }); }
}

/** Fills the elements under `root` that name a file in `data-raw` (rendered Markdown, an HTML
 *  document, an image tab): `src` from rawBlob, a stylesheet's text into a <style>. Each address
 *  is revoked once its element is gone (sweep): a picture shown may still be opened by it. */
export function hydrateRaw(root: ParentNode) {
  for (const el of root.querySelectorAll<HTMLElement>("[data-raw]")) {
    const path = el.dataset.raw!;
    delete el.dataset.raw;
    if (el.tagName === "LINK") {
      void raw(path).then((r) => r.text()).then((css) => {
        const style = el.ownerDocument.createElement("style");
        style.textContent = css;
        el.replaceWith(style);
      }, () => el.remove());
      continue;
    }
    void rawBlob(path).then((url) => {
      whileShown(url, el, () => URL.revokeObjectURL(url));
      el.setAttribute("src", url);
    }, (e: Error) => { el.title = `${path}: ${e.message}`; });
  }
}
export const socketUrl = () => `${location.protocol === "https:" ? "wss" : "ws"}://${location.host}${BASE}/api/ws`;
/** The event socket's subprotocols carry the sign-in: ["tm", credential] (call after proveServer). */
export const socketProtocols = () => (token ? ["tm", token] : undefined);

/** Run an API call; on failure show its message and resolve to undefined. */
export const apiOrToast = <T>(...args: Parameters<typeof api<T>>) => api<T>(...args).catch((e: Error) => { toast(e.message); return undefined; });

export const isDirKind = (k: Row["k"]) => k === "d" || k === "L";
export * from "./paths";
export function fmtSize(n?: number | null): string {
  if (n == null) return "";
  if (n < 1024) return `${n} B`;
  if (n < 1048576) return `${(n / 1024).toFixed(n < 10240 ? 1 : 0)} K`;
  if (n < 1073741824) return `${(n / 1048576).toFixed(1)} M`;
  return `${(n / 1073741824).toFixed(1)} G`;
}

export const esc = (s: string) =>
  s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);

let toastTimer = 0;
/** Copy text, with the old copy command where the clipboard API is refused; then say `done`. */
export async function copyText(text: string, done = `Copied ${text}`) {
  try { await navigator.clipboard.writeText(text); }
  catch {
    const ta = Object.assign(document.createElement("textarea"), { value: text });
    document.body.append(ta); ta.select(); document.execCommand("copy"); ta.remove();
  }
  toast(done);
}

export function toast(msg: string) {
  const el = document.getElementById("toast")!;
  el.textContent = msg;
  el.classList.add("on");
  clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => el.classList.remove("on"), 3500);
}
