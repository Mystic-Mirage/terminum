// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// Keyboard, paste and the hot keys panel. Everything typed ends up in send(bytes).
import { HeldDelete, WORD } from "./heldelete.js";
import { inSelection, kbdTouch, MENU_AFTER, selectWordAt, TAP } from "./kbdtouch.js";
import { ctrlChar, encodeKey } from "./keys.js";
import { copyPreview, hiddenNote, selectedText, unhidden } from "./seltext.js";
import { copyByCommand, keepSelectionAt } from "./touchcopy.js";
import { composing } from "./trace.js";
import { inWidget } from "./widgetui.js";

/** Said when a paste begun in one pane would land in another (input.js, upload.js). */
export const NOT_PASTED = "Another session is shown now: nothing was pasted.";

// Four rows of six keys, grouped by what they do: text and the clipboard (with Upload), control
// keys, the arrows with their modifiers, and moving around. Fn swaps the last two rows for the
// F keys (mc, htop), so the panel never grows. The keyboard has its own floating button.
const HOTKEYS = [
  [["Copy", "copy", "Copy the selected text"], ["Paste", "paste"], ["Upload", "upload", "Upload files and paste their paths"],
    ["⇧↩", "\x1b[13;2u", "Shift+Enter: a new line in apps that tell it from Enter"], ["Tab", "\t"], ["Enter", "\r", "Enter (Return)", "strong"]],
  [["Esc", "\x1b"], ["^C", "\x03", "Interrupt"], ["^D", "\x04", "End of input"], ["^Z", "\x1a", "Suspend"],
    ["^R", "\x12", "Search history"], ["Fn", "fn", "F1–F12 in place of the last two rows"]],
  [["Ctrl", "mod:ctrl"], ["Alt", "mod:alt"], ["←", "arrow:D"], ["↑", "arrow:A"], ["↓", "arrow:B"], ["→", "arrow:C"]],
  [["Home", "\x1b[H"], ["End", "\x1b[F"], ["PgUp", "\x1b[5~"], ["PgDn", "\x1b[6~"], ["⇧←", "\x1b[1;2D", "Shift+Left"],
    ["⇧Tab", "\x1b[Z", "Shift+Tab"]],
];
const FN_ROWS = [
  [["F1", "\x1bOP"], ["F2", "\x1bOQ"], ["F3", "\x1bOR"], ["F4", "\x1bOS"], ["F5", "\x1b[15~"], ["F6", "\x1b[17~"]],
  [["F7", "\x1b[18~"], ["F8", "\x1b[19~"], ["F9", "\x1b[20~"], ["F10", "\x1b[21~"], ["F11", "\x1b[23~"], ["F12", "\x1b[24~"]],
];

const HOLD = 400, REPEAT = 33;   // ms: as a keyboard's repeat
const DELETES = { deleteContentBackward: "\x7f", deleteWordBackward: "\x7f", deleteContentForward: "\x1b[3~" };   // a word as one backspace where ^W is not a word (nano: search)
// On a touch screen the keyboard field keeps invisible characters on both sides of its caret: iOS
// repeats a held delete only while there is something to delete, and its space bar's trackpad
// moves the caret only where there is text to move over. Between them it keeps what was typed
// since a line end or a move: iOS aims its keys by the word being typed (an emptied field made
// every letter a word's first, and more typos).
const KEEP = "\u200b", PAD = KEEP.repeat(32);
const KEPT_MAX = 60;       // characters kept at most
const chars = (s) => Array.from(s);
const TAP_CARET = 600;     // ms after a finger on the field: a caret it moved is no trackpad's
// keys that repeat while held: not the actions, the modifiers, Enter and the signals
const REPEATS = (a) => !/^(copy|paste|upload|fn|mod:)/.test(a) && !["\r", "\x03", "\x04", "\x1a"].includes(a);

export class Input {
  constructor({ kbd, kbdButton, term, panel, pasteDialog, send, options }) {
    Object.assign(this, { kbd, kbdButton, term, panel, pasteDialog, send, options });
    this.mods = { ctrl: false, alt: false };
    this.held = new HeldDelete();
    this.buildPanel();
    this.bind();
  }

  // path and e: how it was typed and when, for the latency trace (trace.js)
  type(text, path = "other", e = null) {
    if (!text) return;
    if (path !== "input" && path !== "compose") this.forget?.();   // a key, a paste, a move: what the field kept is not before the cursor now
    this.send(text, path, e); this.options.onTyped(path === "line", path);
  }

  withMods(text) {
    let out = text;
    if (this.mods.ctrl && text.length === 1) out = ctrlChar(text) ?? text;
    if (this.mods.alt) out = "\x1b" + out;
    if (this.mods.ctrl || this.mods.alt) { this.mods.ctrl = this.mods.alt = false; this.syncMods(); }
    return out;
  }

  bind() {
    const k = this.kbd;
    // macOS's press-and-hold sends no repeat for a held character key in a text field (iTerm2
    // turns it off for itself): the page repeats it, after HOLD ms every REPEAT ms, until it
    // is let go. A repeat from the system takes over, so no key comes twice.
    let char = null;
    const stopChar = () => { if (char) { clearTimeout(char.timer); clearInterval(char.timer); char = null; } };
    const touchable = (this.touchable = navigator.maxTouchPoints > 0);
    // kept: the typed text left of the caret (touch); a change to it is sent as deletes and what is new
    let kept = "", composingNow = false, touchedAt = -Infinity;
    const refill = () => {
      if (!touchable) return;
      kept = "";
      if (k.value !== PAD + PAD) k.value = PAD + PAD;
      if (k.selectionStart !== PAD.length || k.selectionEnd !== PAD.length) k.setSelectionRange(PAD.length, PAD.length);
    };
    this.forget = refill;      // the pane's cursor moved elsewhere (a tap)
    k.addEventListener("focus", refill);
    // what the field holds now against what was kept: iOS's corrections and suggestions too
    const typedInField = (e) => {
      const now = k.value.replaceAll(KEEP, ""), was = chars(kept), is = chars(now);   // by characters, not UTF-16 halves
      let c = 0;
      while (c < was.length && c < is.length && was[c] === is[c]) c++;
      const out = "\x7f".repeat(was.length - c) + is.slice(c).join(""), modded = this.mods.ctrl || this.mods.alt;
      kept = now;
      if (modded || /\n/.test(now) || now.length > KEPT_MAX || k.selectionStart !== k.value.indexOf(now) + now.length) refill();
      this.type(this.withMods(out.replace(/\n/g, "\r")), "input", e);
    };
    // the space bar's trackpad moved the caret: as many ← or → (a jump to an end is an up or down
    // move of a one-line field, not ours to guess)
    document.addEventListener("selectionchange", () => {
      if (!touchable || document.activeElement !== k || composingNow) return;
      const at = k.selectionStart, want = PAD.length + kept.length;
      if (at === want && k.selectionEnd === at) return;
      const tapped = performance.now() - touchedAt < TAP_CARET, chose = k.selectionEnd !== at;
      // the characters passed, not UTF-16 halves: an emoji is one move
      const by = at < want ? -chars(k.value.slice(at, want)).length : chars(k.value.slice(want, at)).length;
      refill();
      if (tapped || chose || Math.abs(at - want) >= PAD.length) return;   // a selection in the field is no move
      const dir = by > 0 ? "C" : "D", arrow = this.options.appCursor() ? `\x1bO${dir}` : `\x1b[${dir}`;
      this.type(arrow.repeat(Math.abs(by)), "slide");
    });
    k.addEventListener("keydown", (e) => {
      if (e.isComposing) return;
      // a delete takes the kept character, and the input event sends it: a held delete repeats
      if (touchable && e.key === "Backspace" && !e.ctrlKey && !e.metaKey && !e.altKey && (k.selectionStart > 0 || k.selectionEnd > k.selectionStart)) { stopChar(); return; }
      if (e.repeat) { if (char?.code === e.code) stopChar(); }
      else if (e.key.length === 1 && !e.ctrlKey && !e.metaKey && !e.altKey) {
        stopChar();
        const held = char = { code: e.code, key: e.key };
        held.timer = setTimeout(() => {
          if (char === held) held.timer = setInterval(() => this.type(held.key, "keydown"), REPEAT);
        }, HOLD);
      } else stopChar();
      let seq = encodeKey(e, this.options.appCursor());
      if (seq === "\x7f" && e.key === "Backspace") seq = this.backspace(e.repeat);   // held: words after a while
      const line = /^Arrow(Up|Down)$/.test(e.key) && !e.shiftKey && !e.altKey && !e.ctrlKey && !e.metaKey && !this.mods.ctrl && !this.mods.alt
        ? this.options.lineMove?.(e.key === "ArrowUp" ? -1 : 1, e.repeat) ?? null : null;   // by the page's lines in an agent's input (boxnav.js)
      if (line !== null) seq = line;
      if (seq !== null) { e.preventDefault(); refill(); if (seq) this.type(this.withMods(seq), line ? "line" : "keydown", e); }
    });
    k.addEventListener("input", (e) => {
      if (e.isComposing) return;
      if (touchable && e.inputType?.startsWith("delete")) {           // iOS's delete, held or not
        // what iOS took of what was kept (a letter, or a word when held): a backspace a character
        const now = k.value.replaceAll(KEEP, ""), gone = chars(kept).length - chars(now).length;
        if (/^delete(Content|Word)Backward$/.test(e.inputType) && gone > 0 && kept.startsWith(now)) {
          kept = now;
          if (!now) refill();
          return this.type(this.withMods("\x7f".repeat(gone)), "input", e);
        }
        refill();                                                     // a pad's character: as a key's delete
        // only a key's delete reaches the pane; a cut or a composition's own delete (an IME) does not
        const del = DELETES[e.inputType];
        const out = del !== "\x7f" ? del : e.inputType === "deleteWordBackward" && this.options.wordDeletes?.() ? WORD : this.backspace();
        return out ? this.type(this.withMods(out), "input", e) : undefined;
      }
      if (touchable && e.inputType !== "insertLineBreak") return typedInField(e);
      const text = e.inputType === "insertLineBreak" ? "\r" : k.value.replaceAll(KEEP, "");
      k.value = "";
      refill();
      this.type(this.withMods(text.replace(/\n/g, "\r")), "input", e);
    });
    k.addEventListener("keyup", (e) => { if (char?.code === e.code) stopChar(); });
    k.addEventListener("blur", stopChar);
    k.addEventListener("compositionstart", (e) => { stopChar(); composingNow = true; composing(e); });
    k.addEventListener("compositionend", (e) => {
      composingNow = false;
      if (touchable) { typedInField(e); refill(); return; }
      this.type(k.value.replaceAll(KEEP, ""), "compose", e); k.value = "";
    });
    kbdTouch({ kbd: k, term: this.term, options: this.options, type: (text, path) => this.type(text, path), refill, touched: () => { touchedAt = performance.now(); } });
    const typing = (on) => {
      this.term.classList.toggle("focused", on);
      this.kbdButton.setAttribute("aria-pressed", String(on));
    };
    k.addEventListener("focus", () => typing(true));
    k.addEventListener("blur", () => typing(false));
    // The button never takes focus itself; iOS opens the keyboard for a focus() made in its click.
    this.kbdButton.addEventListener("mousedown", (e) => e.preventDefault());
    this.kbdButton.addEventListener("click", () => { this.wasTyping = false; document.activeElement === k ? k.blur() : this.focus(); });
    // Terminal text copies with its line ends (the page draws them with CSS, which never copies),
    // without what hides what a line runs (said when there was some).
    document.addEventListener("copy", (e) => {
      const got = selectedText(this.term);
      if (!got) return;                                               // none of the terminal's: the browser's own copy, never an empty clipboard
      const { text, left } = unhidden(got);
      e.clipboardData.setData("text/plain", text);
      e.preventDefault();
      if (left) this.options.status(`Copied${hiddenNote(left)}.`);
    });
    // A hardware keyboard on a phone or a tablet: iOS gives its keys (⌘V too) only to a focused
    // field, and a long press to select takes the focus away; so after a copy from the terminal
    // the keyboard field gets it back when the user was typing before the selection, or a
    // hardware keyboard is known (a modifier pressed alone, which an on-screen keyboard never
    // sends). Without one, that only brings back the keyboard that was up. (A computer keeps its
    // selection.)
    // ⌘C with terminal text selected copies it whatever has the focus: iOS may keep the focus in
    // the keyboard field, whose own (empty) selection it would copy.
    this.hardKbd = this.wasTyping = false;
    const field = (el) => !!el?.closest?.("input, textarea:not(#kbd), select, [contenteditable]");
    k.addEventListener("keydown", (e) => { if (touchable && ["Meta", "Control", "Alt", "Shift"].includes(e.key)) this.hardKbd = true; });
    document.addEventListener("keydown", (e) => {
      if (!e.metaKey || e.ctrlKey || e.altKey || e.key.toLowerCase() !== "c" || !touchable || field(e.target)) return;
      const text = selectedText(this.term);
      if (!text) return;
      e.preventDefault();
      this.copySelection(text);
    }, true);
    // ⌘V with the keys off the terminal's input (after a copy from the history, a click in the
    // list): the input takes them back first, so the paste lands in the pane at its cursor and
    // typing goes on there (Safari pastes only into a field); not under a dialog, nor in Files
    document.addEventListener("keydown", (e) => {
      if (!e.metaKey || e.ctrlKey || e.altKey || e.key.toLowerCase() !== "v" || field(e.target) || document.activeElement === k) return;
      if (document.querySelector("dialog[open]") || this.term.hidden || !this.options.shown?.()) return;
      this.focus();
    }, true);
    document.addEventListener("copy", () => {
      const at = document.activeElement;
      if (at !== k && !field(at)) this.afterCopy(true);
    });
    document.addEventListener("paste", (e) => {
      if (e.target.closest?.("input, textarea:not(#kbd, #pastetext)")) return;   // let fields paste normally
      const image = imageIn(e.clipboardData);
      if (!image && e.target.id === "pastetext") return;               // text goes into the paste field
      e.preventDefault();
      const from = e.target.id === "pastetext" ? this.pasteSid : this.options.sid?.();
      if (e.target.id === "pastetext") this.pasteDialog.close();
      if (image) { if (this.pasteTarget(from)) this.options.pasteImage(image, from); }
      else this.paste(e.clipboardData.getData("text/plain"));
    });
    // With a mouse, a click that does not end a text selection gives the terminal the keyboard; on
    // pointerup, as a program's click (AC-06) cancels its press and with it the browser's mouseup.
    const mouse = matchMedia("(hover: hover) and (pointer: fine)");
    this.term.addEventListener("pointerup", (e) => { if (e.pointerType === "mouse" && mouse.matches && !String(getSelection())) this.focus(); });
    // and moves the cursor there, as a tap does (⌥-click with any pointer); ⌘-click opens a link
    this.term.addEventListener("click", (e) => {
      if (e.button || e.metaKey || e.ctrlKey || e.shiftKey || !(mouse.matches || e.altKey) || String(getSelection()) || inWidget(e)) return;
      this.options.tapMove?.(e.clientX, e.clientY);
    });
    // On a touch screen only a tap on the input (the keyboard field lies over it, placeKbd) or
    // the keyboard button opens the keyboard: a tap elsewhere on the terminal (reading, a link, a
    // full-screen program's click) leaves it as it was; a long press selects text and a moving
    // finger scrolls.
    // While the keyboard is up, iOS selects no page text on a long press (it taps instead; the
    // same on a plain page with a focused field). So a finger held still closes the keyboard and
    // selects the word under it, with iOS's handles; a tap on the selection shows Copy.
    let start = null, hold = 0;
    const release = () => { clearTimeout(hold); hold = 0; };
    // A touch on an output widget (AC-56) is the widget's: its buttons, its own selection.
    this.term.addEventListener("touchstart", (e) => {
      const t = e.touches[0];
      start = e.touches.length === 1 && !inWidget(e) ? { x: t.clientX, y: t.clientY, at: Date.now() } : null;
      release();
      if (start && document.activeElement === this.kbd) {
        this.wasTyping = true;                     // a copy of what gets selected gives the focus back (bind)
        hold = setTimeout(() => { if (start) { const top = selectWordAt(this.kbd, start.x, start.y); if (top !== null) keepSelectionAt(this.term, top); start.held = true; } }, 450);
      }
    }, { passive: true });
    k.addEventListener("blur", () => { if (!start) this.wasTyping = false; });   // closed by the user, not by a selection
    this.term.addEventListener("touchmove", (e) => {
      const t = e.touches[0];
      if (start && Math.hypot(t.clientX - start.x, t.clientY - start.y) > 10) { start = null; release(); }
    }, { passive: true });
    this.term.addEventListener("touchcancel", release, { passive: true });
    this.term.addEventListener("touchend", (e) => {
      release();
      if (start?.held) { e.preventDefault(); start = null; return; }   // its click would drop the word
      const tap = start && Date.now() - start.at < TAP, held = start && Date.now() - start.at >= MENU_AFTER;
      start = null;
      const t = e.changedTouches[0];
      const sel = getSelection();
      // a finger held on a selection in an agent's input box: the page's menu (Cut, Clear, Copy… AC-03)
      if (held && t && sel && !sel.isCollapsed && sel.anchorNode?.parentElement?.closest(".inbox") && inSelection(sel, t.clientX, t.clientY)) { e.preventDefault(); this.options.holdMenu?.(t.clientX, t.clientY); return; }
      if (!tap) return;
      if (t && sel && !sel.isCollapsed && inSelection(sel, t.clientX, t.clientY)) return;   // iOS shows its Copy menu
      // Without this the tap's emulated mousedown that follows moves focus off the keyboard field.
      e.preventDefault();
      if (sel && !sel.isCollapsed) { sel.removeAllRanges(); return; }   // a tap elsewhere clears a selection first
      if (t && this.options.tapLink?.(t.clientX, t.clientY)) return;    // a tap on an address or a path opens it
      if (t) this.options.tapMove?.(t.clientX, t.clientY);              // on the input: the cursor goes there
    });
    const form = this.pasteDialog.querySelector("form");
    form.addEventListener("submit", (e) => {
      e.preventDefault();
      const ta = form.querySelector("textarea");
      if (e.submitter?.value === "send" && this.pasteTarget(this.pasteSid)) this.paste(ta.value);
      ta.value = "";
      this.pasteDialog.close();
      this.focus();
    });
  }

  focus() { this.kbd.focus({ preventScroll: true }); }

  /** One backspace of a hold (repeat: the key's flag; a phone's are told by time): a character,
   *  or after a while a word where the pane takes Ctrl+W as one (a coding agent's input, a
   *  shell's prompt), or "" between two words. */
  backspace(repeat) { return this.held.next(repeat, !!this.options.wordDeletes?.()); }

  paste(text, path = "paste") {
    const clean = text.replace(/\r\n?/g, "\n").replace(/[\x00-\x08\x0b-\x1f\x7f]/g, "").replace(/\n/g, "\r");
    if (!clean) return this.options.status("Nothing to paste: the clipboard holds no text.");   // tmux makes no buffer of nothing
    this.type(this.options.bracketed() ? `\x1b[200~${clean}\x1b[201~` : clean, path);
  }

  // After a copy from the terminal on a touch screen the keyboard field takes the keys back when a
  // hardware keyboard is known or the user was typing before the selection (bind).
  afterCopy(later = false) {
    if (this.keepSelection || !this.touchable || !(this.hardKbd || this.wasTyping)) return;
    this.wasTyping = false;
    if (later) setTimeout(() => this.focus(), 0); else this.focus();
  }

  // Copies the selection, or `given` text (the Copy button's, taken while it was selected). The
  // clipboard API needs HTTPS (or localhost); elsewhere the older copy command copies a selection.
  // keep: the selection stays (Copy beside it, AC-57): the keyboard field does not take the focus.
  // What hides what a line runs is left out; the toast says what was copied.
  async copySelection(given, keep = false) {
    const got = given ?? selectedText(this.term) ?? String(getSelection());
    if (!got) return this.options.status("Select text first, then press Copy.");
    const { text, left } = unhidden(got), said = `Copied ${copyPreview(text)}${hiddenNote(left)}`;
    this.keepSelection = keep;
    try {
      if (window.isSecureContext && navigator.clipboard?.writeText) {
        try {
          await navigator.clipboard.writeText(text);
          this.afterCopy();                        // ⌘V next
          return this.options.status(said);
        } catch { /* fall through */ }
      }
      const ok = given ? copyByCommand(text) : document.execCommand("copy");
      this.options.status(ok ? said : "Copying is blocked here; use the system Copy menu.");
    } finally {
      this.keepSelection = false;
    }
  }

  async pasteFromClipboard() {
    const from = this.options.sid?.();
    const same = () => this.pasteTarget(from);
    // The clipboard API needs HTTPS (or localhost); elsewhere, paste into a field instead. One read
    // for an image or text: Safari asks the user to confirm each read (its own small Paste).
    if (window.isSecureContext && navigator.clipboard?.read) {
      let items;
      try { items = await navigator.clipboard.read(); } catch { /* denied, or no read(): readText or the field */ }
      if (!same()) return;
      if (items) {
        for (const item of items) {
          const type = item.types.find((t) => t === "image/png") ?? item.types.find((t) => t.startsWith("image/"));
          if (type) {
            const image = await item.getType(type);
            return same() && this.options.pasteImage(image, from);
          }
        }
        const item = items.find((i) => i.types.includes("text/plain"));
        const text = item ? await (await item.getType("text/plain")).text() : "";
        return same() && this.paste(text);
      }
    }
    if (window.isSecureContext && navigator.clipboard?.readText) {
      let text;
      try { text = await navigator.clipboard.readText(); } catch { /* denied: use the field */ }
      if (text !== undefined) return same() && this.paste(text);
    }
    if (!same()) return;
    this.pasteSid = from;
    this.pasteDialog.showModal();
    this.pasteDialog.querySelector("textarea").focus();
  }

  /** Clipboard permission and image decoding may finish after the page switched panes. */
  pasteTarget(from) {
    if (!this.options.sid || from === this.options.sid()) return true;
    this.options.status(NOT_PASTED);
    return false;
  }

  renderKeys() {
    const frag = document.createDocumentFragment();
    const rows = this.layout ?? HOTKEYS;
    for (const group of this.fn ? [...rows.slice(0, Math.max(0, rows.length - 2)), ...FN_ROWS] : rows) {
      const g = document.createElement("div");
      g.className = "keyrow";
      for (const [label, action, hint, look] of group) {
        const b = document.createElement("button");
        b.type = "button";
        b.className = look ? `key ${look}` : "key";
        b.textContent = label;
        b.dataset.action = action;
        if (hint) b.title = hint;
        if (action.startsWith("mod:") || action === "fn") b.setAttribute("aria-pressed", String(action === "fn" && !!this.fn));
        g.append(b);
      }
      frag.append(g);
    }
    this.panel.querySelector(".keys").replaceChildren(frag);
    this.syncMods();
  }

  /** The user's layout of the hot keys (hotkeys.js rows), or null for the default (AC-18). */
  setLayout(rows) {
    if (JSON.stringify(rows ?? null) === JSON.stringify(this.layout ?? null)) return;
    this.layout = rows;
    this.fn = false;
    this.renderKeys();
  }

  buildPanel() {
    this.renderKeys();
    // Buttons never take focus from the terminal, so the iOS keyboard stays up.
    this.panel.addEventListener("mousedown", (e) => { if (e.target.closest("button")) e.preventDefault(); });
    // A key held down repeats as a keyboard's does: typed at once, again after HOLD ms, then
    // every REPEAT ms until it is let go (the bridge sends what comes meanwhile together).
    let held = null;
    const stop = () => { if (held) { clearTimeout(held.timer); clearInterval(held.timer); held.done = true; } };
    this.panel.addEventListener("pointerdown", (e) => {
      const b = e.target.closest("button.key");
      if (!b || !REPEATS(b.dataset.action) || e.button > 0) return;
      stop();
      const a = b.dataset.action, live = /^arrow:[AB]$/.test(a) && !this.mods.ctrl && !this.mods.alt;   // ↑ ↓ by the page's lines: each repeat from where the cursor is
      const seq = this.withMods(this.seqOf(a));
      held = { b, done: false };
      this.type(seq, live ? "line" : "hotkey", e);
      held.timer = setTimeout(() => { if (!held.done) held.timer = setInterval(() => this.type(live ? this.seqOf(a, true) : seq, live ? "line" : "hotkey"), REPEAT); }, HOLD);
    });
    for (const end of ["pointerup", "pointercancel", "pointerleave"]) this.panel.addEventListener(end, stop);
    this.panel.addEventListener("click", (e) => {
      const b = e.target.closest("button.key");
      if (!b) return;
      const a = b.dataset.action;
      if (held?.b === b) { held = null; return; }   // typed on pointerdown (and maybe repeated)
      if (a.startsWith("mod:")) { const m = a.slice(4); this.mods[m] = !this.mods[m]; this.syncMods(); return; }
      if (a === "fn") { this.fn = !this.fn; return this.renderKeys(); }
      if (a === "paste") return this.pasteFromClipboard();
      if (a === "upload") return this.options.pickFile();
      if (a === "copy") return this.copySelection();
      this.type(this.withMods(this.seqOf(a)), "hotkey", e);
    });
  }

  seqOf(a, repeat = false) {
    const line = (a === "arrow:A" || a === "arrow:B") && !this.mods.ctrl && !this.mods.alt ? this.options.lineMove?.(a === "arrow:A" ? -1 : 1, repeat) : null;
    return line ?? (a.startsWith("arrow:") ? (this.options.appCursor() ? "\x1bO" : "\x1b[") + a.slice(6) : a);
  }

  syncMods() {
    for (const b of this.panel.querySelectorAll('[data-action^="mod:"]'))
      b.setAttribute("aria-pressed", String(this.mods[b.dataset.action.slice(4)]));
  }
}

/** The first image on a paste event's clipboard, or null. */
function imageIn(data) {
  for (const f of data?.files ?? []) if (f.type.startsWith("image/")) return f;
  for (const it of data?.items ?? []) if (it.kind === "file" && it.type.startsWith("image/")) return it.getAsFile();
  return null;
}

/** A path as a shell word: as it is when nothing in it is special, else in single quotes. */
export function shellWord(path) {
  return /^[\w@%+=:,./-]+$/.test(path) ? path : `'${path.replace(/'/g, "'\\''")}'`;
}

