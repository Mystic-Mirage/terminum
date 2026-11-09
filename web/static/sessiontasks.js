// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The tasks made in the session shown (terminum-kanban.md AC-150): by its agent's create_task or of its records
// (AC-116). Their count stands in a round button under the sent history's, in cornflower blue, while there is one;
// its pane lists them by title, and a press opens the board's own task window here, RUN's with it. Tasks this
// browser was not told of yet are said at the terminal's top when the session is shown, as reminders are.
import { fabPane } from "./fabpane.js";
import { plural, read, write } from "./listparts.js";
import { $, api, say, statusBox } from "./kanbanutil.js";
import { taskWindow } from "./kanbanwin.js";
import { runWindow } from "./kanbanrun.js";
import { newTaskUrl, taskUrl } from "./tasklink.js";
import { recordTasks } from "./converttask.js";

const SEEN = "tm.sessiontasks.seen";     // the keys of tasks this browser was told of: a new ID never tells again
const SEEN_MOST = 2000;
const TITLE_MOST = 80;
const SAY_FOR = 5000;

/** sid(): the session shown; name(key): its title; toast(text, ms); placed(): the floating buttons placed again;
 *  panes(), projects(): the list's sessions and projects, for Convert to task. */
export function sessionTasks({ sid, name, toast, placed, panes, projects }) {
  const fab = $("tasksfab");
  let list = [], of = null, asked = 0, timer = 0, windowed = null, meta = null, failed = "";
  const records = recordTasks({ sid, name, panes, projects, made: (t) => { seenNow([t]); toast(`${t.id} is in TODO: ${t.title.slice(0, TITLE_MOST)}`, SAY_FOR); load(); } });
  const pane = fabPane({ fab, pane: $("taskspane"), close: $("tpclose"), onToggle: (on) => { if (on) draw(); } });

  async function load() {
    const key = sid(), mine = ++asked;
    if (!key) { list = []; of = null; return shown(); }
    try {
      const page = await api(`/api/tasks?made_in=${encodeURIComponent(key)}&subtasks=1&limit=200`);
      if (mine !== asked) return;                  // another session shown meanwhile
      list = page.tasks ?? [];
      of = key;
      failed = "";
    } catch (e) {
      if (mine !== asked) return;
      list = []; of = key;
      failed = e.status === 401 ? "" : e.message;   // said on the button, where the count would be
    }
    shown();
  }

  function shown() {
    const n = list.length;
    fab.toggleAttribute("data-off", !n && !failed);
    $("tasksfabn").textContent = failed ? "!" : n ? String(n) : "";
    fab.title = failed ? `Tasks of this session: ${failed}` : `Tasks made in this session: ${n}`;
    fab.setAttribute("aria-label", fab.title);
    if (!n && !failed && pane.open()) pane.toggle(false);
    if (pane.open()) draw();
    tell();
    placed();
  }

  /** Those of `tasks` this browser was not told of, now told. */
  function seenNow(tasks) {
    const seen = new Set(read(SEEN) ?? []);
    const fresh = tasks.filter((t) => !seen.has(t.uid));
    for (const t of fresh) seen.add(t.uid);
    if (fresh.length) write(SEEN, [...seen].slice(-SEEN_MOST));
    return fresh;
  }

  /** "<session>: 3 tasks made here, 1 new: TERM-7 …" once for each task in this browser. */
  function tell() {
    const fresh = seenNow(list);
    if (!fresh.length || !of) return;
    const first = fresh[0];
    toast(`${name(of) ?? "This session"}: ${plural(list.length, "task")} made here, ${fresh.length} new: ${first.id} ${first.title.slice(0, TITLE_MOST)}`, SAY_FOR);
  }

  function draw() {
    const ul = $("tplist");
    if (failed) {
      const why = document.createElement("li");
      why.className = "tpwhy";
      why.textContent = `Not read: ${failed}`;
      ul.replaceChildren(why);
      return pane.place();
    }
    ul.replaceChildren(...list.map((t) => {
      const li = document.createElement("li"), b = document.createElement("button");
      b.type = "button";
      b.className = "tpitem";
      const title = document.createElement("span");
      title.className = "tptitle";
      title.textContent = t.title.length > TITLE_MOST ? `${t.title.slice(0, TITLE_MOST - 1)}…` : t.title;
      b.append(statusBox(t), title);
      b.title = `${t.id} ${t.title}`;
      b.onclick = () => { pane.toggle(false); open(t.id); };
      li.append(b);
      return li;
    }));
    pane.place();
  }

  /** The board's task window, made the first time it is asked for. */
  async function open(id) {
    try {
      meta = await api("/api/tasks/meta");
      if (!windowed) {
        const runs = runWindow({ addAgent: () => { location.href = "/kanban/"; }, noFolder: (_, words) => say(words), done: () => windowed.refresh() });
        windowed = taskWindow({ getMeta: () => meta, runs, openTask: open,
          newSubtask: (t) => { location.href = newTaskUrl("", t.project, t.id); } });
      }
      await windowed.open(id);
    } catch (e) { say(`${e.message} (${taskUrl(id)})`); }
  }

  return {
    /** Another session shown. */
    changed() { if (sid() !== of) load(); },
    /** A task changed somewhere: the list read again once a burst ends; an open window reads its task. */
    heard(m) {
      clearTimeout(timer);
      if (!m.history) timer = setTimeout(load, 300);
      if (windowed && (m.id === "" || windowed.shows(m.id))) windowed.refresh(false, true);
    },
    place: () => pane.place(),
    open,
    /** Convert to task (AC-116): kept, it is said, and counted where it was made. */
    convert: (item) => records.open(item),
  };
}
