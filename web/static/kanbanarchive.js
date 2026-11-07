// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// A project's Archived (terminum-kanban.md AC-118): its archived tasks newest first, a page at a time,
// with Find; a row opens the task, Return puts it back in the column it had.
import { $, api, button, el, say } from "./kanbanutil.js";

const PAGE = 200;
const day = (at) => new Date(at * 1000).toLocaleDateString(undefined, { dateStyle: "medium" });

export function archivedWindow({ openTask }) {
  const box = $("archivedbox"), list = $("archivedlist"), find = $("archivedfind"), more = $("archivedmore");
  let project = null, have = 0, asked = 0;

  function row(t) {
    const li = el("li");
    const back = button(null, "Return", async () => {
      try {
        await api(`/api/tasks/${t.id}/archive`, { method: "POST", body: { on: false } });
        read();
      } catch (e) { say(e.message); }
    });
    li.append(button("linklike", `${t.id} ${t.title}`, () => openTask(t.id)), el("small", null, `${t.status} · ${day(t.archived_at)}`), back);
    return li;
  }

  /** The newest asked is the one that shows; `after`: the next page under what stands. */
  async function read(after = 0) {
    const mine = ++asked, q = new URLSearchParams({ project: project.id, archived: "1", subtasks: "1", limit: String(PAGE), after: String(after) });
    if (find.value.trim()) q.set("q", find.value.trim());
    try {
      const { tasks } = await api(`/api/tasks?${q}`);
      if (mine !== asked) return;
      if (after) list.append(...tasks.map(row)); else list.replaceChildren(...tasks.map(row));
      have = after + tasks.length;
      more.hidden = tasks.length < PAGE;
      $("archivednone").hidden = have > 0;
    } catch (e) { say(e.message); }
  }

  function open(p) {
    project = p;
    $("archivedhead").textContent = `Archived in ${p.name}`;
    find.value = "";
    list.replaceChildren();
    box.showModal();
    read();
  }

  find.addEventListener("input", () => { clearTimeout(find.wait); find.wait = setTimeout(read, 200); });
  more.addEventListener("click", () => read(have));
  // a task's window may have brought one back
  $("taskbox").addEventListener("close", () => { if (box.open) read(); });
  return { open };
}
