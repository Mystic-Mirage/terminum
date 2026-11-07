// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
import "./signin.js";                                // first: it signs every request the page makes
import { connection } from "./conn.js";
import { pageColors } from "./contrast.js";
import { oneKeyboard } from "./onekbd.js";
import "./press.js";                                // a pressed blob's pulse (motion.css)
import "./pop.js";                                  // where a window flies out of (motion.css)
import { Input } from "./input.js";
import { kbdPlacer } from "./kbdplace.js";
import { pageTasks } from "./pagetasks.js";
import { findItem, listParts, renderNav, seeWaits, serverDown, setTaskMarks, taskMarksOf } from "./nav.js";
import { pageHeader } from "./header.js";
import { banner } from "./banners.js";
import { sessionDrawer } from "./drawer.js";
import { sentHistory } from "./sent.js";
import { colorSchemes } from "./schemes.js";
import { menuPrefs } from "./prefs.js";
import { markDoc } from "./markdoc.js";
import { reminders as reminderBox } from "./reminders.js";
import { listFoot, showBuild } from "./foot.js";
import { cellAt, cursorCell, shellMove } from "./cursor.js";
import { BoxMoves } from "./boxnav.js";
import { selectionCut } from "./selcut.js";
import { switchGate } from "./switchgate.js";
import { askConfirm, navTools } from "./navtools.js";
import { dockGrip } from "./dock.js";
import { embedded, quickShell } from "./quick.js";
import { paneMenu } from "./menu.js";
import { paneSizing } from "./sizing.js";
import { pageSettings } from "./settings.js";
import { listPlace } from "./listplace.js";
import { arranged, bindReorder, listOrderer, ordered } from "./reorder.js";
import { linkMarks } from "./linkmarks.js";
import { paneFiles } from "./panefiles.js";
import { bindLinks } from "./termlinks.js";
import { copyButton, quietSelection } from "./touchcopy.js";
import { TermView } from "./term.js";
import { Views } from "./views.js";
import { uploads } from "./upload.js";
import { waitingCards } from "./waiting.js";
import { Widgets } from "./widgets.js";
import { Marks } from "./marks.js";
import { Jumper } from "./markjump.js";
import { fabFold } from "./fold.js";
import { promptRail } from "./promptrail.js";
import { askedMarks } from "./asked.js";
import { askBox } from "./ask.js";
import { recentSessions } from "./recent.js";
import { rowMenus, saveOrder } from "./rowmenu.js";
import { ideKeys } from "./idekeys.js";
import { editChoices, undoKey, undoTouch } from "./undo.js";
import * as trace from "./trace.js";
import { tipOfDay } from "./tips.js";
import { toast } from "./toast.js";

const $ = (id) => document.getElementById(id);
const narrow = matchMedia("(max-width: 860px)");
const touch = matchMedia("(hover: none)");

// a quick shell's page (embed mode, AC-88) reads this browser's choices and keeps none of its own
const store = {
  get(k, d) { try { const v = localStorage.getItem(k); return v === null ? d : JSON.parse(v); } catch { return d; } },
  set(k, v) { if (embedded) return; try { localStorage.setItem(k, JSON.stringify(v)); } catch { /* private mode */ } },
};
if (embedded) document.body.classList.add("embed");

const SIZE_MIN = 7, SIZE_MAX = 28;                 // the text sizes the − and + buttons allow
const tasks = pageTasks({ box: $("asks"), embedded, say: (...a) => toast(...a), isThere: (g, id) => !!findItem(g, id), down: (g, id) => serverDown(g, id) });
const st = {
  groups: [], sid: embedded ?? tasks.asked ?? store.get("sid", null), projects: [], listOrder: [],
  size: null, base: null, step: store.get("sizeStep", 0),
  collapsed: new Set(store.get("collapsed", [])),   // session groups closed in the list
  appCursor: false, bracketed: store.get("bracketed", true), insertClipboard: store.get("insertClipboard", true), atMac: false,
};
/** The layout row of pane `id`, or null. */
const itemOf = (id) => findItem(st.groups, id)?.item ?? null;

// ---------- connection (conn.js) ----------
const conn = connection({ parse: (data) => trace.parse(data), onMessage: (m) => onMessage(m), status,
  busy: () => [$("files"), ...$("filewin").querySelectorAll("iframe")].some((f) => { try { return f.contentWindow?.fbBusy?.(); } catch { return false; } }),
  onOpen: (ws) => {
    trace.connected(send, ws);
    look = "on";                                   // a new connection streams until told otherwise:
    syncActive();                                  // a hidden page says so before it shows a pane (AC-93: it reads nothing)
    if ($("term").hidden) send({ t: "screens", on: false });   // a terminal behind Files or View gets no frames (AC-02)
    if (st.sid) select(st.sid, true);
    if (!embedded || focusAsked) focusTerminal();  // a computer types into the chat at once; a phone opens no keyboard
  } });
// Nothing typed waits for a connection: it would arrive late, all at once (AC-14).
function send(m) {
  trace.sent(m);
  if (!conn.send(m) && (m.t === "in" || m.t === "paste" || m.t === "mouse")) toast("Not connected");
}
function status(text) { banner("status", text); }

// Pasted images, uploaded and dropped files: saved for the shown pane, their paths pasted.
const files = uploads({ sid: () => st.sid, host: () => itemOf(st.sid)?.host ?? null, atMac: () => st.atMac, paste: (text) => input.paste(text), lines: () => !!st.bracketed && !!itemOf(st.sid)?.agent, toast, hideToast: () => { $("toast").hidden = true; } });

// The lines kept on the page while it follows the output: each one is laid out on every frame, a
// phone's on every touch too (AC-52; a computer with 20,000: 39 ms a frame).
const view = new TermView({ term: $("term"), hist: $("hist"), screen: $("screen"), note: $("note"),
  bottomBtn: $("bottom"), paused: $("paused"), send, keep: touch.matches ? 2000 : 5000 });
// Sizing and full-screen programs (AC-04, AC-05, AC-13), settings.json's page defaults (AC-19): before anything draws.
const sizing = paneSizing({ view, send, sid: () => st.sid, refit: () => view.keepBottom(syncControls), notify: (text) => toast(text, 9000),
  copied: (pick) => bookmarks.picker.program(pick) });   // a full-screen program's own selection (AC-06)
// another pane opened: the one shown stays until the new one has this view's grid, then it is drawn at once (AC-04)
const gate = switchGate({ release: (queue, reset) => { reset(); for (const m of queue) onMessage(m); } });
const settings = pageSettings({ toast, setLayout: (rows) => { input.setLayout(rows); syncControls(); }, onPage: (page) => {
  if (page.textSize) setBase(page.textSize);
  prefs.page(page);
  schemes.page(page);
  view.keepBottom(syncControls);
} });

const input = new Input({ kbd: $("kbd"), kbdButton: $("kbdbtn"), term: $("term"), panel: $("hotkeys"), pasteDialog: $("pastebox"),
  send: (data, path, e) => { if (data === "\r") sent.enter(); send({ t: "in", data, ...trace.keyFields(data, path, e) }); sizing.typed(); if (data.includes("\r")) st.ran = true; },
  options: { appCursor: () => st.appCursor || view.alt, bracketed: () => st.bracketed, insertClipboard: () => st.insertClipboard, shown: () => !!st.sid, sid: () => st.sid, onTyped: (line, path) => { if (path !== "asked") view.typed(); st.undone = false; if (!line) boxMoves.forget(); }, status: toast, pasteImage: files.pasteImage, pickFile,
    tapLink: (x, y) => tapLink(x, y), tapMove: (x, y) => tapMove(x, y), holdMenu: (x, y) => editMenu(x, y), undo: () => undoRedo(false, true), inBox: () => !!boxMoves.state(), cellWidth: () => view.charRatio() * parseFloat(getComputedStyle($("term")).fontSize),
    // Ctrl+W deletes a word: in an agent's input box; at a shell's prompt, not after an Enter until the list says the shell is back (nano: search)
    lineMove: (d, repeat) => (findItem(st.groups, st.sid)?.item.agent ? boxMoves.line(d, st.appCursor, repeat) : null),
    wordDeletes: () => { const it = st.sid && findItem(st.groups, st.sid)?.item; return !!(it?.agent ? view.box : it?.shell && !st.ran); } } });
copyButton($("copyfab"), $("term"), (text) => input.copySelection(text));
quietSelection($("term"), touch, { hold: (on) => view.hold(on) });                   // no iOS menu over a selection: the page's Copy, [+], Ask (AC-37)

// The files the output names, on the pane's host: a path is a link only
// when its file is there (relative paths from the pane's folder, as a click takes them).
const shownFiles = paneFiles(() => views.paneInfo());
// Addresses and paths get a dotted underline (linkmarks.js).
const marks = linkMarks($("term"), [$("hist"), $("screen")], shownFiles);
const asked = askedMarks({ term: $("term"), boxes: [$("hist"), $("screen")], view, send, sid: () => st.sid });
const views = new Views({ send, store, onShow: () => { view.resume(); view.keepBottom(applyFit); marks(); }, onBack: () => focusTerminal() });
// the sessions shown last, ⌘[ and ⌘] through them; ⌘F the history's search, ⌘B Files (AC-07)
const recent = recentSessions({ store, exists: (id) => !!findItem(st.groups, id), show: (id) => pick(id) });
if (!embedded) ideKeys({ back: recent.back, forward: recent.forward, files: () => views.toggleFiles(), undo: undoRedo, cut: () => editMenu.cut() && (input.focus(), true),
  find: () => document.body.dataset.shown === "term" && !!st.sid && (bookmarks.search.toggle(true), true) });
/** Undo or redo in the shown pane (AC-03): its program's key, or why it has none. */
// said: a toast says it went (a swipe has no other sign); an Undo makes Redo apply, typing ends that
function undoRedo(redo, said = false) {
  const how = undoKey(itemOf(st.sid), redo);
  if (!how.bytes) return toast(how.why, 3000);
  send({ t: "in", data: how.bytes }); sizing.typed(); st.undone = !redo;
  if (said) toast(redo ? "Redo" : "Undo", 1500);
}
// Addresses open in a new tab here, paths in File (on the pane's host).
// (a quick shell's page has no View: its pane's page opens it)
const openFile = (p, host) => (embedded ? parent.postMessage({ type: "tm-open", path: p, host }, location.origin) : views.open(p, host));
const openAt = bindLinks($("term"), { paneInfo: () => views.paneInfo(), openFile, toast, state: shownFiles.state, pathOf: shownFiles.pathOf });
// Where a tap moves the cursor (a shell or a coding agent), on the cursor's row it does so, also
// over a path typed there; elsewhere a tap there opens a link.
function tapLink(x, y) {
  if (sizing.mouseOn()) return false;              // a tap is the program's click (AC-06)
  const it = st.sid && findItem(st.groups, st.sid)?.item;
  const cur = (it?.agent || it?.shell) && cursorCell($("screen")), at = cur && cellAt($("screen"), x, y);
  return !(at && (cur.row === at.row || (it.agent && boxMoves.has(x, y)))) && openAt(x, y);
}

// A tap or a click (input.js) on the input moves the cursor there with arrow keys: in a coding
// agent's input box by its text (boxnav.js: ← and → only, as the page draws its lines), on a
// shell's prompt along the row. Only where arrows edit a line (not htop, less, mc); never ↑ or ↓,
// which would recall history.
const boxMoves = new BoxMoves(view, $("screen"));
const sendCut = (data) => { input.forget?.(); send({ t: "in", data, ...trace.keyFields(data, "cut") }); view.typed(); };
selectionCut({ term: $("term"), cut: (range) => (!gate.holding() && findItem(st.groups, st.sid)?.item.agent ? boxMoves.cut(range, st.appCursor) : null),
  send: sendCut, focus: () => input.focus() });
function tapMove(x, y) {
  const found = st.sid && findItem(st.groups, st.sid);
  if ((!found?.item.agent && !found?.item.shell) || sizing.mouseOn() || gate.holding()) return;   // the screen drawn is the pane before
  const inBox = found.item.agent ? boxMoves.to(x, y, st.appCursor) : null;
  if (inBox !== null) { if (inBox) { input.forget?.(); send({ t: "in", data: inBox, ...trace.keyFields(inBox, "tap") }); view.typed(); } return; }
  const seq = shellMove($("screen"), view.screen, x, y, st.appCursor);
  if (seq) { input.forget?.(); send({ t: "in", data: seq, ...trace.keyFields(seq, "tap") }); }
  if (seq) view.typed();
}

// On a touch screen the invisible keyboard field lies over the cursor's row (kbdplace.js).
const placeKbd = kbdPlacer({ touch, mouseOn: () => sizing.mouseOn() });
view.onDrawn = () => { sizing.syncTui(); placeKbd(); marks(); asked.redraw(); Jumper.drawn(); trace.drawn(); };
new ResizeObserver(() => placeKbd()).observe($("term"));   // a banner above it moved the terminal: the field follows, never over the banner

// Code, Markdown and diagrams in the output as widgets (AC-56).
const widgets = new Widgets({ view, term: $("term"), boxes: [$("hist"), $("screen")],
  copy: (text) => input.copySelection(text), send, notify: (text) => toast(text, 3000), onZoomClosed: () => focusTerminal() });
// none over a full-screen program, nor when settings.json turned them off
const widgetsOn = () => !sizing.fullscreen() && settings.page().widgets !== false;
view.onRows = (full, keep) => { if (widgetsOn()) (keep ? widgets.keepPlace(() => widgets.update(full)) : widgets.update(full)); };
view.onReset = () => { widgets.reset(); shownFiles.forget(); };
// Bookmarks of a selection, with a note, kept on the Mac (AC-57)
const alive = (id) => findItem(st.groups, id)?.item.title ?? null;
const doc = markDoc({ alive });
// every pane of the layout, one list a layout (bookmarks ask for it on every frame)
let paneList = [null, []];
const panes = () => (paneList[0] === st.groups ? paneList[1] : (paneList = [st.groups, st.groups.flatMap((g) => g.items)])[1]);
const bookmarks = new Marks({ view, widgets, term: $("term"), send, sid: () => st.sid, alive, panes, projects: () => st.projects,
  openAll: (items, title) => doc.open(items, title), pick: (id) => pick(id), toast, onChange: () => { drawNav(); reminded.draw(); rail?.draw(); }, copy: (text) => input.copySelection(text, true),
  toChat: (text) => { input.paste(text); focusTerminal(); }, store });
trace.watch(view, { st, bookmarks, widgets, findItem, send, mode: () => (!sizing.fullscreen() ? ($("term").classList.contains("reflow") ? "wrap" : "rows") : view.tooWide() ? "scaled" : "grid"), copy: (text) => input.copySelection(text) });
let kbdFrame = 0;
$("term").addEventListener("scroll", () => { if (!kbdFrame) kbdFrame = requestAnimationFrame(() => { kbdFrame = 0; placeKbd(); }); }, { passive: true });

// On a phone an agent's status lines (its footer) stay behind a button, remembered per browser.
function showFooter(on) {
  $("term").classList.toggle("showfoot", on);
  $("footfab").setAttribute("aria-pressed", String(on));
  store.set("showFooter", on);
}
showFooter(store.get("showFooter", false));
$("footfab").addEventListener("click", () => view.keepBottom(() => showFooter(!$("term").classList.contains("showfoot"))));

function pickFile() { $("upload").value = ""; $("upload").click(); }
$("upload").addEventListener("change", () => { if ($("upload").files.length) files.uploadFiles([...$("upload").files]); });
$("attachfab").addEventListener("click", pickFile);
$("attachfab").addEventListener("mousedown", (e) => e.preventDefault());   // the terminal keeps the keys
files.bindDrop($("term"));


const tools = navTools({ send, store, current: () => (st.sid && findItem(st.groups, st.sid)) || null, groups: () => st.groups,
  onResize: () => view.keepBottom(applyFit), onRenamed: () => (computer.matches ? focusTerminal() : document.activeElement?.blur()) });
dockGrip({ store, cellWidth: () => view.charRatio() * view.chosenSize(), onResize: () => view.keepBottom(applyFit) });   // docked Files' width (AC-70)
const menu = paneMenu({ send, pick: (id) => pick(id), signOut: () => conn.signOut(), copy: (text) => input.copySelection(text) });
const quick = quickShell({ send, store, current: () => (st.sid && findItem(st.groups, st.sid)) || null, item: (id) => findItem(st.groups, id)?.item,
  pick: (id) => pick(id), askConfirm, narrow });

// ---------- the keys go back to the terminal (a computer only) ----------
// With a mouse and a hardware keyboard, typing goes to the terminal again when the user comes
// back to this window or tab (if it had the keys before), picks a session, or returns to the
// Terminal view; never while a field, Files or File, the paste dialog, a name or a menu has
// them. A touch screen gets no focus() it did not tap for: on iOS that raises the keyboard.
const computer = matchMedia("(hover: hover) and (pointer: fine)");
let termHadKeys = false;
function focusTerminal() {
  if (!computer.matches || $("app").hidden || document.body.dataset.shown !== "term") return;
  if (!$("menu").hidden || $("pastebox").open || bookmarks.busy() || tools.renaming()) return;
  if (document.activeElement?.closest?.("input, textarea:not(#kbd), select, iframe, [contenteditable]")) return;
  input.focus();
}
addEventListener("blur", () => { termHadKeys = document.activeElement === $("kbd"); });   // leaving: who had the keys
addEventListener("focus", () => { if (termHadKeys) focusTerminal(); });
document.addEventListener("visibilitychange", () => { if (!document.hidden && termHadKeys) focusTerminal(); });
for (const b of [document.querySelector('button[data-view="term"]'), $("backterm"), $("fileclose")]) b.addEventListener("click", () => focusTerminal());

// A hidden page (another tab, a minimized window, a locked phone) costs the Mac nothing: its stream
// pauses, and the panes' captures with it. On a computer a page out of focus (the user types in
// another app beside it) keeps its stream but is not the active page: it sizes nothing and reads no
// turn (AC-13, AC-93); focus in the Files frame is still this page's.
let look = "on";                               // on, away or hidden; as terminumd takes a page that connects
let parentActive = true;                       // embed mode: its pane's page is looked at
let focusAsked = false;                        // embed mode: its pane's page gave it the keys
function syncActive() {
  const now = document.hidden ? "hidden"
    : !computer.matches || document.hasFocus() || (!!embedded && parentActive) ? "on" : "away";
  if (now !== look) send(now === "on" ? { t: "resume" } : { t: "pause", hidden: now === "hidden" });
  look = now;
  quick.active(now === "on");
}
addEventListener("message", (e) => {
  if (e.origin !== location.origin) return;
  if (!embedded && e.source === $("quickframe").contentWindow && e.data?.type === "tm-open") return views.open(String(e.data.path), e.data.host ?? null);
  if (!embedded || e.source !== parent) return;
  if (e.data?.type === "tm-focus") { focusAsked = true; focusTerminal(); }
  if (e.data?.type === "tm-active") { parentActive = !!e.data.on; syncActive(); }
});
document.addEventListener("visibilitychange", syncActive);
addEventListener("focus", syncActive);
addEventListener("blur", () => setTimeout(syncActive, 0));   // the focus may have gone into the Files frame

function onMessage(m) {
  if (m.sid && m.sid !== st.sid) return;            // a frame from the session we just left
  if (gate.take(m)) return;                         // the pane just opened, until its first frame
  trace.received(m);
  bookmarks.seen(m);
  rail?.seen(m);
  ask.seen(m); asked.seen(m); rowMenu.seen(m);
  switch (m.t) {
    case "layout": st.groups = m.groups; tasks.layout(m.groups); place.note(); seeWaits(st.groups, st.sid); st.ran = false; if (!embedded) reminders.layout(m.groups); drawNav(); reminded.draw(); ask.sync(); tools.layoutCame(m.groups); views.forget(m.groups); break;
    case "theme": themeSent = m.theme; applyTheme(schemes.over(m.theme)); break;
    case "hist":                                    // a reset: another pane, or its scrollback cleared
      if (m.mode === "reset") widgets.folds.reset();
      widgets.folds.add(m.folds); view.onHist(m); break;
    case "folds": widgets.folds.add(m.items); widgets.keepPlace(() => widgets.update(false, true)); break;   // lines that scrolled off
    case "lines":
      if (m.error) { widgets.folds.failed(m.first); toast(m.error, 5000); break; }
      view.refill(m.first, m.lines); widgets.keepPlace(() => widgets.folds.filled(m.first, m.lines.length)); break;
    case "screen": if ("folds" in m) widgets.folds.screen(m.folds); view.onScreen(m); sizing.sync(); break;
    case "sized": sizing.onSized(m); break;
    case "copied": sizing.onCopied(m); break;   // what a full-screen program copied after our release (AC-06)
    case "settings": settings.apply(m); break;
    case "restore": if (!embedded) menu.showRestore(m.report); break;
    case "files": if (embedded) views.answered(m); else views.onFiles(m); break;   // a quick shell has no Files
    case "quickmade": quick.made(m); break;
    case "hostadded": case "hostagents": tools.hostAnswer(m); break;
    case "tmuxadded": tools.tmuxAdded(); break;
    case "tmuxsessions": tools.tmuxChoices(m); break;
    case "error": if (!m.msg) status(""); else if (!tools.formError(m.msg)) status(m.msg); break;   // empty: tmux is back
    case "update": if (!embedded) menu.showUpdate(m.latest); break;
    case "note": toast(m.msg, 4000); break;
    case "stall": tools.showStall(m); drawNav(); break;
    case "build": conn.checkBuild(m.id); showBuild(m); st.atMac = m.mac === true; break;
    case "trace": trace.onMessage(m); break;
    case "plan": case "plannew": showCurrent.plan.onMessage(m); break;   // the Plan pane (AC-97)
    case "sentone": case "sentnew": sent.onMessage(m); break;
    case "projects": st.projects = Array.isArray(m.items) ? m.items : []; st.listOrder = Array.isArray(m.order) ? m.order : []; place.note(); bookmarks.changed(); break;   // the list, and the projects the list below it names
    case "taskmarks": setTaskMarks(m); bookmarks.changed(); tasks.marks(m); break;   // the rows' marks, the Plan button (terminum-kanban.md AC-113)
    case "created": if (!m.stay) pick(m.id); if (m.note) toast(m.note, 6000); break;
  }
}

// ---------- navigation ----------

// Rows reorder tmux's windows in their session; projects and groups reorder the list, every page's.
// Clone (AC-07): a new window right after the row's, in its folder; in its project when it has one (AC-92)
function cloneRow(item) { send({ t: "clone", id: item.id }); }
const orderList = listOrderer(st, () => arranged(drawnGroups(), st.projects, st.listOrder).map((p) => p.id), send, () => drawNav());
const rowMenu = rowMenus({ list: $("sessions"), send, item: itemOf, projects: () => st.projects, clone: cloneRow, orderList,
  lastLines: () => settings.page().lastLines ?? 0, copy: (text) => input.copySelection(text), say: (text) => toast(text, 5000),
  groups: () => st.groups, split: (it, side) => { pick(it.id); quick.open(side); }, keepFolded: (from, to) => { if (st.collapsed.has(from)) { st.collapsed.add(to); store.set("collapsed", [...st.collapsed]); } } });

// a finger resting on the keyboard button or held on the input offers Paste, Undo and Redo (AC-03)
const editMenu = undoTouch({ button: $("kbdbtn"), field: $("kbd"), menu: rowMenu, undo: undoRedo, paste: () => input.pasteFromClipboard(), can: editChoices({ term: $("term"), item: () => itemOf(st.sid), undone: () => !!st.undone,
  cuttable: (range) => !gate.holding() && !!itemOf(st.sid)?.agent && boxMoves.cuttable(range), cut: (range) => boxMoves.cut(range, st.appCursor),
  copy: (text) => input.copySelection(text, true), send: sendCut, say: (text) => toast(text, 3000) }) });
const reorder = bindReorder($("sessions"), (kind, parent, ids) => {
  if (kind === "part") orderList(ids);
  else saveOrder(send, parent, ids);
}, () => drawNav(), rowMenu.held);                  // a layout may have come while dragging

function pick(id) {
  const of = !narrow.matches && findItem(st.groups, id)?.item.quickOf;   // a quick shell's row: its pane, with it
  select(of || id); closeDrawer();
  if (of) quick.focus(); else focusTerminal();
}
const reminders = waitingCards($("asks"), pick);
const sent = sentHistory({ view, send, item: () => itemOf(st.sid), copy: (text) => input.copySelection(text, true), toast, toChat: (text) => bookmarks.toChat(text),
  jump: (it) => bookmarks.jumpText(it) });
const rail = embedded ? null : promptRail({ view, term: $("term"), send, sid: () => st.sid, marks: bookmarks, toast });
// Add to Chat's paste ("asked") leaves the view where it was: the user reads on, the cursor out of sight (AC-37)
const ask = askBox({ send, toChat: (text, stay) => input.paste(text, stay ? "asked" : "paste"), keys: (data, stay) => input.type(data, stay ? "asked" : "ask"), closed: focusTerminal, sid: () => st.sid, agent: () => !!itemOf(st.sid)?.agent,
  pane: () => ({ cols: view.cols, rows: view.rows, used: view.boxHeight }), asked: asked.asked, toast });
bookmarks.onAsk = (text, line) => ask.open(text, line);
const reminded = reminderBox({ marks: bookmarks });
listFoot({ store });
const fold = fabFold({ away: () => { $("kbd").blur(); if (!$("hotkeys").hidden) showKeys(false); } });   // a phone's floating buttons under ⟨…⟩ (AC-18)
bookmarks.onPlaced = () => { fold.sync(); sent.place(); reminded.place(); };
const drawnGroups = () => ordered(st.groups, store.get("groupOrder", [])), place = listPlace({ sid: () => st.sid, parts: () => listParts(drawnGroups(), st.projects, st.listOrder) });
function drawNav() {
  if (embedded) return showCurrent(findItem(st.groups, st.sid) || null);
  reminders.show(st.sid);
  if (reorder.dragging()) return;                  // the next layout redraws it
  renderNav($("sessions"), drawnGroups(), st.sid, $("filter").value, pick, st.collapsed, toggleGroup,
    bookmarks.sids(), tools.hostMenu, (text) => input.copySelection(text, true), tools.importTmux,
    { projects: st.projects, onClone: cloneRow, order: st.listOrder, onProjectNew: (p, last) => rowMenu.newInProject(p, last?.id),
      onGroupNew: (g, last) => (last ? cloneRow(last) : send({ t: "new", group: g.wid })), onProjectMenu: rowMenu.headMenu, onProjectRename: rowMenu.rename, onGroupMenu: rowMenu.groupMenu, onReconnect: tools.reconnect });
  const found = st.sid && findItem(st.groups, st.sid);
  if (!found) {
    const stay = st.sid && serverDown(st.groups, st.sid), next = stay ? null : place.next();   // its project's or group's first, else the list's (AC-92); while its server is away, the pane (AC-132)
    if (next) select(next);
    else if (!stay) showCurrent(null);             // staying: its header too
    if (!st.sid && narrow.matches) openDrawer();
    return quick.sync();
  }
  showCurrent(found);
  quick.sync();
}

function toggleGroup(key) {
  if (!st.collapsed.delete(key)) st.collapsed.add(key);
  store.set("collapsed", [...st.collapsed]);
  drawNav();
}

// The header (header.js): the title, then the folder with its Copy, the window, what runs there.
const showCurrent = pageHeader({ copy: (text) => input.copySelection(text, true), renaming: () => tools.renaming(), setAgent: (a) => { view.setAgent(a); sizing.sync(); },
  send, sid: () => st.sid, toChat: (text) => bookmarks.toChat(text), tasks: () => taskMarksOf(st.sid), toast, onShown: tools.sayStalls });

function select(id, force = false) {
  if (id === st.sid && !force && view.screen.length) return;
  st.sid = id; store.set("sid", id); place.note(); seeWaits(st.groups, id);
  recent.shown(id);
  status("");                                      // an error about the pane shown before
  if (tools.renaming()) tools.endRename();         // a name being typed was for that pane
  gate.start(id, () => { view.reset(); view.end.pin(); });   // pinned: its last line, however late the layout settles
  trace.opening(id);
  views.follow(id);                                // its own Files open or closed (AC-70): the view's width
  send({ t: "sub", id, ...sizing.grid() });        // with this view's grid: its first frame comes at it (AC-04)
  views.sessionChanged(id);                        // after "sub": the server answers "files" for the subscribed pane
  sizing.opened();                                 // this view's size
  sent.shown();
  drawNav(); syncControls();
}

const { openDrawer, closeDrawer } = sessionDrawer({ narrow, store, drawNav, focusInput: () => input.focus(), peek: () => showCurrent.peek() });

// ---------- theme ----------

let shownTheme = "", shownFont = "", themeSent = null;
const prefs = menuPrefs({ store, send, toast, redraw: drawNav });       // Markdown size, history lines (AC-19)
// a color scheme picked in the menu goes over the session's theme at once (AC-12)
const schemes = colorSchemes({ box: $("schemes"), send, onPick: () => { if (themeSent) applyTheme(schemes.over(themeSent)); } });
function applyTheme(theme) {
  // sent with every subscribe and resume; the same one changes nothing (and rebuilds no line)
  const key = JSON.stringify(theme);
  if (key === shownTheme) return;
  shownTheme = key;
  views.setTheme(theme);
  widgets.setTheme(theme);
  pageColors(theme);
  if (!settings.page().textSize && theme.size) setBase(theme.size);
  if (theme.font && document.fonts && theme.font !== shownFont) {
    shownFont = theme.font;
    const face = new FontFace("TermFont", `local("${theme.font.replace(/["\\]/g, "")}")`);
    face.load().then((f) => document.fonts.add(f)).catch(() => { /* not on this device: the shipped one (fonts/fonts.css) */ });
  }
  view.setTheme(theme);
}

function setBase(px) { st.base = narrow.matches ? Math.min(px, 14) : px; setSize(st.base + st.step, false); }   // the theme's or settings.json's size, a phone's 14 at most; − and + step from it
function setSize(px, save = true) {
  st.size = Math.max(SIZE_MIN, Math.min(SIZE_MAX, Math.round(px)));
  document.documentElement.style.setProperty("--size", `${st.size}px`);
  $("sizeval").textContent = st.size;
  if (save) store.set("sizeStep", st.step = st.size - (st.base ?? st.size));   // a step kept per browser: a new theme size still shows
  sizing.changed();                                // the grid this view holds changed (AC-13)
}

// ---------- the pane in the view (AC-04) ----------
// The active page sizes the pane to its view (AC-13). The normal screen is drawn at the chosen
// text size whatever tmux's width, re-flowed when it is not the view's (sizing.js). A full-screen program's grid wider than
// the view (another page sized it, the user's own tmux not typed in yet, a window of several
// panes) is scaled to the view's width, down to SIZE_MIN px; smaller than that, it stays at the
// chosen size and scrolls sideways. While the view's new width is on its way to tmux, it stays
// at the chosen size, clipped (sizing.js).
function applyFit() {
  sizing.reflow();
  const hold = sizing.holding(), px = view.cols && !hold && sizing.fullscreen() && view.tooWide() ? view.fitPx() : 0;
  $("term").classList.toggle("holding", hold);
  $("term").style.fontSize = px >= SIZE_MIN ? `${px}px` : "";
}
view.onCols = () => view.keepBottom(applyFit);   // the pane's width changed
view.onAlt = () => { sizing.sync(); view.keepBottom(applyFit); };
document.fonts?.addEventListener("loadingdone", () => { view.ratio = 0; view.keepBottom(applyFit); });

function syncControls() {
  $("appcur").checked = st.appCursor;
  $("bracket").checked = st.bracketed;
  $("insclip").checked = st.insertClipboard;
  const keysOn = !$("hotkeys").hidden;
  $("keysbtn").setAttribute("aria-pressed", String(keysOn));
  $("keysfab").setAttribute("aria-pressed", String(keysOn));
  document.documentElement.style.setProperty("--keys-h", `${$("hotkeys").offsetHeight}px`);
  applyFit();
}

$("smaller").onclick = () => view.keepBottom(() => { setSize(st.size - 1); applyFit(); });
$("bigger").onclick = () => view.keepBottom(() => { setSize(st.size + 1); applyFit(); });
$("appcur").onchange = (e) => { st.appCursor = e.target.checked; };
$("bracket").onchange = (e) => { st.bracketed = e.target.checked; store.set("bracketed", st.bracketed); };
$("insclip").onchange = (e) => { st.insertClipboard = e.target.checked; store.set("insertClipboard", st.insertClipboard); };
if (!embedded) tipOfDay({ store });

// ---------- the on-screen keyboard ----------
// iOS Safari lays the keyboard over the page instead of shrinking it. Size the app to the
// visible part of the screen, so the terminal and the hot keys sit just above the keyboard,
// and give the space back when the keyboard closes.
const vv = window.visualViewport;
let fitted = "";
function fitViewport() {
  if (!vv) return;
  const now = `${Math.round(vv.height)} ${Math.round(vv.offsetTop)} ${innerWidth}`;
  if (now === fitted) return;                      // iOS sends scroll and resize for one change, and some for none
  fitted = now;
  const stick = view.atBottom();
  document.documentElement.style.setProperty("--app-h", `${Math.round(vv.height)}px`);
  $("app").style.transform = vv.offsetTop ? `translateY(${Math.round(vv.offsetTop)}px)` : "";
  applyFit();
  if (stick) view.toBottom();
}
vv?.addEventListener("resize", fitViewport);
vv?.addEventListener("scroll", fitViewport);
addEventListener("resize", fitViewport);   // a window resize does not always reach visualViewport

function showKeys(on) {
  $("hotkeys").hidden = !on;
  store.set("keys", on);
  view.keepBottom(syncControls);
}
$("keysbtn").onclick = () => showKeys($("hotkeys").hidden);
const keyboards = oneKeyboard({ kbd: $("kbd"), panel: $("hotkeys"), show: showKeys });   // the on-screen keyboard or the hot keys
$("keysfab").onclick = () => keyboards.toggle();
$("keysfab").onmousedown = (e) => e.preventDefault();      // the iOS keyboard stays up
document.addEventListener("keydown", (e) => { if (e.key === "Escape") { menu.close(); closeDrawer(); } });

// Toolbar buttons never take focus from the terminal: Enter must keep going to the shell.
$("bar").addEventListener("mousedown", (e) => { if (e.target.closest("#tools button")) e.preventDefault(); });

$("hotkeys").hidden = !store.get("keys", false);
syncControls();
conn.connect();
