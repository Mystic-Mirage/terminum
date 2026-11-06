// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The Plan pane (AC-97, AC-98): the shown session's plan and how many records its agent kept for
// the user (its bookmarks and reminders, AC-98; a press opens them), modal as the
// reminders' pane (fabpane.js). Opened by the header's Plan button (#curplan), the progress circle
// in the floating column (#planfab) or a row's percent (a "planopen" event after the row's session is shown). It asks terminumd {"t":"plan"} on
// opening and again on {"t":"plannew"} of its key; it closes when another session is shown. Every
// text the agent wrote goes in as text (el's textContent): never HTML or Markdown.
import { fabPane } from "./fabpane.js";
import { icon } from "./icons.js";
import { el } from "./marks.js";
import { ageText, clock, etaLine, pctText, rowTitle, sourceText, statusIcon } from "./plans.js";

const $ = (id) => document.getElementById(id);
const now = () => Date.now() / 1000;
export const EMPTY = "No plan: the agent has not sent one for this session.";
const STATUS = { completed: "completed", in_progress: "in progress", pending: "pending", blocked: "blocked" };

/** send(msg) to terminumd; sid(): the shown pane's key; tasks(): its open tasks' IDs. */
/** The bottom of the banners shown (banners.js), else of the header (#bar): a phone's sheet starts there. */
const bannersEnd = () => Math.max($("bar").getBoundingClientRect().bottom,
  ...[...document.querySelectorAll(".banner")].filter((b) => !b.hidden && b.offsetParent).map((b) => b.getBoundingClientRect().bottom));

export function planPane({ send, sid, tasks }) {
  const fab = $("curplan"), ring = $("planfab");
  let of = null, shown = null, waiting = false;   // the key the pane shows; the shown row (layout); asked, no answer yet
  const pane = fabPane({ fab, pane: $("planpane"), close: $("planclose"), onToggle: (on) => {
    of = on ? sid() : null;
    ring.setAttribute("aria-expanded", String(on));
    if (on) { $("planbody").replaceChildren(el("p", "plmeta", "Loading…")); ask(); place(); }
  }, onPlace: () => {   // a phone's sheet: its top at the header's or a banner's bottom
    if ("sheet" in $("planpane").dataset) $("planpane").style.setProperty("--plan-top", `${Math.round(bannersEnd())}px`);
  } });
  ring.addEventListener("click", () => pane.toggle());
  ring.addEventListener("mousedown", (e) => e.preventDefault());   // the keys stay put until the pane opens
  const ask = () => { if (of) { waiting = true; send({ t: "plan", key: of }); } };
  document.addEventListener("planopen", () => { if (!pane.open()) pane.toggle(true); else ask(); });

  /** The header's button and the column's progress circle for the shown row (item: a layout row or null). */
  function header(item) {
    shown = item;
    const plan = item?.plan ?? null, notes = item ? tasks().length : 0;
    $("planmenu").hidden = !item;                    // the header ⋯ menu's entry, for any shown session
    if (pane.open() && sid() !== of) pane.toggle(false);
    else if (pane.open() && waiting) ask();          // asked while terminumd was away (a restart): its next layout asks again
    fab.hidden = !plan && !notes && !item?.agent;    // tasks alone, or a coding agent without a plan yet, still open the pane
    const said = notes ? `${notes} task${notes === 1 ? "" : "s"}` : "";
    fab.title = plan ? `Plan: ${rowTitle(plan, now())}${said && ` · ${said}`}` : said ? `Kept: ${said}` : "Plan: none yet";
    fab.setAttribute("aria-label", fab.title);
    const pct = plan ? Math.round(Math.max(0, Math.min(100, plan.pct ?? 0))) : 0;
    $("curplanp").textContent = pct ? `${pct}%` : "";   // 0%: nothing, the icon alone
    fab.classList.toggle("pct", pct > 0);                // its width for "100%" (style.css)
    progress(pct ? plan : null, fab.title);
  }

  /** The column's circle: the percent in its ring while a plan has some done (marks.js placeFab shows and places it). */
  function progress(plan, title) {
    const had = "pct" in ring.dataset;
    if (!plan) delete ring.dataset.pct;
    else {
      const pct = Math.round(Math.max(0, Math.min(100, plan.pct ?? 0)));
      ring.dataset.pct = String(pct);
      ring.style.setProperty("--p", String(pct));
      $("planfabt").textContent = pct === 100 ? "✓" : pctText(plan);   // "100%" outgrows the ring
      ring.classList.toggle("stale", !!plan.stale);
      ring.title = title;
      ring.setAttribute("aria-label", title);
    }
    if (had !== !!plan) document.dispatchEvent(new Event("planfab"));   // shown or gone: the column placed again
  }
  setInterval(() => { if (shown) header(shown); }, 60_000);   // the age moves on without a message
  let last = null;                                   // the plan drawn: drawn again when the agent's records change
  document.addEventListener("marks", () => { if (shown) header(shown); if (pane.open() && last) draw(...last); });   // its agent's records changed (marks.js)

  function onMessage(m) {
    if (!pane.open() || m.key !== of) return;
    if (m.t === "plannew") return void ask();
    waiting = false;
    last = [m.plan, m.hooks];
    draw(m.plan, m.hooks);
  }

  // a section's small caps label (UI-GUIDELINES §2: one label style for every section)
  const section = (text) => el("h4", "plsec", text);

  function draw(p, hooks) {
    const body = [];
    if (hooks === false) body.push(el("p", "plmeta plhooks", "terminum's hooks are not running in this session (Codex: trust them in /hooks)."));
    if (!p) body.push(el("p", "plmeta plempty", EMPTY));
    else {
      if (p.finished) body.push(el("p", "plmeta plfinished", `Finished session (agent exited ${clock(p.finished_at ?? p.updated)})`));
      if (p.title) body.push(el("h3", "pltitle", p.title));
      if (p.doing) {                                  // what it does now, set apart
        const now = el("div", "plnow");
        now.append(el("span", "plnowlabel", "Now"), el("p", "pldoing", p.doing));
        body.push(now);
      }
      // the progress: done of all, the percent, a bar, the agent's ETA under it
      const pct = Math.round(Math.max(0, Math.min(100, p.pct ?? 0)));
      const prog = el("div", "plprog"), head = el("div", "plproghead");
      head.append(el("span", "plcount", `${p.done} of ${p.total} done`), el("span", "plpctv", `${pct}%`));
      const bar = el("div", "plbar"), fill = el("span", "plbarfill");
      fill.style.width = `${pct}%`;
      bar.setAttribute("role", "progressbar");
      bar.setAttribute("aria-valuemin", "0"); bar.setAttribute("aria-valuemax", "100"); bar.setAttribute("aria-valuenow", String(pct));
      bar.setAttribute("aria-label", `${p.done} of ${p.total} done`);
      bar.append(fill);
      prog.append(head, bar);
      if (etaLine(p)) prog.append(el("p", "plmeta pleta", `ETA ${etaLine(p)}`));
      body.push(prog);
      if ((p.items ?? []).length) {
        body.push(section("Steps"));
        const ul = el("ul", "plitems");
        for (const it of p.items) {
          const li = el("li", `plitem ${it.status ?? "pending"}`);
          const mark = el("span", "plmark", statusIcon(it.status));
          mark.title = STATUS[it.status] ?? "pending";
          mark.setAttribute("aria-label", mark.title);
          li.append(mark, el("span", "pltext", it.text));
          ul.append(li);
        }
        body.push(ul);
      }
      if (p.partial) body.push(el("p", "plpartial", "partial: some updates were missed"));
      const foot = el("div", "plfoot");
      foot.append(el("p", "plmeta", `Updated ${ageText(p.updated, now())} (${clock(p.updated)})`), el("p", "plmeta", sourceText(p.source)));
      body.push(foot);
    }
    // this session's open tasks, each opening its window on the board (terminum-kanban.md AC-113)
    const open = tasks();
    body.push(section("Tasks"));
    const kept = el("p", "plmeta plkept", open.length ? "" : "No task in this session.");
    for (const id of open) {
      const a = el("a", "linkish", id);
      a.href = `/kanban/${id}`;
      kept.append(a, " ");
    }
    body.push(kept);
    const held = $("planpane").contains(document.activeElement);   // a pressed Done goes with the redraw: the pane keeps the keys (Esc)
    $("planbody").replaceChildren(...body);
    if (held && !$("planpane").contains(document.activeElement)) $("planpane").focus({ preventScroll: true });
    place();
  }

  $("planclose").replaceChildren(icon("x", 16));
  return { header, onMessage, place };
  /** Placed by fabpane: a window under the banners shown, or a sheet. */
  function place() {
    if (pane.open()) pane.place();
  }
}
