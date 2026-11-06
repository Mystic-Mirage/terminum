// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The new-task window (AC-106): a title, Markdown with Preview, files dropped or pasted into the
// text, in TODO unless said. A draft survives closing and a reload on this browser.
import { askConfirm } from "./dialogs.js";
import { $, api, el, fileMarkdown, fillSelect, iconButton, onFiles, onSend, read, renderInto, sessionOptions, upload, write } from "./kanbanutil.js";

const DRAFT = "tm.taskdraft";

export function taskForm({ getMeta, getFilter, onMade }) {
  const box = $("newbox");
  let files = [], uploads = 0, offered = "";
  /** Create waits for the files on their way: pressed earlier, they would not be on the task (AC-133). */
  const canCreate = (busy = false) => { $("newcreate").disabled = busy || uploads > 0; };

  const error = (text) => { $("newerror").textContent = text ?? ""; $("newerror").hidden = !text; };
  const draft = () => ({ title: $("newtitle").value, text: $("newtext").value, project: $("newproj").value, severity: $("newsev").value, owner: $("newowner").value, session: $("newsession").value, files });
  const keep = () => write(DRAFT, draft());

  function showFiles() {
    $("newfiles").replaceChildren(...files.map((f) => {
      const li = el("li", null, f.name);
      li.append(iconButton("icon", "x", `Remove ${f.name}`, "Remove this file", () => {
        files = files.filter((o) => o.id !== f.id);
        $("newtext").value = $("newtext").value.replaceAll(fileMarkdown(f), "");
        showFiles();
        keep();
      }));
      return li;
    }));
  }

  function insert(text) {
    const t = $("newtext"), at = t.selectionStart ?? t.value.length;
    t.value = `${t.value.slice(0, at)}${text}${t.value.slice(t.selectionEnd ?? at)}`;
    t.selectionStart = t.selectionEnd = at + text.length;
    keep();
  }

  /** Kept by terminumd at once, linked at the cursor; a refusal keeps the window and says why. */
  async function add(list) {
    error(null);
    uploads += list.length;
    canCreate();
    for (const f of list) {
      try {
        const kept = await upload(f, getMeta().most_bytes);
        files.push(kept);
        insert(`${fileMarkdown(kept)}\n`);
      } catch (e) { error(e.message); }
      uploads--;
      showFiles();
      keep();
    }
    canCreate();
  }

  function view(preview) {
    $("newwrite").setAttribute("aria-selected", String(!preview));
    $("newpreview").setAttribute("aria-selected", String(preview));
    $("newtext").hidden = preview;
    $("newshown").hidden = !preview;
    if (preview) renderInto($("newshown"), $("newtext").value);
  }

  function open(given = {}) {
    const meta = getMeta(), saved = read(DRAFT) ?? {};
    const filter = getFilter().project;
    fillSelect($("newproj"), meta.projects.map((p) => [p.id, p.name]), given.project ?? saved.project ?? (filter.length === 1 ? filter[0] : "default"));
    fillSelect($("newsession"), sessionOptions(meta), given.session ?? saved.session ?? "");
    $("newsev").value = saved.severity ?? "P2";
    fillSelect($("newowner"), meta.users.map((u) => [u.id, u.name]), saved.owner ?? "human");
    $("newtitle").value = saved.title ?? "";
    $("newtext").value = given.text ? `${saved.text ? `${saved.text}\n\n` : ""}${given.text}` : saved.text ?? "";
    files = saved.files ?? [];
    showFiles();
    view(false);
    error(null);
    box.showModal();
    $("newtitle").focus();
  }

  async function create() {
    error(null);
    if ($("newcreate").disabled) return;
    const d = draft();
    canCreate(true);
    try {
      const made = await api("/api/tasks", { method: "POST", body: { title: d.title, text: d.text, project: d.project, severity: d.severity, owner: d.owner || undefined, session: d.session || undefined, files: files.map((f) => f.id) } });
      write(DRAFT, null);
      files = [];
      box.close();
      onMade(made);
    } catch (e) { error(e.message); $("newtitle").focus(); } finally { canCreate(); }
  }

  $("newcreate").addEventListener("click", create);
  // Enter in the title is Create; what is typed never goes by a slip: Discard asks
  box.querySelector("form").addEventListener("submit", (e) => { e.preventDefault(); create(); });
  onSend($("newtext"), create);
  $("newdiscard").addEventListener("click", async () => {
    const typed = $("newtitle").value.trim() || $("newtext").value.trim() || files.length;
    if (typed && !await askConfirm("Discard this task? What you typed and its files go.", "Discard")) return;
    write(DRAFT, null);
    files = [];
    box.close();
  });
  $("newclose").addEventListener("click", () => { keep(); box.close(); });
  box.addEventListener("cancel", keep);
  box.querySelector("form").addEventListener("input", keep);
  $("newwrite").addEventListener("click", () => view(false));
  $("newpreview").addEventListener("click", () => view(true));
  $("newadd").addEventListener("click", () => $("newpick").click());
  $("newpick").addEventListener("change", (e) => { add([...e.target.files]); e.target.value = ""; });
  onFiles($("newtext"), box, add, (on) => box.classList.toggle("dropping", on));

  /** The pickers follow the projects and sessions while the window is open; a chosen one gone is said. */
  function refresh() {
    if (!box.open) return;
    const meta = getMeta(), projects = meta.projects.map((p) => [p.id, p.name]), sessions = sessionOptions(meta), now = JSON.stringify([projects, sessions]);
    if (now === offered) return;
    offered = now;
    const project = $("newproj").value, session = $("newsession").value;
    fillSelect($("newproj"), projects, project);
    fillSelect($("newsession"), sessions, session);
    if ($("newproj").value !== project) error("The project you chose is gone: choose another.");
    else if ($("newsession").value !== session) error("The session you chose is gone: choose another.");
    keep();
  }

  return { open, refresh, add };
}
