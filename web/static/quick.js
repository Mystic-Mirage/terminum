// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The shown pane's quick shell (AC-88): a tmux window of its own (terminumd marks it with the
// pane), shown below the terminal, on its right or on its left by the page itself in embed mode
// (`/?embed=<key>`: its own connection, stream, size and keys). It shows whenever the shown
// pane has one, on a computer; its share of the view is dragged at its edge and remembered per
// side in this browser; ⇄ moves it, × closes it (asked first when a program runs in it). ⌘D
// splits the view as iTerm2 does, a terminal on the right; ⌘⇧D and the button under the
// magnifier, one below.
import { frameTo } from "./listparts.js";

const $ = (id) => document.getElementById(id);
const DEFAULT = 1 / 3, LEAST = 0.12, MOST = 0.8;

/** current(): the shown pane's {group, item}; item(id) finds a row; pick(id) shows a pane. */
export function quickShell({ send, store, current, item, pick, askConfirm, narrow }) {
  let key = null, side = "bottom";
  let asked = null, focusKey = null;     // the pane whose quick shell was asked for; the one to focus once loaded
  let loaded = false;                    // the frame shows `key`
  const row = $("termrow"), frame = $("quickframe");

  function setShare(f, save) {
    const v = Math.min(MOST, Math.max(LEAST, f));
    row.style.setProperty("--quick", `${(v * 100).toFixed(1)}%`);
    $("quickgrip").setAttribute("aria-valuenow", String(Math.round(v * 100)));
    if (save) store.set(`quickShare.${side}`, v);
  }

  /** The shown pane or the layout changed: its quick shell shows, or none. */
  function sync() {
    const it = current()?.item;
    const want = (!narrow.matches && it?.quick) || null;
    $("quickbtn").setAttribute("aria-pressed", String(!!it?.quick));
    // the button under the magnifier: a split on a computer; on a phone the quick shell alone, and back from it
    const label = !narrow.matches ? "Split: a terminal below (⌘⇧D; on the right: ⌘D)" : it?.quickOf ? "Back to its pane" : "Quick shell, in this pane's folder";
    $("splitfab").title = label;
    $("splitfab").setAttribute("aria-label", label);
    side = ["left", "right"].includes(it?.quickSide) ? it.quickSide : "bottom";
    row.classList.toggle("left", !!want && side === "left");
    row.classList.toggle("right", !!want && side === "right");
    $("quick").hidden = $("quickgrip").hidden = !want;
    $("quickgrip").setAttribute("aria-orientation", side === "bottom" ? "horizontal" : "vertical");
    if (want) {
      setShare(store.get(`quickShare.${side}`, DEFAULT), false);
      const q = item(want);
      $("quick").querySelector(".qtitle").textContent = ["Quick shell", ...(q?.sub ?? []).filter((s) => s !== "quick shell")].join(" · ");
    }
    if (it?.id !== asked) asked = null;    // another pane shown: an answer for that one moves no keys
    if (want !== focusKey) focusKey = null;
    if (want !== key) {
      key = want;
      loaded = false;
      frameTo(frame, key ? `/?embed=${encodeURIComponent(key)}` : "about:blank");
    }
  }

  function focus() {
    if (!key) return;
    frame.contentWindow?.focus();
    frame.contentWindow?.postMessage({ type: "tm-focus" }, location.origin);
  }
  frame.addEventListener("load", () => {
    loaded = true;
    if (focusKey && focusKey === key) { focusKey = null; focus(); }
  });

  /** The header's button, the menu's items: the pane's quick shell, made when it has none. */
  function open(where = "bottom") {
    const it = current()?.item;
    if (!it) return;
    if (narrow.matches && it.quickOf) return pick(it.quickOf);      // a phone shows a quick shell alone: back to its pane
    if (it.quick && !narrow.matches) {
      if (where !== (it.quickSide ?? "bottom")) send({ t: "quickside", id: it.quick, side: where });
      return focus();
    }
    asked = it.id;
    send({ t: "quick", id: it.id, side: where });
  }

  /** terminumd made (or found) it: a computer shows it with its pane, a phone shows it alone. */
  function made(m) {
    if (narrow.matches) return pick(m.id);
    const mine = asked !== null && asked === (current()?.item.id ?? null);
    asked = null;
    sync();
    if (!mine || key !== m.id) return;
    if (loaded) focus();
    else focusKey = m.id;
  }

  $("quickbtn").onclick = () => open("bottom");
  $("splitfab").onclick = () => open("bottom");
  $("splitfab").addEventListener("mousedown", (e) => e.preventDefault());   // the terminal keeps the keys until the split has them
  // ⌘D: a terminal on the right, ⌘⇧D: one below (iTerm2's splits); before the browser's bookmark
  if (!embedded) document.addEventListener("keydown", (e) => {
    if (!e.metaKey || e.ctrlKey || e.altKey || e.code !== "KeyD" || narrow.matches || !current()) return;
    e.preventDefault();
    e.stopPropagation();
    if (!e.repeat) open(e.shiftKey ? "bottom" : "right");
  }, true);
  $("quickflip").onclick = () => key && send({ t: "quickside", id: key, side: side === "left" ? "bottom" : "left" });
  $("quickclose").onclick = async () => {
    const q = key && item(key);
    if (!q) return;
    const prog = q.sub.find((s) => s !== "quick shell" && !/^[~/]/.test(s));
    if (!q.shell && !(await askConfirm(`Close the quick shell? The program running in it (${prog || "a program"}) ends.`, "Close"))) return;
    send({ t: "kill", id: q.id });
  };

  // its edge: dragged (the terminals resize once a second at most, terminumd waits for the end)
  const grip = $("quickgrip");
  const shareAt = (e) => {
    const r = row.getBoundingClientRect();
    return side === "left" ? (e.clientX - r.left) / r.width : side === "right" ? (r.right - e.clientX) / r.width : (r.bottom - e.clientY) / r.height;
  };
  grip.addEventListener("pointerdown", (e) => {
    e.preventDefault();
    grip.setPointerCapture(e.pointerId);
    row.classList.add("dragging");
    const move = (ev) => setShare(shareAt(ev), false);
    const end = (ev) => {
      if (ev.type === "pointerup") setShare(shareAt(ev), true);
      row.classList.remove("dragging");
      for (const [t, f] of [["pointermove", move], ["pointerup", end], ["pointercancel", end]]) grip.removeEventListener(t, f);
    };
    grip.addEventListener("pointermove", move);
    grip.addEventListener("pointerup", end);
    grip.addEventListener("pointercancel", end);
  });
  grip.addEventListener("keydown", (e) => {
    const grow = side === "left" ? { ArrowRight: 1, ArrowLeft: -1 } : side === "right" ? { ArrowLeft: 1, ArrowRight: -1 } : { ArrowUp: 1, ArrowDown: -1 };
    if (!grow[e.key]) return;
    e.preventDefault();
    setShare(store.get(`quickShare.${side}`, DEFAULT) + grow[e.key] * 0.05, true);
  });

  /** Whether this page is looked at (the embedded page streams while its pane's page does). */
  function active(on) { if (key) frame.contentWindow?.postMessage({ type: "tm-active", on }, location.origin); }

  return { sync, open, made, focus, active };
}

/** The page in embed mode: the pane key it shows, or null for the page itself. */
export const embedded = new URLSearchParams(location.search).get("embed");
