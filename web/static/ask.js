// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// Ask about a selection (AC-37): Ask (markpick.js) opens a dialog with the selected text and a
// field for the question; Add to Chat asks terminumd for the pane's next quote number (it
// counts up across devices: `askno`) and puts the quote, as a Markdown quote, and the question at
// the input's cursor, a blank line after each, without Enter, the view left where it was. In a
// coding agent's box it is typed (its new lines as Ctrl+J; terminumd types a long one in parts,
// keys.rs), so the agent shows it whole rather than folded into "[Pasted text]", unless its box
// would then be taller than the pane, two rows kept: an agent's box taller than the pane breaks its
// redraw (Claude Code draws over its answer), so a long quote is pasted and folded. The pane is
// measured as the dialog opens: on a phone its field's keyboard then makes the pane shorter. In a shell, where Ctrl+J would run the line, it is pasted. Send to Agent (a
// coding agent's pane only) puts the same and presses Enter.

import { sendOnModEnter } from "./modenter.js";

const $ = (id) => document.getElementById(id);
const ANSWER = 10000;          // ms the dialog waits for the number
const ENTER_AFTER = 200;       // ms: an Enter in the same read as a paste is a new line to Claude Code
const SPARE = 2;               // rows of the pane a typed quote leaves above the agent's box and its footer
const BOX = 6;                 // rows an agent's box and its footer take when the view has not found the box

/** What Add to Chat puts: a Markdown quote numbered on its first line ("> Quote #0007"), the
 *  selection's lines under it (blank lines, line ends' spaces, tabs and control characters left
 *  out: each would be a key when typed), a blank line, the question. */
export function askText(n, quote, question) {
  const lines = quote.replace(/\t/g, "    ").replace(/[\x00-\x08\x0b-\x1f\x7f]/g, "").split("\n").map((l) => l.trimEnd()).filter(Boolean);
  // a line ending in an @-word ends in a space: the file list the agent opened for it closes before the new line
  const body = lines.map((l) => `> ${l}`.replace(/(^|\s)@\S*$/, "$& "));
  return [`> Quote #${String(n).padStart(4, "0")}`, ...body, "", question.trim()].join("\n");
}

/** Rows `text` takes in an agent's box `cols` wide (its border and prompt take 4). */
export const boxRows = (text, cols) => text.split("\n").reduce((n, l) => n + Math.max(1, Math.ceil(l.length / Math.max(1, cols - 4))), 0);

/** Whether `text` typed fits a pane `cols` × `rows` whose agent's box (prompt, rules and footer)
 *  takes `used` rows now (0: not found, BOX counted); not measured (undefined): no. */
export const fitsTyped = (text, { cols, rows, used } = {}) => !!cols && !!rows && boxRows(text, cols) + (used || BOX) + SPARE <= rows;

/** Whether the text just before a quote's occurrence (white space as one space) is askText's
 *  opening, or one of before 0.4.9 (`Quote #N: «`, before 0.4.7 `Quote Start ---- «`): that
 *  occurrence is the copy sent to the agent, never what was asked about. */
export const sentCopy = (before) => /((Quote #\d+:|Quote Start -+) ?«|Quote #\d+ ?>) ?$/.test(before);

/** What goes to the pane for `text`, in order: [kind, data], kind "keys" (typed), "paste" or
 *  "enter" (typed a moment after); `now`: sent to the agent (Send to Agent); `typed`: an agent's
 *  box takes it as keys (it fits: fitsTyped). */
export function askSends(text, { agent, now = false, typed = false }) {
  if (agent && typed) return now ? [["keys", text], ["enter", "\r"]] : [["keys", `${text}\n\n`]];
  if (now) return [["paste", text], ["enter", "\r"]];
  return agent ? [["paste", text], ["keys", "\n\n"]] : [["paste", `${text}\n\n`]];
}


/** send(msg) to terminumd; toChat(text, stay): pasted into the shown pane's input; keys(data, stay): typed
 *  there (stay: Add to Chat's, the view left where it was); sid(): the shown pane; agent(): it runs a
 *  coding agent; pane(): its {cols, rows, used (the rows its agent's box and footer take, 0 unknown)};
 *  asked(n, quote, line): one was asked about (asked.js keeps it
 *  marked); toast(text): says what was not done; closed(): the dialog closed, however (Add to Chat,
 *  Send to Agent, Cancel, Esc): the keys go back to the terminal, as the browser gives them back to
 *  what had them before it opened, the page's body after a Shift selection (sizing.js).
 *  {open(text, line), seen(m), sync()}. */
export function askBox({ send, toChat, keys, sid, agent, pane, asked, toast, closed }) {
  const box = $("askbox"), q = $("askq"), add = box.querySelector(".primary"), now = $("asksend");
  let quote = "", line = null, waiting = null, size;
  const fail = (text) => { $("askerr").textContent = text; $("askerr").hidden = !text; };
  const ready = (on) => { add.disabled = now.disabled = !on; };
  q.addEventListener("input", () => ready(q.value.trim()));
  sendOnModEnter(box);                                             // ⌘Enter: Add to Chat
  // Esc leaves the focus in the closed dialog's field a moment (the terminal would not take it from a field)
  box.addEventListener("close", () => { waiting?.(null); if (box.contains(document.activeElement)) document.activeElement.blur(); closed?.(); });
  box.querySelector("form").addEventListener("submit", async (e) => {
    const how = e.submitter?.value;
    if (how !== "add" && how !== "send") return;                    // Cancel closes it
    e.preventDefault();
    if (!q.value.trim() || waiting) return;
    if (how === "send" && !agent()) return sync();                 // the agent ended since it opened
    ready(false);
    fail("");
    const at = sid();
    const m = await new Promise((resolve) => {
      const timer = setTimeout(() => waiting?.({ error: "The Mac did not answer: is the connection back?" }), ANSWER);
      waiting = (got) => { clearTimeout(timer); waiting = null; resolve(got); };
      send({ t: "askno" });
    });
    ready(q.value.trim());
    if (!m || !box.open) return;                                   // closed meanwhile
    if (m.error || !Number.isInteger(m.n)) return fail(m.error ?? "No number came back.");
    if (sid() !== at) return fail("The pane shown changed: nothing was put in.");
    if (how === "send" && !agent()) return sync();
    box.close();
    asked?.(m.n, quote, line);                                      // the quote stays marked where it was selected
    const stay = how === "add";                                   // Send to Agent goes to the input: the answer comes there
    const text = askText(m.n, quote, q.value), typed = fitsTyped(text, size);
    for (const [kind, data] of askSends(text, { agent: agent(), now: !stay, typed })) {
      if (kind !== "enter") (kind === "keys" ? keys : toChat)(data, stay);
      // the Enter only to the same pane, still in its agent: another pane's line or a shell would run it
      else setTimeout(() => {
        if (sid() === at && agent()) return keys(data);
        toast?.(sid() === at ? "Send to Agent: the agent ended, so Enter was not pressed." : "Send to Agent: the pane shown changed, so Enter was not pressed there.");
      }, ENTER_AFTER);
    }
  });
  /** Send to Agent only while the shown pane runs a coding agent: a shell would run the quote. */
  function sync() {
    const was = !now.hidden;
    now.hidden = !agent();
    if (was && now.hidden && box.open) fail("The coding agent ended: Send to Agent is off, Add to Chat puts the quote in.");
  }
  return {
    open(text, at = null) {
      quote = text;
      line = at;
      $("asktext").textContent = text;
      q.value = "";
      ready(false);
      fail("");
      sync();
      size = pane();                                                // before its field brings a phone's keyboard up
      box.showModal();
      q.focus();
    },
    /** Every message from terminumd (app.js dropped another pane's). */
    seen(m) { if (m.t === "askno") waiting?.(m); },
    /** The list changed (a layout): the shown pane may have left its agent. */
    sync() { if (box.open) sync(); },
  };
}
