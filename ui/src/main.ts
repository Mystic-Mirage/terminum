// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// Files panel: pinned to the pane the page names in its URL (key, cwd, host; AS-51), shows
// its folder as a virtual tree, opens files in tabs, edits and saves them, and keeps a
// workspace per pane in terminumd. `?view=<path>&host=` is the page's View: tabs only.

import "./style.css";
import {
  api, apiOrToast, ApiError, basename, CLIENT, copyText, DEFAULT_PREFS, dirname, isUnder, proofRefusal, PROXIED, scope, setScope, SORTS, toast,
  type FsChange, type Pane, type Prefs, type TermState,
} from "./api";
import { renderOffer } from "./host-offer";
import { contextMenu } from "./context-menu";
import { acceptFromPage, emptiedOutside, escapedOutside, openOutside, PIN, PIN_CWD, PIN_HOST, pinKey, revealOutside } from "./embed";
import { applyTheme } from "./theme";
import { redrawDiagrams } from "./mermaid";
import { Tree, type EditMode } from "./tree";
import { bindDrag, bindHold } from "./tree-drag";
import { bindDrop } from "./tree-drop";
import { Viewer } from "./viewer";
import { PathBar } from "./viewer-path";
import { Stream } from "./stream";
import { Upgrade } from "./upgrade";
import { commitPath, trashPaths } from "./file-actions";
import { refreshScope } from "./panel-refresh";

const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;

/** A viewer (the page's View) shows only tabs: `?view=<path>&host=`. */
const params = new URLSearchParams(location.search);
const VIEW = params.get("view");
/** The pane shown: its mode badge and, for a host's pane, the host as the title. */
let term: TermState = { version: -1, bridge: false, mode: PIN_HOST ? "remote" : "tmux", title: PIN_HOST ?? "" };
let updateNote = "";                  // an upgrade waits for unsaved work (AC-16)
let key: string | null = null;        // the panel's workspace key of the pane shown: web:[host, pane key]
let ws: Pane | null = null;           // its workspace as last read or written
let workspaceRun = 0;                // orders reads and accepted write replies, even after a revision reset
let prefs: Prefs = { ...DEFAULT_PREFS };
let saveTimer = 0;
let pinCwd = PIN_CWD;                 // the pane's folder, sent with the first workspace read
let filterTimer = 0;

/** Say what is wrong instead of an endless "connecting…". */
function notice(title: string, text: string) {
  const el = $("notice");
  el.classList.toggle("on", !!title);
  el.innerHTML = "";
  if (title) el.append(Object.assign(document.createElement("b"), { textContent: title }), text);
  $("title").textContent = title || term.title || "";
}

/** The sign-in was refused, or terminumd did not prove it knows it. `retrying`: the stream
 *  keeps asking for a proof (no secret goes out), so the panel comes back by itself if
 *  terminumd does. */
function authFailed(why: string, retrying = false) {
  if (!retrying) stream.close();
  $("dot").className = "dot off";
  notice("Not signed in", `${why}. Reload the page once terminum is signed in again (run terminum open on the Mac).`);
}

// ── workspace sync ───────────────────────────────────────────────────────────

function snapshot(): Pane | null {
  if (!ws) return null;
  return {
    ...ws,
    root: tree.root?.path ?? ws.root,
    expanded: [...tree.expanded],
    selected: [...tree.selected],
    scroll: Math.round(tree.scroll),
    tabs: viewer.tabs.map((t) => ({ ...t })),
    active_tab: viewer.active,
  };
}

/** A pane's state without its revision and time, its keys sorted: terminumd answers with sorted
 *  keys, a snapshot has them in the order it built them. */
const content = (p: Pane) => JSON.stringify({ ...p, rev: 0, updated: 0 }, (_k, v: unknown) =>
  v && typeof v === "object" && !Array.isArray(v) ? Object.fromEntries(Object.entries(v).sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0))) : v);

function scheduleSave() {
  clearTimeout(saveTimer);
  saveTimer = window.setTimeout(() => void saveNow(), 300);
}

/** The pane and content of a PUT that failed without an answer (a network error): it may have landed. */
let ambiguous: { key: string; content: string } | null = null;

/** A failed save says so once until that kind of save works again: saves follow every scroll and
 *  expand, and a stopped terminumd would stack a toast for each over its banner. */
const warned = new Set<string>();
function failed(kind: string, text: string) {
  if (!warned.has(kind)) toast(text);
  warned.add(kind);
}

async function saveNow() {
  clearTimeout(saveTimer);
  const snap = snapshot(), k = key;
  if (!snap || !k || content(snap) === content(ws!)) return;
  const previous = ws, run = workspaceRun;
  const current = () => key === k && ws === previous && run === workspaceRun;
  try {
    const r = await api<{ rev: number }>("PUT", "/api/workspace", { query: { key: k }, body: snap });
    ambiguous = null;
    warned.delete("workspace");
    if (current()) { workspaceRun++; ws = { ...snap, rev: r.rev }; }
  } catch (e) {
    if (e instanceof ApiError && e.code === "stale_rev" && current()) {
      workspaceRun++;
      const theirs = e.body.current as Pane;
      // our own earlier write landed although its answer was lost: keep what changed since, on its rev
      if (ambiguous?.key === k && content(theirs) === ambiguous.content) { ambiguous = null; ws = theirs; scheduleSave(); return; }
      ambiguous = null;
      // another page showing this pane, or a cd, won the race: take the server's version
      await applyWorkspace(theirs);
      return;
    }
    // fail loud; the snapshot stays unsaved (ws unchanged): the next change or reconnect retries
    if (!(e instanceof ApiError) || e.code === "network") ambiguous = { key: k, content: content(snap) };   // api() wraps a network error
    failed("workspace", `Panel state not saved: ${(e as Error).message}`);
  }
}

async function loadWorkspace(k: string) {
  const run = ++workspaceRun;
  // a pinned panel names its pane's folder once: a later reconnect must not undo a newer cd
  const cwd = pinCwd ?? undefined;
  pinCwd = null;
  const pane = await api<Pane>("GET", "/api/workspace", { query: { key: k, cwd } });
  if (key === k && run === workspaceRun) await applyWorkspace(pane);
}

async function applyWorkspace(pane: Pane) {
  const paneKey = key;
  const sameTree = ws?.root === pane.root && tree.root?.path === pane.root &&
    JSON.stringify([...tree.expanded].sort()) === JSON.stringify([...pane.expanded].sort());
  ws = pane; // restoring it below is then a no-op for saveNow
  if (sameTree) { tree.selected = new Set(pane.selected); tree.queue(); }
  else await tree.setRoot(pane.root, pane.expanded, pane.selected, pane.scroll);
  if (key !== paneKey || ws !== pane) return;
  viewer.refreshTools();
  if (JSON.stringify(viewer.tabs) !== JSON.stringify(pane.tabs) || viewer.active !== pane.active_tab) viewer.setTabs(pane.tabs, pane.active_tab);
  renderHeader(term);
}

// ── terminal state ───────────────────────────────────────────────────────────

function renderHeader(s: TermState) {
  $("dot").className = "dot" + (!s.bridge ? " off" : s.stale ? " stale" : "");
  $("dot").title = !s.bridge ? "terminumd not connected" : s.stale ? "folder unavailable" : "following the shown pane";
  $("mode").textContent = s.mode ?? "—";
  $("mode").className = "badge" + (s.mode === "remote" ? " warn" : "");
  $("title").textContent = s.remote?.name ?? s.title ?? "";
  const cwd = ws?.root ?? s.cwd ?? "";
  const home = scope ? undefined : cwd.match(/^\/Users\/[^/]+/)?.[0];
  const head = (scope ? `${s.remote?.name ?? scope}:` : "") + (cwd === "/" ? "" : dirname(cwd).replace(/\/?$/, "/"));
  const b = Object.assign(document.createElement("b"), { textContent: basename(cwd) });
  const bdi = document.createElement("bdi");
  bdi.append(home ? head.replace(home, "~") : head, b);
  $("crumbs").replaceChildren(bdi);
  $("crumbs").title = cwd;
  $("note").textContent = [updateNote, s.stale && "⏸ cwd unavailable — showing last known"].filter(Boolean).join(" · ");
  $("foot").textContent = s.note ?? "";
}

/** terminumd's state (its build, AC-16) from the socket's first frame. */
function onState(s: TermState) {
  upgrade.build(s.build);
  term = { ...term, bridge: true };
  if (!VIEW) renderHeader(term);
  // terminumd is back: send what a failed save left behind
  void saveNow();
  if (prefsUnsaved) savePrefs();
}

/** A host's pane: its files offer (AC 83), read when the panel connects and whenever terminumd
 *  says it changed (`host`); a host that came up (enabled, reconnected) is listed again. */
async function refreshHost() {
  if (!PIN_HOST || VIEW) return;
  const s = await api<TermState>("GET", "/api/state", { query: { host: PIN_HOST }, host: null }).catch(() => null);
  const was = term.remote?.state;
  if (s?.remote) {
    term = { ...term, remote: s.remote };
    renderOffer($("offer"), s.remote);
    renderHeader(term);
    if (was && was !== "up" && s.remote.state === "up") refreshScope(scope, scope, tree, viewer);
  }
}

function onFsChange(c: FsChange) {
  if ((c.host ?? null) !== scope) return; // another host's files (AC-37)
  for (const { from, to } of c.moved) { tree.renamed(from, to); viewer.renamed(from, to); }
  void tree.refreshDirs(c.dirs);
  viewer.diskChanged(c.files);
}

let lastUnproven = false;
// a View waiting hidden for its session (the page's fb-park) listens to nothing until shown again
let parked = false, resumed = false;
function connectStream() { if (!parked) void stream.connect(); }
function park(on: boolean) {
  if (on === parked) return;
  parked = on;
  if (on) stream.close();
  else { resumed = true; connectStream(); }
}
/** The event stream's handlers; it opens after terminumd proved itself (stream.ts, AC 09). */
const stream = new Stream({
  on: {
    state: (s) => onState(s),
    // our own writes echo back with by === CLIENT
    workspace: ({ key: k, rev, by }) => { if (k === key && by !== CLIENT && (!ws || rev > ws.rev)) void loadWorkspace(k); },
    "fs-change": onFsChange,
    rescan: ({ host }) => refreshScope(host ?? null, scope, tree, viewer),
    host: ({ host }) => { if (host === PIN_HOST) void refreshHost(); },
  },
  open() {
    notice("", "");
    if (upgrade.back() || resumed) { // terminumd was away, or this View waited parked: what changed meanwhile was not announced
      resumed = false;
      refreshScope(scope, scope, tree, viewer);
    }
    if (VIEW) return;
    void refreshHost();
    if (!PIN) return void ($("empty").textContent = "No pane named: open Files from terminum's page");
    if (!key) { key = pinKey(PIN, PIN_HOST); setScope(PIN_HOST); viewer.setHost(scope); }
    void loadWorkspace(key);
  },
  lost(unproven) {  // say so after the grace period (the latest reason); the stream tries again by itself
    lastUnproven = unproven;
    $("dot").className = "dot wait"; // a restart or an upgrade takes a moment (AC-16)
    upgrade.lost(() => {
      $("dot").className = "dot off";
      if (lastUnproven) authFailed(proofRefusal(), true);
      else notice("terminumd not running", "terminumd is not reachable; retrying. terminum status says why.");
    });
  },
});

// ── file operations ──────────────────────────────────────────────────────────

function commit(mode: EditMode, dir: string, name: string, path?: string): Promise<string | null> {
  const host = scope, paneKey = key;
  return commitPath(mode, dir, name, path, {
    host, current: () => scope === host && key === paneKey, tree, viewer, open: openFile, changed: scheduleSave,
  });
}

/** The pane changed hosts while the user was deciding: the decision was about other files. */
function switchedSince(host: string | null) {
  if (scope === host) return false;
  toast("The panel switched to another pane — nothing changed");
  return true;
}

async function trashSelected() {
  const host = scope, paneKey = key;
  const paths = [...tree.selected];
  if (!paths.length) return;
  if (paths.some((p) => !tree.isWritable(dirname(p)))) return toast("Read-only: outside writable folders");
  const current = () => !switchedSince(host) && key === paneKey;
  const removed = await trashPaths(paths, host, paths.flatMap((p) => viewer.dirtyUnder(p)), current);
  if (!removed || !current()) return;
  viewer.removed(removed);
  tree.selected.clear();
  await tree.refreshDirs([...new Set(paths.map(dirname))]);
  scheduleSave();
}

const copyPaths = (text: string) => copyText(text, text.includes("\n") ? `Copied ${text.split("\n").length} paths` : `Copied ${text}`);

const relative = (p: string) => {
  const root = tree.root?.path ?? "";
  return p === root ? "." : p.startsWith(root + "/") ? p.slice(root.length + 1) : p;
};

/** terminumd quotes paths for the pane's shell and types into the shown pane only (AC-60). */
const terminal = (action: "insert" | "cd", body: object) =>
  key ? apiOrToast("POST", `/api/terminal/${action}`, { body: { ...body, key } }) : toast("No pane shown — nothing typed");

// ── layout ───────────────────────────────────────────────────────────────────

/** Preferences not yet saved (a failed PUT): the next change or reconnect sends them again. */
let prefsUnsaved = false, prefsSent = 0;
const savePrefs = () => {
  prefsUnsaved = true;
  const sent = ++prefsSent;   // only the newest PUT's answer says whether they are saved
  void api("PUT", "/api/prefs", { body: prefs }).then(
    () => { if (sent === prefsSent) prefsUnsaved = false; warned.delete("prefs"); },
    (e: Error) => failed("prefs", `Preferences not saved: ${e.message}`));
};

function setSplit(r: number) {
  prefs.split = Math.min(0.85, Math.max(0.15, r));
  $("app").style.setProperty("--split", `${prefs.split * 100}%`);
}

function initSplitter() {
  const sp = $("splitter");
  sp.addEventListener("pointerdown", (e) => {
    sp.setPointerCapture(e.pointerId);
    const app = $("app").getBoundingClientRect(), top = $("tree").getBoundingClientRect().top;
    const move = (ev: PointerEvent) => setSplit((ev.clientY - top) / (app.bottom - top));
    const up = () => { sp.removeEventListener("pointermove", move); sp.removeEventListener("pointerup", up); savePrefs(); };
    sp.addEventListener("pointermove", move);
    sp.addEventListener("pointerup", up);
  });
  sp.addEventListener("dblclick", () => { setSplit(0.5); savePrefs(); });
}

// ── wiring ───────────────────────────────────────────────────────────────────

// ⌘-click asks the page for a window of its own (AC-70); without a page around, a tab here
const openWindow = (path: string) => { if (!openOutside(path, scope, true)) viewer.open(path); };
const openFile = (path: string) => { if (!openOutside(path, scope)) viewer.open(path); };

const tree = new Tree($("tree"), {
  open: openFile,
  openWindow,
  changed: scheduleSave,
  context: (p, isDir, ev) => void contextMenu(p, isDir, ev, {
    tree, term: () => term, switchedSince, open: openFile, openWindow, trash: () => void trashSelected(),
    copy: (t) => void copyPaths(t), relative, terminal,
    sort: () => prefs.sort, setSort: (s) => { prefs.sort = s; tree.setSort(s); savePrefs(); },
  }),
  commit,
  notify: toast,
});
// a row dragged onto the page's terminal puts its path at the input; a selected one, the selection's (AC-36)
bindHold($("tree"));
bindDrag($("tree"), (p) => (tree.selected.has(p) ? [p, ...[...tree.selected].filter((q) => q !== p)] : [p]), () => scope);
// files dragged from Finder upload into the folder under the pointer, by the tree's host (AC-95)
bindDrop($("tree"), { root: () => tree.root?.path ?? null, host: () => scope, readOnly: (d) => tree.readOnly(d), mark: (d) => tree.setDrop(d), toast });

const viewer = new Viewer($("tabs"), $("tools"), $("vbody"), {
  changed: scheduleSave,
  layout: (has) => $("app").classList.toggle("has-tabs", has),
  reveal: (p) => { if (!revealOutside(p)) revealInTree(p); },
  canReveal: (p) => { const root = VIEW ? (pageRoot?.host === scope ? pageRoot.path : null) : tree.root?.path; return !!root && isUnder(p, root) && p !== root; },
  active: (p) => pathBar?.show(p),
  emptied: () => { if (VIEW) emptiedOutside(); },
});
let pathBar: PathBar | null = null; // viewer window only
let pageRoot: { path: string; host: string | null } | null = null; // View: the folder the page's Files shows (the page says)
/** Reveal in tree: its folders expand, it is selected; a file outside the tree's folder (the pane's) says so. */
function revealInTree(p: string) {
  void tree.reveal(p, true).then((found) => {
    if (!found) return toast(tree.root && !isUnder(p, tree.root.path) ? `Not in this tree: ${p} is outside ${tree.root.path}` : `Not found in the tree: ${p}`);
    tree.selected = new Set([p]);
    tree.queue();
    scheduleSave();
  });
}

const busy = () => viewer.hasDirty || !!document.querySelector(".modal, .inline-edit.on, dialog[open]");
// The page reloads after an upgrade only while none of its frames is busy (AC-16).
if (PROXIED) (window as unknown as { fbBusy: () => boolean }).fbBusy = busy;
const upgrade = new Upgrade({
  busy,
  beforeReload: () => saveNow(),
  waiting: (text) => {
    if (updateNote) return;
    updateNote = text;
    if (VIEW) toast(text); else renderHeader(term);
  },
});

const filterEl = $<HTMLInputElement>("filter");
filterEl.addEventListener("input", () => {
  clearTimeout(filterTimer);
  filterTimer = window.setTimeout(() => tree.setFilter(filterEl.value), 150);
});
filterEl.addEventListener("keydown", (e) => {
  if (e.key === "Escape") { filterEl.value = ""; tree.setFilter(""); tree.focus(); }
  if (e.key === "ArrowDown" || e.key === "Enter") { e.preventDefault(); tree.focus(); }
});
const newItem = (mode: "file" | "folder") => { const d = tree.targetDir(); if (d) void tree.startCreate(d, mode); };
$("refresh").onclick = () => { tree.reload(); viewer.reloadAll(); };
$("collapse").onclick = () => tree.collapseAll();
$("expand").onclick = () => void tree.expandAll().then((m) => { if (m) toast(m); });
$("new-file").onclick = () => newItem("file");
$("new-folder").onclick = () => newItem("folder");
$("hidden").onclick = () => {
  prefs.hidden = !prefs.hidden;
  $("hidden").classList.toggle("on", !prefs.hidden);
  tree.setShowHidden(prefs.hidden);
  savePrefs();
};
$("maximize").onclick = () => $("app").classList.toggle("max");

// Keyboard. ⌘-shortcuts the browser owns (⌘N, ⌘W, ⌘T) are avoided; ⌥-variants are used
// instead. ⌘S is also bound inside the editor.
document.addEventListener("keydown", (e) => {
  if (document.querySelector("dialog[open]")) return;
  const t = e.target as HTMLElement;
  const inField = t.matches("input, textarea") || !!t.closest(".cm-editor");
  if (e.key === "/" && !inField) { e.preventDefault(); filterEl.focus(); return; }
  // in View an Esc nothing took first (the editor's search panel, a completion list) hides it (AC-70)
  if (e.key === "Escape" && VIEW && !e.defaultPrevented && !t.closest(".cm-panels, .cm-tooltip") && escapedOutside()) { e.preventDefault(); return; }
  // in the tree it hides View too, the keys staying here: the next file is a ↓ and Enter away
  if (e.key === "Escape" && !VIEW && !e.defaultPrevented && t.closest("#tree") && escapedOutside()) return;
  if (e.key === "Escape" && t.closest("#viewer") && !t.closest(".cm-panels")) { tree.focus(); return; }
  if (inField) return;
  const inTree = !!t.closest("#tree");
  if (e.altKey && !e.metaKey && e.code === "KeyN") { e.preventDefault(); newItem(e.shiftKey ? "folder" : "file"); }
  else if (e.metaKey && e.key === "Backspace" && inTree) { e.preventDefault(); void trashSelected(); }
  else if (e.metaKey && e.key === "Enter" && inTree) {
    e.preventDefault();
    const p = tree.cursorPath;
    if (p && tree.targetDir() !== p) openWindow(p); // a file, not a folder
  }
  else if (e.metaKey && e.altKey && e.code === "KeyC" && inTree) {
    e.preventDefault();
    const sel = [...tree.selected];
    void copyPaths((e.shiftKey ? sel.map(relative) : sel).join("\n"));
  }
  else if (e.ctrlKey && e.key === "Tab") { e.preventDefault(); viewer.cycle(e.shiftKey ? -1 : 1); }
  else if (e.altKey && e.code === "KeyW") { e.preventDefault(); viewer.closeActive(); }
  else if (e.metaKey && e.key.toLowerCase() === "s" && viewer.active != null) {
    e.preventDefault();
    void viewer.save(viewer.tabs[viewer.active].path);
  }
});

window.addEventListener("beforeunload", (e) => {
  void saveNow();
  if (viewer.hasDirty) e.preventDefault();
});

function startViewer(path: string) {
  document.documentElement.classList.add("viewer-mode");
  setScope(params.get("host")); // a remote file's viewer (AC 82)
  viewer.setHost(scope);
  pathBar = new PathBar($("pathbar"), copyPaths);
  pathBar.show(path);
  connectStream();
  viewer.open(path, "auto");
}

acceptFromPage({ open: (p) => viewer.open(p), reveal: revealInTree, root: (path, host) => { pageRoot = path ? { path, host } : null; viewer.refreshTools(); }, theme: (t) => { if (applyTheme(t)) { if (!VIEW) tree.themeChanged(); void redrawDiagrams(); } }, park });
if (!PROXIED) notice("Not served by terminumd", "Open Files from terminum's page.");

(async () => {
  if (VIEW) return startViewer(VIEW);
  initSplitter();
  renderHeader(term);
  // this panel starts from the latest layout; changing it later updates the default only
  let denied = "";
  const saved = await api<Partial<Prefs> | null>("GET", "/api/prefs").catch((e: ApiError) => {
    if (e.status === 401) denied = e.code === "bad_session" ? "Signed out: run terminum open" : e.message;
    return null;
  });
  if (denied) return authFailed(denied);
  prefs = { ...DEFAULT_PREFS, ...(saved ?? {}) };
  setSplit(prefs.split);
  tree.showHidden = prefs.hidden;
  if (!SORTS.some((s) => s.id === prefs.sort)) prefs.sort = DEFAULT_PREFS.sort;   // a newer build's, or none
  tree.sort = prefs.sort;
  $("hidden").classList.toggle("on", !prefs.hidden);
  tree.themeChanged();
  connectStream();
})().catch((e) => toast(String(e)));
