// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// A task's subtasks in its window (terminum-kanban.md AC-112): each a full task with a parent. Made here,
// another task put under it, one taken out (it stays a task), and a task with none put under another.
import { askName } from "./dialogs.js";
import { el, iconButton } from "./listparts.js";
import { button, madeChanged, statusBox } from "./kanbanutil.js";

/** Asks for a task's ID; `put(id)` throws terminumd's refusal, said in the dialog. */
const askId = (text, yes, put) => askName(text, "", yes, 40, put);

/** `patch(id, change)` → whether terminumd took it, said where refused; `send(id, change)` throws instead. */
export function family(t, { openTask, newSubtask, patch, send }) {
  const wrap = el("section", "tasksec subtasks");
  const head = el("div", "sechead");
  if (t.parent) {
    const up = el("h3", null, "Subtask of ");
    up.append(button("linklike", t.parent, () => openTask(t.parent)));
    head.append(up, button("textkey", "Take out", () => patch(t.id, { parent: null })));
    head.lastChild.title = `Take ${t.id} out of ${t.parent}: it stays a task of its own`;
    wrap.append(head);
    return wrap;
  }
  const keys = el("span", "seckeys");
  keys.append(button("textkey", "Add existing", () => askId(`A task to put under ${t.id}: its ID`, "Add", (id) => send(id, { parent: t.id }))));
  // a task with subtasks of its own stays a parent
  if (!t.children.length) keys.append(button("textkey", "Make it a subtask", () => askId(`Put ${t.id} under which task? Its ID`, "Move", (id) => send(t.id, { parent: id }))));
  keys.append(button("textkey", "New subtask", () => newSubtask(t)));
  head.append(el("h3", null, t.subtasks ? `Subtasks ${t.subtasks_done}/${t.subtasks}` : "Subtasks"), keys);
  const list = el("ul", "sublist");
  list.append(...t.children.map((k) => {
    const li = el("li");
    const name = button("linklike subname", k.title, () => openTask(k.id));
    const meta = el("small", null, [k.archived_at ? `${k.status}, archived` : k.status, k.owner_name, madeChanged(k)].join(" · "));
    const out = iconButton("icon", "x", `Take ${k.id} out`, `Take ${k.id} out of ${t.id}: it stays a task of its own`, () => patch(k.id, { parent: null }));
    const text = el("div", "subtext");
    text.append(name, meta);
    li.append(statusBox(k, openTask), text, out);
    return li;
  }));
  wrap.append(head, list);
  return wrap;
}
