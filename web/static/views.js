// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// Terminal, Files and View (the session list is the drawer). Files is iterm-enhancer's panel
// pinned to the shown session; View (internally "file") is its viewer and editor, where every
// file opened from Files gets a tab. Below WIDE they are three views in the same place, and View
// has a button back to the terminal; at WIDE and up Files is docked on the right and View is a
// window over the terminal, each closed with its own [x]. Closing View's last file goes back to
// the terminal. Each session has its own View: the shown session's frame is #file, the others'
// wait hidden beside it with their tabs and unsaved edits (moving an iframe would reload it).
// Docked Files is open or closed for each session, in this browser (AC-70).

import { blobUrl } from "./signin.js";
import { following, onFollow } from "./follow.js";
import { openZoom } from "./zoom.js";

const $ = (id) => document.getElementById(id);
const NAMES = ["term", "files", "file"];
const WIDE = matchMedia("(min-width: 1400px)");

export class Views {
  constructor({ send, store, onShow, onBack }) {
    Object.assign(this, { send, store });
    this.onShow = onShow;          // the terminal was shown or changed width: it may need a redraw
    this.onBack = onBack;          // View closed its last file: the terminal may take the keys
    onFollow(() => this.send({ t: "files" }));   // the menu's switch: Files' root asked again (onFiles)
    this.current = "term";         // the view in the main column below WIDE
    this.dockedBy = store.get("filesDockedBy", {}) ?? {};   // the sessions whose docked Files are closed (the others': open)
    this.docked = true;            // Files is open on the right at WIDE, for the shown session
    this.floating = false;         // File is open over the terminal at WIDE
    this.filesSrc = "";            // what the Files frame shows: its pane, folder and host
    this.filesError = null;        // why the pane has no Files
    this.termShown = true;         // as terminumd knows it: frames are sent while it is shown (AC-02)
    this.filesShown = false;
    this.fileHost = null;          // the host the File frame's files are on
    this.sid = null;               // the shown session, whose View is #file
    this.theme = null;             // the shown pane's, for both frames (fbd's is the focused pane's)
    for (const f of ["files", "file"]) { const el = $(f); el.addEventListener("load", () => { if (el.id === f) this.tellTheme(el); if (el.id === "file") this.tellRoot(); }); }
    // another session's Files show once its own folder is in (the last one asked for)
    $("files").addEventListener("load", () => { if ($("files").getAttribute("src") === this.filesSrc) $("files").classList.remove("moving"); });
    for (const b of document.querySelectorAll("button[data-view]")) b.addEventListener("click", () => this.show(b.dataset.view, true));
    $("sideclose").addEventListener("click", () => this.setDocked(false));
    $("fileclose").addEventListener("click", () => this.show("term"));
    $("backterm").addEventListener("click", () => this.show("term"));
    $("filewin").addEventListener("keydown", (e) => { if (e.key === "Escape" && !e.defaultPrevented) this.escape(); });   // its head, its keys
    WIDE.addEventListener("change", () => {
      // The open view moves between the main column and its wide place.
      if (WIDE.matches && this.current === "file") this.floating = true;
      if (WIDE.matches && this.current === "files") this.setDocked(true);
      if (!WIDE.matches) this.current = this.floating ? "file" : "term";   // docked Files was beside it
      if (WIDE.matches) this.current = "term";
      else this.floating = false;
      this.layout();
    });
    addEventListener("message", (e) => this.onMessage(e));
    this.layout();
  }

  /** Shows a view; a click on a view button at WIDE opens or closes Files or File instead. */
  show(name, clicked = false) {
    if (!WIDE.matches) this.current = name;
    else if (name === "files") return this.setDocked(clicked ? !this.docked : true);
    else if (name === "file") this.floating = clicked ? !this.floating : true;
    else this.floating = false;
    this.layout();
  }

  /** Esc in View (its frame tells, fb-esc, or its head): hidden as its × hides it, the terminal shown with the keys (AC-70). */
  escape() {
    if (document.activeElement === $("file")) $("file").blur();
    this.show("term");
    this.onBack();
  }

  /** Esc in Files' tree: View's window hidden, the keys left in the tree (AC-70). */
  hideView() {
    if (!WIDE.matches || !this.floating) return;
    this.floating = false;
    this.layout();
  }

  /** ⌘B: Files open or closed (docked at WIDE, else in the terminal's place). */
  toggleFiles() {
    if (WIDE.matches) this.setDocked(!this.docked);
    else this.show(this.current === "files" ? "term" : "files");
    return true;
  }

  setDocked(on) {
    this.docked = on;
    if (this.sid) { if (on) delete this.dockedBy[this.sid]; else this.dockedBy[this.sid] = false; this.store.set("filesDockedBy", this.dockedBy); }
    this.layout();
  }

  layout() {
    const wide = WIDE.matches;
    const shown = {
      term: wide || this.current === "term",
      files: wide ? this.docked : this.current === "files",
      file: wide ? this.floating : this.current === "file",
    };
    document.body.classList.toggle("wide", wide);
    document.body.classList.toggle("docked", wide && this.docked);
    for (const n of NAMES) document.querySelector(`button[data-view="${n}"]`).setAttribute("aria-selected", String(shown[n]));
    $("term").hidden = !shown.term;
    if (shown.term !== this.termShown) this.send({ t: "screens", on: shown.term });   // hidden: no frames for it; shown: all anew (AC-02)
    this.termShown = shown.term;
    // a selection left in the hidden terminal would keep it paused, and iOS its Copy menu up
    const sel = getSelection();
    if (!shown.term && sel?.anchorNode && $("term").contains(sel.anchorNode)) sel.removeAllRanges();
    $("files").hidden = !shown.files || !!this.filesError;
    $("viewnote").hidden = !shown.files || !this.filesError;
    $("viewnote").textContent = this.filesError ?? "";
    $("filewin").hidden = !shown.file;
    for (const f of $("filewin").querySelectorAll("iframe")) f.hidden = f.id !== "file";
    document.body.dataset.shown = wide ? "term" : this.current;
    if (shown.files && !this.filesShown) this.send({ t: "files" });   // the pane may have changed folder since
    this.filesShown = shown.files;
    if (shown.term) this.onShow();
  }

  /** Another session is shown: its Files are another folder, and its View its own files: the
   *  View shown so far waits for its session, open or closed as it was. */
  sessionChanged(sid) {
    this.follow(sid);
    this.filesSrc = "";
    this.filesError = null;
    // hidden, not emptied: WebKit may load an emptied frame's about:blank after the next folder
    // set meanwhile, and the frame stays blank (a quick switch away and back)
    $("files").classList.add("moving");
    if (this.filesShown) this.send({ t: "files" });
  }

  /** The view of session `sid`: its View, its docked Files open or closed (AC-70). */
  follow(sid) { if (sid !== this.sid) this.switchView(sid); }

  switchView(sid) {
    const old = $("file"), button = document.querySelector('button[data-view="file"]');
    const open = WIDE.matches ? this.floating : this.current === "file";
    // a View with files waits for its session; an empty one (never used, or its last file closed) goes
    // a waiting View's event stream pauses (fb-park) until it is shown again
    if (old.getAttribute("src") && !button.disabled) {
      Object.assign(old.dataset, { sid: this.sid ?? "", host: this.fileHost ?? "", open: String(open) });
      old.contentWindow?.postMessage({ type: "fb-park", park: true }, location.origin);
    } else old.remove();
    old.removeAttribute("id");
    let next = [...$("filewin").querySelectorAll("iframe[data-sid]")].find((f) => f.dataset.sid === (sid ?? ""));
    if (next) next.contentWindow?.postMessage({ type: "fb-park", park: false }, location.origin);
    else {
      next = Object.assign(document.createElement("iframe"), { title: "View" });
      $("backterm").before(next);
      next.addEventListener("load", () => { if (next.id === "file") { this.tellTheme(next); this.tellRoot(); } });
    }
    next.id = "file";
    this.sid = sid;
    this.tellRoot();
    this.docked = this.dockedBy[sid] ?? true;
    this.fileHost = next.dataset.host || null;
    const reopen = next.dataset.open === "true";
    delete next.dataset.sid; delete next.dataset.open; delete next.dataset.host;
    button.disabled = !next.getAttribute("src");
    if (WIDE.matches) this.floating = reopen;
    else if (this.current === "file" || reopen) this.current = reopen ? "file" : "term";
    this.layout();
  }

  /** A new layout: a waiting View whose session closed goes, unless it holds unsaved edits or
   *  its server is only down for now (a host reconnecting lists no sessions). */
  forget(groups) {
    const live = new Set(groups.flatMap((g) => g.items.map((it) => it.id)));
    const down = new Set(groups.filter((g) => g.server && g.state && g.state !== "attached").map((g) => g.server));
    for (const f of $("filewin").querySelectorAll("iframe[data-sid]")) {
      const sid = f.dataset.sid, server = sid.split(":").slice(1, 3).join(":");
      let dirty = true;
      try { dirty = !!f.contentWindow?.fbBusy?.(); } catch { /* not loaded yet: keep it */ }
      if (!live.has(sid) && !down.has(server) && !dirty) f.remove();
    }
    // a closed session's goes; not one of a server this layout lists no pane of (its read failed, it reconnects)
    const listed = new Set([...live].map((sid) => sid.split(":").slice(1, 3).join(":")));
    const gone = Object.keys(this.dockedBy).filter((sid) => !live.has(sid) && listed.has(sid.split(":").slice(1, 3).join(":")));
    if (gone.length) { for (const sid of gone) delete this.dockedBy[sid]; this.store.set("filesDockedBy", this.dockedBy); }
  }

  setTheme(theme) {
    this.theme = theme;
    for (const f of ["files", "file"]) this.tellTheme($(f));
  }

  /** View: the folder Files shows and its host, where Reveal in tree can find a file (none: "");
   *  View shows Reveal only for a file under it. */
  tellRoot() {
    const r = this.filesRoot;
    if ($("file").getAttribute("src")) $("file").contentWindow?.postMessage({ type: "fb-root", path: r?.cwd ?? "", host: r?.host ?? null }, location.origin);
  }

  tellTheme(frame) {
    if (this.theme && frame.getAttribute("src")) frame.contentWindow?.postMessage({ type: "fb-theme", theme: this.theme }, location.origin);
  }

  /** The shown pane's folder, host and home, asked now (a "cd" may have moved it): for a path
   *  clicked in the terminal. {error} when it has none, null when the server does not answer. */
  paneInfo() {
    return new Promise((resolve) => {
      const timer = setTimeout(() => { this.waiters = this.waiters.filter((w) => w !== done); resolve(null); }, 5000);
      const done = (m) => { clearTimeout(timer); resolve(m); };
      (this.waiters ??= []).push(done);
      this.send({ t: "files" });
    });
  }

  /** The answer to paneInfo() (a quick shell's page has no Files to load). */
  answered(m) {
    for (const w of this.waiters?.splice(0) ?? []) w(m);
  }

  /** The server's answer to "files": the pane's key, folder and host, or why there are none. The
   *  root is the folder its coding agent works in when it has one and Files follows it (AC-51). */
  onFiles(m) {
    this.answered(m);
    this.filesError = m.error ?? null;
    const cwd = following() && m.agent ? m.agent : m.cwd;
    this.filesRoot = m.error || !cwd ? null : { cwd, host: m.host ?? null };
    this.tellRoot();
    if (!m.error && this.filesShown) {         // the Files frame loads only while it is shown
      const q = new URLSearchParams({ key: m.key, cwd: cwd ?? "" });
      if (m.host) q.set("host", m.host);
      const src = `/fb/?${q}`;
      if (src !== this.filesSrc) $("files").src = this.filesSrc = src;   // reloaded only when it moved
    } else $("files").classList.remove("moving");
    this.layout();
  }

  /** A file chosen in Files opens as a tab of File. */
  open(path, host) {
    const frame = $("file");
    if (this.fileHost === (host ?? null) && frame.contentWindow && frame.src) {
      frame.contentWindow.postMessage({ type: "fb-open", path }, location.origin);
    } else {
      const q = new URLSearchParams({ view: path });
      if (host) q.set("host", host);
      frame.src = `/fb/?${q}`;
      this.fileHost = host ?? null;
    }
    document.querySelector('button[data-view="file"]').disabled = false;
    this.show("file");
  }

  /** Files asks to open a file; View asks to reveal one in Files, or says its last file was
   *  closed; either shows an image full screen. Nothing else is listened to. */
  onMessage(e) {
    if (e.origin !== location.origin || typeof e.data?.path !== "string") return;
    const parked = [...$("filewin").querySelectorAll("iframe[data-sid]")].find((f) => f.contentWindow === e.source);
    if (parked && e.data.type === "fb-empty") return void parked.remove();   // a waiting View closed its last file
    if (e.source === $("file").contentWindow && e.data.type === "fb-esc") return this.escape();
    if (e.source === $("files").contentWindow && e.data.type === "fb-esc") return this.hideView();
    if (e.source === $("file").contentWindow && e.data.type === "fb-empty") {
      document.querySelector('button[data-view="file"]').disabled = true;
      if (!this.floating && this.current !== "file") return;
      if (document.activeElement === $("file")) $("file").blur();   // the keys leave the closed View
      this.show("term");
      this.onBack();
    }
    if (e.source === $("files").contentWindow && e.data.type === "fb-open") this.open(e.data.path, e.data.host ?? null);
    // an image tab's picture full screen: only a file's own URL from terminumd (/fb/api/raw), nothing else loads
    const frame = [$("file"), $("files"), parked].find((f) => f && f.contentWindow === e.source);
    if (frame && e.data.type === "fb-zoom") {
      const url = new URL(e.data.path, location.href);
      if (url.origin === location.origin && url.pathname === "/fb/api/raw") {
        // fetched with the sign-in, shown from a blob: (no address carries it), gone when it closes
        void blobUrl(url.href).then((got) => got.url && openZoom(got.url, { caption: String(e.data.caption ?? ""), alt: String(e.data.caption ?? ""),
          vector: /\.svg$/i.test(url.searchParams.get("path") ?? ""), onClose: () => { URL.revokeObjectURL(got.url); frame.focus(); } }));
      }
    }
    if (e.source === $("file").contentWindow && e.data.type === "fb-reveal") {
      this.show("files");
      $("files").contentWindow?.postMessage({ type: "fb-reveal", path: e.data.path }, location.origin);
    }
  }
}
