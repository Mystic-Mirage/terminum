// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// A session's last lines (AC-96): its row menu's "Show Last 300 Lines" asks terminumd for what it
// kept of the pane, and a large window shows them as plain text (never rendered: terminal output
// is text), scrolled to the end: those from before its tmux restarted, when there are, else the
// latest; "Before restart | Now" switches when both are. Copy copies the lines shown; Esc or × closes.
// The same window shows a pane's diagnostics (AC-100, diag.js).
import { when } from "./marks.js";

const $ = (id) => document.getElementById(id);

/** What the window says above the lines of `kept` (a file's: session, window, dir, at). */
export function metaOf(which, kept) {
  if (!kept) return "";
  return [which === "before" ? "Before the restart" : "Kept", kept.session, kept.window, kept.dir, kept.at ? when(kept.at) : ""].filter(Boolean).join(" · ");
}

/** send(m); copy(text); title(id): the row's title now. */
export function lastLinesView({ send, copy, title }) {
  const box = $("lastlines"), seg = $("lastlinesseg"), text = $("lastlinestext");
  let asked = null, got = null, which = "before";
  $("lastlinesclose").onclick = () => box.close();
  $("lastlinescopy").onclick = () => { if (text.textContent) copy(text.textContent); };
  for (const b of seg.querySelectorAll("button")) b.onclick = () => { which = b.dataset.which; draw(); };

  function draw() {
    const kept = got?.[which];
    for (const b of seg.querySelectorAll("button")) b.setAttribute("aria-selected", String(b.dataset.which === which));
    $("lastlinesmeta").textContent = got?.error ?? kept?.error ?? metaOf(which, kept);
    $("lastlinesmeta").classList.toggle("bad", !!(got?.error ?? kept?.error));
    text.textContent = kept?.text ?? "";
    text.hidden = !kept?.text;
    $("lastlinesnone").hidden = !got || !!got.error || !!kept;
    $("lastlinescopy").disabled = !kept?.text;
    text.scrollTop = text.scrollHeight;
  }

  return {
    /** Another text in the same window (a pane's diagnostics, AC-100): its heading, the line above
     *  it, the text; Copy copies it on its own press (Safari copies only within one). */
    showText(heading, meta, body) {
      asked = null; got = null;
      $("lastlinestitle").textContent = heading;
      $("lastlinesmeta").textContent = meta;
      $("lastlinesmeta").classList.remove("bad");
      seg.hidden = true; $("lastlinesnone").hidden = true;
      text.textContent = body; text.hidden = false;
      $("lastlinescopy").disabled = false;
      if (!box.open) box.showModal();
      text.scrollTop = 0;
    },
    /** The row menu's item: asks for `id`'s lines and opens the window at once ("Reading…"). */
    open(id, lines) {
      asked = id; got = null;
      $("lastlinestitle").textContent = `Last ${lines} lines · ${title(id) ?? ""}`;
      $("lastlinesmeta").textContent = "Reading…";
      $("lastlinesmeta").classList.remove("bad");
      seg.hidden = true;
      text.textContent = ""; text.hidden = true;
      $("lastlinesnone").hidden = true;
      $("lastlinescopy").disabled = true;
      if (!box.open) box.showModal();
      send({ t: "lastlines", id });
    },
    /** terminumd's answer: {id, lines, before, latest} or {id, error}. */
    seen(m) {
      if (m.t !== "lastlines" || m.id !== asked || !box.open) return;
      got = m;
      which = m.before ? "before" : "latest";
      seg.hidden = !(m.before && m.latest);
      draw();
    },
  };
}
