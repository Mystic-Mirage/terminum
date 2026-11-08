// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// A task's comments and history in its window (terminum-kanban.md AC-107, AC-108, AC-149): a section of its
// own under the task, newest first unless the user turned it; a comment a card with when it was written
// and changed, a line of history muted.
import { el, iconButton, read, write } from "./listparts.js";
import { api, button, renderInto, row, say, stamp } from "./kanbanutil.js";

const ORDER = "tm.activity.oldest";                 // this browser's choice: oldest first

/** A field for Markdown. */
export function editor(text, label) {
  const area = el("textarea", "field");
  area.value = area.defaultValue = text;
  if (label) area.setAttribute("aria-label", label);
  return area;
}

/** `refresh(saved)` reads the task again; `confirmed(question, yes, run)` asks, then runs. */
export function activity(t, { refresh, confirmed }) {
  /** A human's own comment changes or goes (AC-108): small keys on its head's line. */
  function commentActions(e, said) {
    const route = `/api/tasks/${t.id}/comments/${e.id}`;
    const edit = iconButton("icon", "pencil", "Edit", "Edit this comment", () => {
      const area = editor(e.text ?? "");
      const save = button("primary", "Save", async () => {
        try {
          await api(route, { method: "PATCH", body: { text: area.value } });
          refresh(true);
        } catch (err) { say(err.message); }
      });
      said.replaceChildren(area, row(save));
      area.focus();
    });
    const del = iconButton("icon", "x", "Delete", "Delete this comment", () => confirmed("Delete this comment?", "Delete", () => api(route, { method: "DELETE" })));
    const keys = el("span", "evacts");
    keys.append(edit, del);
    return keys;
  }

  function eventRow(e) {
    const li = el("li", `ev ev-${e.kind} by-${e.author_kind}`);
    const head = el("div", "evhead");
    head.append(el("strong", null, e.author_name), el("time", null, stamp(e.at)));
    if (e.edited_at) head.append(el("span", "evedited", `edited ${stamp(e.edited_at)}`));
    li.append(head);
    if (e.kind === "comment") {
      const said = el("div", "markdown");
      renderInto(said, e.text ?? "");
      li.append(said);
      if (e.author_kind === "human") head.append(commentActions(e, said));
    } else if (e.kind === "deleted") {
      li.append(el("em", null, "Comment deleted"));
    } else {
      li.append(el("span", "evtext", e.text ?? ""));
    }
    return li;
  }

  const wrap = el("section", "tasksec activity");
  const comments = t.history.filter((e) => e.kind === "comment").length;
  const head = el("div", "sechead");
  const oldest = !!read(ORDER);
  const turn = button("textkey", oldest ? "Oldest first" : "Newest first", () => { write(ORDER, oldest ? null : 1); refresh(true); });
  turn.title = "Turn the order of comments and history";
  turn.setAttribute("aria-label", `Order: ${turn.textContent}. Press to turn it`);
  head.append(el("h3", null, `Comments ${comments} · history`), turn);
  const list = el("ol", "history");
  const events = oldest ? t.history : [...t.history].reverse();
  list.append(...events.map(eventRow));
  wrap.append(head, list);
  return wrap;
}
