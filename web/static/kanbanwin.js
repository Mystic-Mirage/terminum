// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// A task's window (AC-107, AC-108, AC-109, AC-113): its fields, text, files, and one list of comments
// and history. A comment by a human closes with CLOSE; RUN comes with Dispatcher.
import { COLUMNS, api, ask, choose, download, el, fileMarkdown, renderInto, say, upload } from "./kanbanutil.js";

const $ = (id) => document.getElementById(id);
const time = (at) => new Date(at * 1000).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });

export function taskWindow({ getMeta, onChange }) {
  const box = $("taskbox"), body = $("taskbody");
  let shown = null, draftFiles = [];

  const patch = async (id, change) => {
    try {
      await api(`/api/tasks/${id}`, { method: "PATCH", body: change });
    } catch (e) {
      if (e.message === "session_choice") return moveAcross(id, change);
      say(e.message);
    }
    onChange();
    return refresh(true);
  };

  /** A task with a session moving to another project: its session goes too, or is unlinked (AC-113). */
  async function moveAcross(id, change) {
    const how = await choose(`${id} has a session. What becomes of it?`, "A session stands in one project.", [["move", "Move the session too"], ["unlink", "Unlink the session"]]);
    if (how) await patch(id, { ...change, session_choice: how });
    else refresh();
  }

  function select(label, options, value, onChange2) {
    const row = el("label", "fieldrow", label);
    const s = el("select", "field");
    s.replaceChildren(...options.map(([id, name]) => { const o = el("option", null, name); o.value = id; return o; }));
    s.value = value;
    s.addEventListener("change", () => onChange2(s.value));
    row.append(s);
    return row;
  }

  function fields(t) {
    const meta = getMeta(), grid = el("div", "fieldgrid");
    const key = t.session?.key ?? "";
    const sessions = [["", "None"], ...meta.sessions.map((s) => [s.key, s.title || s.key])];
    if (key && !sessions.some(([k]) => k === key)) sessions.push([key, "Closed session"]);
    const owners = meta.users.map((u) => [u.id, u.name]);
    if (!owners.some(([u]) => u === t.owner)) owners.push([t.owner, t.owner_name]);
    grid.append(
      select("Status", COLUMNS.map((c) => [c, c]), t.status, (status) => patch(t.id, { status })),
      select("Severity", ["P0", "P1", "P2", "P3"].map((s) => [s, s]), t.severity, (severity) => patch(t.id, { severity })),
      select("Owner", owners, t.owner, (owner) => patch(t.id, { owner })),
      select("Project", meta.projects.map((p) => [p.id, p.name]), t.project, (project) => patch(t.id, { project })),
      select("Session", sessions, key, (session) => patch(t.id, { session: session || null })));
    return grid;
  }

  function text(t) {
    const wrap = el("section", "tasktext");
    const shownText = el("div", "md");
    renderInto(shownText, t.text);
    const edit = el("button", null, "Edit");
    edit.type = "button";
    edit.addEventListener("click", () => {
      const area = el("textarea", "field");
      area.value = t.text;
      area.rows = 10;
      area.setAttribute("aria-label", "Text, Markdown");
      const save = el("button", "primary", "Save");
      save.type = "button";
      save.addEventListener("click", () => patch(t.id, { text: area.value }));
      const cancel = el("button", null, "Cancel");
      cancel.type = "button";
      cancel.addEventListener("click", () => refresh(true));
      wrap.replaceChildren(area, row(cancel, save));
      area.focus();
    });
    const purge = el("button", "quiet", "Purge text");
    purge.type = "button";
    purge.addEventListener("click", async () => {
      if (await ask(`Purge the text of ${t.id}?`, "It leaves the store for good. Use this for a secret pasted by mistake.", "Purge")) {
        await api(`/api/tasks/${t.id}/purge`, { method: "POST", body: {} }).catch((e) => say(e.message));
        onChange();
        refresh();
      }
    });
    wrap.append(shownText, row(edit, t.text ? purge : null));
    return wrap;
  }

  const row = (...kids) => { const r = el("div", "actions"); r.append(...kids.filter(Boolean)); return r; };

  function filesOf(t) {
    if (!t.files.length) return null;
    const list = el("ul", "filelist");
    list.append(...t.files.map((f) => {
      const li = el("li");
      const open = el("button", "linklike", f.name);
      open.type = "button";
      open.addEventListener("click", () => download(f.id, f.name).catch((e) => say(e.message)));
      const del = el("button", "icon", "×");
      del.type = "button";
      del.setAttribute("aria-label", `Delete ${f.name}`);
      del.addEventListener("click", async () => {
        if (await ask(`Delete ${f.name}?`, "Its bytes leave the store for good.", "Delete")) {
          await api(`/api/task-files/${f.id}`, { method: "DELETE" }).catch((e) => say(e.message));
          onChange();
          refresh();
        }
      });
      li.append(open, el("small", null, `${Math.ceil(f.bytes / 1024)} KB`), del);
      return li;
    }));
    return list;
  }

  function eventRow(t, e) {
    const li = el("li", `ev ev-${e.kind} by-${e.author_kind}`);
    const head = el("div", "evhead");
    head.append(el("strong", null, e.author_name), el("time", null, time(e.at)));
    li.append(head);
    if (e.kind === "comment") {
      const said = el("div", "md");
      renderInto(said, e.text ?? "");
      li.append(said);
      if (e.author_kind === "human") {
        const edit = el("button", "quiet", "Edit");
        edit.type = "button";
        edit.addEventListener("click", () => {
          const area = el("textarea", "field");
          area.value = e.text ?? "";
          const save = el("button", "primary", "Save");
          save.type = "button";
          save.addEventListener("click", async () => {
            await api(`/api/tasks/${t.id}/comments/${e.id}`, { method: "PATCH", body: { text: area.value } }).catch((err) => say(err.message));
            refresh(true);
          });
          said.replaceChildren(area, row(save));
          area.focus();
        });
        const del = el("button", "quiet", "Delete");
        del.type = "button";
        del.addEventListener("click", async () => {
          if (await ask("Delete this comment?", null, "Delete")) {
            await api(`/api/tasks/${t.id}/comments/${e.id}`, { method: "DELETE" }).catch((err) => say(err.message));
            refresh();
          }
        });
        li.append(row(edit, del));
      }
    } else if (e.kind === "deleted") {
      li.append(el("em", null, "Comment deleted"));
    } else {
      li.append(el("span", "evtext", e.text ?? ""));
    }
    return li;
  }

  function commentForm(t) {
    const wrap = el("section", "newcomment");
    const area = el("textarea", "field");
    area.rows = 4;
    area.placeholder = "Comment, Markdown. Drop or paste files here.";
    area.setAttribute("aria-label", "Comment");
    area.value = sessionStorage.getItem(`tm.comment.${t.id}`) ?? "";
    area.addEventListener("input", () => sessionStorage.setItem(`tm.comment.${t.id}`, area.value));
    const add = async (list) => {
      for (const f of list) {
        try {
          const kept = await upload(f);
          draftFiles.push(kept);
          area.value += `${area.value && !area.value.endsWith("\n") ? "\n" : ""}${fileMarkdown(kept)}\n`;
          sessionStorage.setItem(`tm.comment.${t.id}`, area.value);
        } catch (e) { say(e.message); }
      }
    };
    area.addEventListener("paste", (e) => { const l = [...(e.clipboardData?.files ?? [])]; if (l.length) { e.preventDefault(); add(l); } });
    area.addEventListener("dragover", (e) => { if (e.dataTransfer?.types.includes("Files")) e.preventDefault(); });
    area.addEventListener("drop", (e) => { const l = [...(e.dataTransfer?.files ?? [])]; if (l.length) { e.preventDefault(); add(l); } });
    const close = el("button", "primary", "CLOSE");
    close.type = "button";
    close.title = "Keep the comment and close: nothing moves, the session hears nothing";
    close.addEventListener("click", async () => {
      if (area.value.trim()) {
        try {
          await api(`/api/tasks/${t.id}/comments`, { method: "POST", body: { text: area.value, files: draftFiles.map((f) => f.id) } });
        } catch (e) { say(e.message); return; }
        sessionStorage.removeItem(`tm.comment.${t.id}`);
        draftFiles = [];
        onChange();
      }
      box.close();
    });
    wrap.append(area, row(close));
    return wrap;
  }

  function draw(t) {
    shown = t;
    $("tasktitle").textContent = `${t.id} ${t.title}`;
    const history = el("ol", "history");
    history.append(...t.history.map((e) => eventRow(t, e)));
    const closed = t.session?.closed ? el("p", "note", "session closed") : null;
    const pieces = [fields(t), closed, text(t), filesOf(t), history, commentForm(t)].filter(Boolean);
    const scroll = body.scrollTop;
    const typing = body.querySelector(".newcomment textarea");
    const kept = typing?.value;
    body.replaceChildren(...pieces);
    body.scrollTop = scroll;
    if (kept) body.querySelector(".newcomment textarea").value = kept;
  }

  async function open(id) {
    draftFiles = [];
    draw(await api(`/api/tasks/${id}`));
    if (!box.open) box.showModal();
  }

  /** Reads it again; a text being edited holds until it is saved (`saved`). */
  async function refresh(saved = false) {
    if (!box.open || !shown) return;
    if (!saved && body.querySelector(".tasktext textarea, .md textarea")) return;
    try { draw(await api(`/api/tasks/${shown.id}`)); } catch (e) { say(e.message); }
  }

  $("taskclose").addEventListener("click", () => box.close());
  return { open, refresh };
}
