// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// Tip of the day (AC-101): one tip at the session list's foot on a computer (foot.css), the next
// one each new day (a tab left open too, when looked at again); Next shows the next at once, ×
// hides it until tomorrow (Settings' "Show today's tip" brings it back), Turn off turns off
// Settings' "Tip of the day" (on for a new browser). All kept in this browser.
import { icon } from "./icons.js";

const $ = (id) => document.getElementById(id);

// {ring}: the plan's round button; {split-…}: the row menu's split drawings (icons.js)
export const TIPS = [
  "Run your coding agent in its normal TUI, not fullscreen: terminum is faster then and draws tables right in the output.",
  "Select text to bookmark it or set a reminder: come back to it later instead of losing it in the chat.",
  "Click the colored or white marks on the terminal's right edge to jump to bookmarks and to your own messages.",
  "terminum installs its hooks for coding agents, on this computer and on remote hosts. Codex runs them once you trust them in its /hooks.",
  "Files follows the terminal's working folder, on this computer and on a remote host.",
  "The file viewer renders PDFs and tables, and plays audio, video and MIDI.",
  "Paste or drop images and files on a terminal, a remote one too: terminum puts them on the pane's host.",
  "Bring your own tmux sessions into terminum with Attach tmux sessions in the list's tools.",
  "Missing a feature? Add it to terminum's repository with a pull request.",
  "Group sessions into projects, and drag sessions and projects with the mouse to order them.",
  "Reach the terminum on your computer from anywhere, privately, through Tailscale: see terminum's README.",
  "A coding agent shows its plan and progress on long tasks: open it with the round button {ring} at the top of the terminal.",
  "Open a shell beside a session: right-click it and pick a split {split-right} {split-bottom}, or press ⌘D (right) or ⌘⇧D (below).",
  "Settings (⋯ at the top right) hold the text and Markdown sizes, history lines and switches; Colors there changes the color scheme. Themes in ~/.terminum/themes set one per host or session.",
  "Claude Code and Codex can work through the bookmarks and reminders they made, like a task list: just ask them.",
];

const today = () => new Date().toLocaleDateString("sv");    // YYYY-MM-DD, this browser's day

/** A tip's text with its {names} drawn as icons (a name is at every odd place of the split). */
function tipNodes(text) {
  return text.split(/\{([a-z-]+)\}/).map((part, i) => {
    if (!(i % 2)) return document.createTextNode(part);
    if (part !== "ring") return icon(part, 13);
    const ring = document.createElement("span");
    ring.className = "tipring";
    ring.setAttribute("aria-hidden", "true");
    return ring;
  });
}

/** store: the page's {get, set} (localStorage). */
export function tipOfDay({ store }) {
  const box = $("tip"), on = $("tipson");
  let tip = store.get("tip", null);                         // {day, at, closed}: today's tip, the day × was pressed
  if (!Number.isInteger(tip?.at) || tip.at < 0 || tip.at >= TIPS.length) tip = { day: "", at: -1, closed: "" };
  on.checked = store.get("tips", true);

  function draw() {
    box.hidden = !on.checked || tip.closed === tip.day;
    $("tipshow").hidden = !on.checked || !box.hidden;          // Settings' way back to a tip closed today
    if (!box.hidden) $("tiptext").replaceChildren(...tipNodes(TIPS[tip.at]));
  }
  const keep = (change) => { tip = { ...tip, ...change }; store.set("tip", tip); draw(); };
  /** A new day (and a first one): the next tip, × forgotten; at load, and when a tab left open is looked at again. */
  function roll() {
    const day = today();
    if (tip.day !== day) keep({ day, at: (tip.at + 1) % TIPS.length, closed: "" });
  }
  $("tipnext").onclick = () => keep({ at: (tip.at + 1) % TIPS.length });
  $("tipclose").onclick = () => keep({ closed: tip.day });
  $("tipoff").onclick = () => { on.checked = false; on.onchange(); };
  on.onchange = () => { store.set("tips", on.checked); keep({ closed: "" }); };
  document.addEventListener("tipshow", () => keep({ closed: "" }));
  document.addEventListener("visibilitychange", () => { if (!document.hidden) roll(); });
  window.addEventListener("focus", roll);
  roll();
  draw();
}
