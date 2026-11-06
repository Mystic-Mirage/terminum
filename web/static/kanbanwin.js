// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// A task's window (AC-107, AC-108, AC-109, AC-113): its fields, text, files, and one list of comments
// and history. A comment by a human closes with CLOSE; RUN comes with Dispatcher.
import { askConfirm } from "./dialogs.js";
import { $, COLUMNS, SEVERITIES, api, button, choose, download, el, fileMarkdown, fillSelect, onFiles, renderInto, say, sessionOptions, upload } from "./kanbanutil.js";

const time = (at) => new Date(at * 1000).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
const row = (...kids) => { const r = el("div", "actions"); r.append(...kids.filter(Boolean)); return r; };

/** A field for Markdown. */
function editor(text, label) {
  const area = el("textarea", "field");
  area.value = text;
  if (label) area.setAttribute("aria-label", label);
  return area;
}

export function taskWindow({ getMeta }) {
  const box = $("taskbox"), body = $("taskbody");
  let shown = null;

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

  function select(label, options, value, changed) {
    const line = el("label", "fieldrow", label);
    const s = el("select", "field");
    fillSelect(s, options, value);
    s.addEventListener("change", () => changed(s.value));
    line.append(s);
    return line;
  }

  function fields(t) {
    const meta = getMeta(), grid = el("div", "fieldgrid");
    const key = t.session?.key ?? "";
    const sessions = sessionOptions(meta);
    if (key && !sessions.some(([k]) => k === key)) sessions.push([key, "Closed session"]);
    const owners = meta.users.map((u) => [u.id, u.name]);
    if (!owners.some(([u]) => u === t.owner)) owners.push([t.owner, t.owner_name]);
    grid.append(
      select("Status", COLUMNS.map((c) => [c, c]), t.status, (status) => patch(t.id, { status })),
      select("Severity", SEVERITIES.map((s) => [s, s]), t.severity, (severity) => patch(t.id, { severity })),
      select("Owner", owners, t.owner, (owner) => patch(t.id, { owner })),
      select("Project", meta.projects.map((p) => [p.id, p.name]), t.project, (project) => patch(t.id, { project })),
      select("Session", sessions, key, (session) => patch(t.id, { session: session || null })));
    return grid;
  }

  /** Asked first, then `run`; a refusal is said; the window shows what stands. */
  const confirmed = async (question, yes, run) => {
    if (await askConfirm(question, yes)) {
      await run().catch((e) => say(e.message));
      refresh();
    }
  };

  function text(t) {
    const wrap = el("section", "tasktext");
    const shownText = el("div", "markdown");
    renderInto(shownText, t.text);
    const edit = button(null, "Edit", () => {
      const area = editor(t.text, "Text, Markdown");
      area.rows = 10;
      const save = button("primary", "Save", async () => { if (await patch(t.id, { text: area.value })) refresh(true); });
      wrap.replaceChildren(area, row(button(null, "Cancel", () => refresh(true)), save));
      area.focus();
    });
    const purge = button("quiet", "Purge text", () => confirmed(`Purge the text of ${t.id}? It leaves the store for good. Use this for a secret pasted by mistake.`, "Purge",
      () => api(`/api/tasks/${t.id}/purge`, { method: "POST" })));
    wrap.append(shownText, row(edit, t.text ? purge : null));
    return wrap;
  }

  function filesOf(t) {
    if (!t.files.length) return null;
    const list = el("ul", "filelist");
    list.append(...t.files.map((f) => {
      const li = el("li");
      const open = button("linklike", f.name, () => download(f.id, f.name).catch((e) => say(e.message)));
      const del = button("icon", "×", () => confirmed(`Delete ${f.name}? Its bytes leave the store for good.`, "Delete", () => api(`/api/task-files/${f.id}`, { method: "DELETE" })));
      del.setAttribute("aria-label", `Delete ${f.name}`);
      li.append(open, el("small", null, `${Math.ceil(f.bytes / 1024)} KB`), del);
      return li;
    }));
    return list;
  }

  /** A human's own comment changes or goes (AC-108). */
  function commentActions(t, e, said) {
    const route = `/api/tasks/${t.id}/comments/${e.id}`;
    const edit = button("quiet", "Edit", () => {
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
    return row(edit, button("quiet", "Delete", () => confirmed("Delete this comment?", "Delete", () => api(route, { method: "DELETE" }))));
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
      if (e.author_kind === "human") li.append(commentActions(t, e, said));
    } else if (e.kind === "deleted") {
      li.append(el("em", null, "Comment deleted"));
    } else {
      li.append(el("span", "evtext", e.text ?? ""));
    }
    return li;
  }

  function commentForm(t) {
    const wrap = el("section", "newcomment");
    const area = editor(sessionStorage.getItem(`tm.comment.${t.id}`) ?? "", "Comment");
    area.rows = 4;
    area.placeholder = "Comment, Markdown. Drop or paste files here.";
    area.addEventListener("input", () => sessionStorage.setItem(`tm.comment.${t.id}`, area.value));
    // the files of a comment being written: kept with its text, or the sweep would take them
    const filesKey = `tm.commentfiles.${t.id}`;
    const dropped = () => { try { return JSON.parse(sessionStorage.getItem(filesKey) ?? "[]"); } catch { return []; } };
    onFiles(area, area, async (list) => {
      for (const f of list) {
        try {
          const kept = await upload(f, getMeta().most_bytes);
          sessionStorage.setItem(filesKey, JSON.stringify([...dropped(), kept.id]));
          area.value += `${area.value && !area.value.endsWith("\n") ? "\n" : ""}${fileMarkdown(kept)}\n`;
          sessionStorage.setItem(`tm.comment.${t.id}`, area.value);
        } catch (e) { say(e.message); }
      }
    });
    const close = button("primary", "CLOSE", async () => {
      if (area.value.trim()) {
        try {
          await api(`/api/tasks/${t.id}/comments`, { method: "POST", body: { text: area.value, files: dropped() } });
        } catch (e) { say(e.message); return; }
        sessionStorage.removeItem(`tm.comment.${t.id}`);
        sessionStorage.removeItem(filesKey);
      }
      box.close();
    });
    close.title = "Keep the comment and close: nothing moves, the session hears nothing";
    wrap.append(area, row(close));
    return wrap;
  }

  function draw(t) {
    shown = t;
    $("tasktitle").textContent = `${t.id} ${t.title}`;
    const history = el("ol", "history");
    history.append(...t.history.map((e) => eventRow(t, e)));
    const closed = t.session?.closed ? el("p", "note", "session closed") : null;
    const scroll = body.scrollTop;
    body.replaceChildren(...[fields(t), closed, text(t), filesOf(t), history, commentForm(t)].filter(Boolean));
    body.scrollTop = scroll;
  }

  async function open(id) {
    draw(await api(`/api/tasks/${id}`));
    if (!box.open) box.showModal();
  }

  /** Reads it again; a text being edited holds until it is saved (`saved`). */
  async function refresh(saved = false) {
    if (!box.open || !shown) return;
    if (!saved && body.querySelector(".tasktext textarea, .markdown textarea")) return;
    try { draw(await api(`/api/tasks/${shown.id}`)); } catch (e) { say(e.message); }
  }

  $("taskclose").addEventListener("click", () => box.close());
  return { open, refresh, shows: (id) => box.open && shown?.id === id };
}
