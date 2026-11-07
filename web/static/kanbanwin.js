// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// A task's window (AC-107, AC-108, AC-109, AC-113): its fields, text, files, and one list of comments
// and history. A comment by a human closes with CLOSE; RUN comes with Dispatcher.
import { askConfirm } from "./dialogs.js";
import { $, SEVERITIES, agentChip, api, button, choose, download, el, fileMarkdown, fillSelect, iconButton, onFiles, onSend, read, renderInto, say, sessionOptions, upload, write } from "./kanbanutil.js";

const time = (at) => new Date(at * 1000).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
const EDITS = ".tasktext textarea, .markdown textarea";   // a text or a comment being edited
const row = (...kids) => { const r = el("div", "actions"); r.append(...kids.filter(Boolean)); return r; };

/** A field for Markdown. */
function editor(text, label) {
  const area = el("textarea", "field");
  area.value = area.defaultValue = text;
  if (label) area.setAttribute("aria-label", label);
  return area;
}

export function taskWindow({ getMeta, runs, openTask, newSubtask }) {
  const box = $("taskbox"), body = $("taskbody");
  let shown = null, opening = 0;

  /** A change of a field: whether terminumd took it. The window shows what stands; a text being
   *  typed stays (refresh). The board hears of it from terminumd. */
  const patch = async (id, change) => {
    let took = false;
    try {
      const answered = await api(`/api/tasks/${id}`, { method: "PATCH", body: change });
      if (answered.note) say(answered.note);
      took = true;
    } catch (e) {
      if (e.status === 409) return moveAcross(id, change);
      if (e.status === 412) return textChanged(id, change);
      say(e.message);
    }
    await refresh();
    return took;
  };

  /** A task with a session moving to another project: its session goes too, or is unlinked (AC-113). */
  async function moveAcross(id, change) {
    const how = await choose(`${id} has a session. What becomes of it?`, "A session stands in one project.", [["move", "Move the session too"], ["unlink", "Unlink the session"]]);
    if (how) return patch(id, { ...change, session_choice: how });
    refresh(true);
    return false;
  }

  /** Another changed the text while this one was written: nothing goes unasked; Cancel keeps writing (AC-133). */
  async function textChanged(id, { text_rev: _, ...change }) {
    if (await askConfirm("The text changed meanwhile. Put yours over it? The other text goes.", "Overwrite")) return patch(id, change);
    return false;
  }

  function select(label, options, value, changed) {
    const line = el("label", "fieldrow", label);
    const s = el("select", "field");
    fillSelect(s, options, value);
    s.addEventListener("change", () => changed(s.value));
    line.append(s);
    return line;
  }

  /** The time a RUN's agent has, in minutes; empty: none (terminum-dispatcher.md AC-126). */
  function budgetField(t) {
    const line = el("label", "fieldrow", "Budget, minutes");
    const f = el("input", "field");
    f.type = "number";
    f.min = "5";
    f.step = "5";
    f.value = t.budget_seconds ? String(t.budget_seconds / 60) : "";
    f.placeholder = "None";
    f.title = "Only for a session RUN started: 5 minutes to 7 days";
    f.addEventListener("change", () => patch(t.id, { budget_minutes: f.value ? Number(f.value) : null }));
    line.append(f);
    return line;
  }

  function fields(t) {
    const meta = getMeta(), grid = el("div", "fieldgrid");
    const key = t.session?.key ?? "";
    const sessions = sessionOptions(meta);
    if (key && !sessions.some(([k]) => k === key)) sessions.push([key, "Closed session"]);
    const owners = meta.users.map((u) => [u.id, u.name]);
    if (!owners.some(([u]) => u === t.owner)) owners.push([t.owner, t.owner_name]);
    const stands = el("div", "fieldrow", "Status");
    const value = el("span", "fieldvalue", t.archived_at ? "Archived" : t.status);
    if (t.agent) value.append(agentChip(t.agent));
    stands.append(value);
    grid.append(
      stands,
      select("Severity", SEVERITIES.map((s) => [s, s]), t.severity, (severity) => patch(t.id, { severity })),
      select("Owner", owners, t.owner, (owner) => patch(t.id, { owner })),
      select("Project", meta.projects.map((p) => [p.id, p.name]), t.project, (project) => patch(t.id, { project })),
      select("Session", sessions, key, (session) => patch(t.id, { session: session || null })),
      budgetField(t));
    return grid;
  }

  /** Asked first, then `run`; a refusal is said; the window shows what stands. */
  const confirmed = async (question, yes, run) => {
    if (await askConfirm(question, yes)) {
      await run().catch((e) => say(e.message));
      refresh();
    }
  };

  /** Files for a text: each kept by terminumd, linked in the text (AC-106, AC-109). */
  async function addFiles(list, area, kept) {
    for (const f of list) {
      try {
        const file = await upload(f, getMeta().most_bytes);
        kept(file);
        area.value += `${area.value && !area.value.endsWith("\n") ? "\n" : ""}${fileMarkdown(file)}\n`;
        area.dispatchEvent(new Event("input"));
      } catch (e) { say(e.message); }
    }
  }

  /** A text field that takes files dropped or pasted. */
  function takesFiles(area, kept) {
    area.classList.add("droparea");
    onFiles(area, area, (list) => addFiles(list, area, kept), (on) => area.classList.toggle("dropping", on));
  }

  function text(t) {
    const wrap = el("section", "tasktext");
    const shownText = el("div", "markdown");
    renderInto(shownText, t.text);
    /** Its editor, with files already dropped on the text shown. */
    let editing = false;
    const startEdit = (files = []) => {
      editing = true;
      wrap.classList.remove("droparea", "dropping");
      const area = editor(t.text, "Text, Markdown");
      area.rows = 10;
      area.placeholder = "Markdown. Drop or paste files here.";
      const dropped = [], kept = (file) => dropped.push(file.id);
      takesFiles(area, kept);
      const save = button("primary", "Save", async () => { if (await patch(t.id, { text: area.value, text_rev: t.text_rev, files: dropped })) refresh(true); });
      wrap.replaceChildren(area, row(button(null, "Cancel", () => refresh(true)), save));
      area.focus();
      addFiles(files, area, kept);
    };
    const edit = button(null, "Edit", () => startEdit());
    wrap.classList.add("droparea");
    onFiles(wrap, wrap, (list) => { if (!editing) startEdit(list); }, (on) => wrap.classList.toggle("dropping", on && !editing));
    const purge = button(null, "Purge text", () => confirmed(`Purge the text of ${t.id}? It leaves the store for good. Use this for a secret pasted by mistake.`, "Purge",
      () => api(`/api/tasks/${t.id}/purge`, { method: "POST" })));
    wrap.append(shownText, row(edit, t.text ? purge : null));
    return wrap;
  }

  /** Its parent, or its subtasks and the key that makes one: one level (AC-112). */
  function family(t) {
    if (t.parent) {
      const up = el("p", "subof", "Subtask of ");
      up.append(button("linklike", t.parent, () => openTask(t.parent)));
      return up;
    }
    const wrap = el("section", "subtasks");
    const head = el("div", "edithead");
    head.append(el("span", null, t.subtasks ? `Subtasks ${t.subtasks_done}/${t.subtasks}` : "Subtasks"), button("textkey", "New subtask", () => newSubtask(t)));
    const list = el("ul", "filelist");
    list.append(...t.children.map((k) => {
      const li = el("li");
      li.append(button("linklike", `${k.id} ${k.title}`, () => openTask(k.id)), el("small", null, k.archived_at ? `${k.status} · archived` : k.status));
      return li;
    }));
    wrap.append(head, list);
    return wrap;
  }

  function filesOf(t) {
    if (!t.files.length) return null;
    const list = el("ul", "filelist");
    list.append(...t.files.map((f) => {
      const li = el("li");
      const open = button("linklike", f.name, () => download(f.id, f.name).catch((e) => say(e.message)));
      const del = iconButton("icon", "x", `Delete ${f.name}`, "Delete this file", () => confirmed(`Delete ${f.name}? Its bytes leave the store for good.`, "Delete", () => api(`/api/task-files/${f.id}`, { method: "DELETE" })));
      li.append(open, el("small", null, `${Math.ceil(f.bytes / 1024)} KB`), del);
      return li;
    }));
    return list;
  }

  /** A human's own comment changes or goes (AC-108): small keys on its head's line. */
  function commentActions(t, e, said) {
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

  function eventRow(t, e) {
    const li = el("li", `ev ev-${e.kind} by-${e.author_kind}`);
    const head = el("div", "evhead");
    head.append(el("strong", null, e.author_name), el("time", null, time(e.at)));
    li.append(head);
    if (e.kind === "comment") {
      const said = el("div", "markdown");
      renderInto(said, e.text ?? "");
      li.append(said);
      if (e.author_kind === "human") head.append(commentActions(t, e, said));
    } else if (e.kind === "deleted") {
      li.append(el("em", null, "Comment deleted"));
    } else {
      li.append(el("span", "evtext", e.text ?? ""));
    }
    return li;
  }

  function commentForm(t) {
    const wrap = el("section", "newcomment");
    const draft = `tm.comment.${t.id}`, filesKey = `tm.commentfiles.${t.id}`;
    const area = editor(read(draft) ?? "", "Comment");
    area.rows = 4;
    area.placeholder = "Comment, Markdown. Drop or paste files here.";
    area.addEventListener("input", () => write(draft, area.value));
    // the files of a comment being written: kept with its text, or the sweep would take them
    const dropped = () => read(filesKey) ?? [];
    takesFiles(area, (file) => write(filesKey, [...dropped(), file.id]));
    const close = button(t.run_holds ? "primary" : null, "Close", async () => {
      if (area.value.trim()) {
        try {
          await api(`/api/tasks/${t.id}/comments`, { method: "POST", body: { text: area.value, files: dropped() } });
        } catch (e) { say(e.message); return; }
        write(draft, null);
        write(filesKey, null);
      }
      tryClose();
    });
    // Run: the comment goes with it, and on in its session or a new one (terminum-dispatcher.md AC-122)
    const run = button(t.run_holds ? null : "primary", "Run", async () => {
      try {
        if (await runs.open([t], { words: area.value, files: dropped() }) !== true) return;
      } catch (e) { say(e.message); return; }
      write(draft, null);
      write(filesKey, null);
      tryClose();
    });
    onSend(area, () => close.click());
    close.title = "Keep the comment and close: nothing moves, the session hears nothing";
    // its run cannot be reached or is no longer wanted: its tasks fail, the window stays (terminum-dispatcher.md AC-138)
    const giveUp = t.run_holds ? button(null, "Give up", () => confirmed(`Give up on this run? ${t.run_holds} go to FAILED. Dispatcher lets go of its window, which stays open, and an agent in it may go on.`, "Give up",
      () => api(`/api/tasks/${t.id}/give-up`, { method: "POST" }))) : null;
    // a task nobody needs leaves the board by this (AC-118); it stands where it stood when it comes back
    const shelve = button(null, t.archived_at ? "Bring back" : "Archive", () => api(`/api/tasks/${t.id}/archive`, { method: "POST", body: { on: !t.archived_at } }).then(() => refresh(), (e) => say(e.message)));
    shelve.classList.add("evenleft");
    wrap.append(area, row(t.run_holds ? null : shelve, giveUp, close, t.run_holds ? null : run));
    return wrap;
  }

  function draw(t) {
    shown = t;
    $("tasktitle").textContent = `${t.id} ${t.title}`;
    const history = el("ol", "history");
    history.append(...t.history.map((e) => eventRow(t, e)));
    const closed = t.session?.closed ? el("p", "cstate closed", "session closed") : null;
    const scroll = body.scrollTop;
    body.replaceChildren(...[fields(t), closed, text(t), family(t), filesOf(t), history, commentForm(t)].filter(Boolean));
    body.scrollTop = scroll;
  }

  /** The newest asked is the one that shows, whichever answers last. */
  async function open(id) {
    const mine = ++opening;
    const t = await api(`/api/tasks/${id}`);
    if (mine !== opening) return;
    draw(t);
    if (!box.open) box.showModal();
  }

  /** A text being edited, changed from what shows. */
  const edited = () => [...body.querySelectorAll(EDITS)].some((a) => a.value !== a.defaultValue);

  /** Closes it; an edit not saved is asked about first. Whether it closed. */
  async function tryClose() {
    if (edited() && !await askConfirm("Discard your edit? It is not saved.", "Discard")) return false;
    box.close();
    return true;
  }

  /** Reads it again; a text being edited holds until it is saved (`saved`). */
  async function refresh(saved = false) {
    if (!box.open || !shown) return;
    const editing = () => !saved && body.querySelector(EDITS);
    if (editing()) return;
    try {
      const t = await api(`/api/tasks/${shown.id}`);
      if (!editing()) draw(t);                     // an editor opened while it was read stays
    } catch (e) { say(e.message); }
  }

  $("taskclose").addEventListener("click", tryClose);
  box.addEventListener("cancel", (e) => { if (edited()) { e.preventDefault(); tryClose(); } });
  return { open, refresh, tryClose, shows: (id) => box.open && shown?.id === id, shownId: () => shown?.id };
}
