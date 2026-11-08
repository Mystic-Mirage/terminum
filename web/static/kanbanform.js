// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The new-task window (AC-106): a title, Markdown with Preview, files dropped or pasted into the
// text, in TODO unless said. A draft survives closing and a reload on this browser.
import { askConfirm } from "./dialogs.js";
import { el, iconButton, read, write } from "./listparts.js";
import { $, DEFAULT, api, fileMarkdown, fillAgents, fillSelect, onFiles, onSend, renderInto, sessionOptions, showError, upload } from "./kanbanutil.js";

const DRAFT = "tm.taskdraft";

/** `onRun(task, agent)`: a task made by Run starts at once on the agent chosen (terminum-dispatcher.md AC-138);
 *  `slots()`: who is at a limit, as terminumd counts it. */
export function taskForm({ getMeta, getFilter, onMade, onRun, slots }) {
  const box = $("newbox");
  let files = [], uploads = 0, offered = "", parent = null, record = null, chose = null;   // chose: the agent the user picked in this window, "" for none
  /** Each its own draft: a subtask's never takes a new task's. */
  const draftKey = () => (parent ? `${DRAFT}.${parent}` : DRAFT);
  /** Create waits for the files on their way: pressed earlier, they would not be on the task (AC-133). */
  const canCreate = (busy = false) => { $("newcreate").disabled = $("newrun").disabled = busy || uploads > 0; };

  const error = (text) => showError($("newerror"), text);
  const draft = () => ({ title: $("newtitle").value, text: $("newtext").value, project: $("newproj").value, severity: $("newsev").value, owner: $("newowner").value, session: $("newsession").value, files });
  const keep = () => write(draftKey(), draft());

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

  /** The pool: the agent the user picked while it is free, else the project's last run's, else the first free;
   *  none picked stays none, whatever changes meanwhile (AC-106). */
  function showAgents() {
    const meta = getMeta(), project = $("newproj").value, all = meta.agents, now = slots();
    const busy = (a) => (now.full.includes(a.id) ? "at its limit" : now.full_projects.includes(project) ? "the project is at its limit" : "");
    fillAgents($("newagent"), all, busy, [chose, meta.last_agents?.[project]], !all.length ? "No agents yet" : all.every(busy) ? "All busy" : "Choose when it runs");
  }

  /** `given.parent`: a subtask of that task, in its project (AC-112). */
  function open(given = {}) {
    parent = given.parent ?? null;
    record = given.record ?? null;                    // goes once the task is kept (AC-116)
    const meta = getMeta(), saved = read(draftKey()) ?? {};
    $("newhead").textContent = parent ? `New subtask of ${parent}` : "New task";
    $("newproj").disabled = !!parent;
    const filter = getFilter().project;   // a draft with nothing typed says nothing of where it goes
    fillSelect($("newproj"), meta.projects.map((p) => [p.id, p.name]), given.project ?? (saved.title || saved.text ? saved.project : null) ?? (filter.length === 1 ? filter[0] : DEFAULT));
    chose = null;
    showAgents();
    fillSelect($("newsession"), sessionOptions(meta), given.session ?? saved.session ?? "");
    $("newsev").value = saved.severity ?? "P2";
    fillSelect($("newowner"), meta.users.map((u) => [u.id, u.name]), saved.owner ?? "human");
    $("newtitle").value = given.title ?? saved.title ?? "";
    $("newtext").value = given.text ? `${saved.text ? `${saved.text}\n\n` : ""}${given.text}` : saved.text ?? "";
    files = saved.files ?? [];
    showFiles();
    view(false);
    error(null);
    box.showModal();
    $("newtitle").focus();
  }

  async function create(run = false) {
    error(null);
    if ($("newcreate").disabled) return;
    const d = draft(), agent = $("newagent").value;
    canCreate(true);
    try {
      const made = await api("/api/tasks", { method: "POST", body: { title: d.title, text: d.text, project: d.project, severity: d.severity, owner: d.owner || undefined, session: d.session || undefined, parent: parent ?? undefined, files: files.map((f) => f.id), record: record ?? undefined } });
      write(draftKey(), null);
      files = [];
      box.close();
      if (run) onRun(made, agent); else onMade(made);
    } catch (e) { error(e.message); $("newtitle").focus(); } finally { canCreate(); }
  }

  $("newcreate").addEventListener("click", () => create());
  $("newrun").addEventListener("click", () => create(true));
  // Enter in the title is Create; what is typed never goes by a slip: Discard asks
  box.querySelector("form").addEventListener("submit", (e) => { e.preventDefault(); create(); });
  onSend($("newtext"), () => create());
  $("newdiscard").addEventListener("click", async () => {
    const typed = $("newtitle").value.trim() || $("newtext").value.trim() || files.length;
    if (typed && !await askConfirm("Discard this task? What you typed and its files go.", "Discard")) return;
    write(draftKey(), null);
    files = [];
    box.close();
  });
  $("newclose").addEventListener("click", () => { keep(); box.close(); });
  box.addEventListener("cancel", keep);
  box.querySelector("form").addEventListener("input", keep);
  $("newproj").addEventListener("change", showAgents);
  $("newagent").addEventListener("change", () => { chose = $("newagent").value; });
  $("newwrite").addEventListener("click", () => view(false));
  $("newpreview").addEventListener("click", () => view(true));
  $("newadd").addEventListener("click", () => $("newpick").click());
  $("newpick").addEventListener("change", (e) => { add([...e.target.files]); e.target.value = ""; });
  onFiles($("newtext"), box, add, (on) => $("newtext").classList.toggle("dropping", on));

  /** The pickers follow the projects and sessions while the window is open; a chosen one gone is said. */
  function refresh() {
    if (!box.open) return;
    const meta = getMeta(), projects = meta.projects.map((p) => [p.id, p.name]), sessions = sessionOptions(meta), now = JSON.stringify([projects, sessions]);
    showAgents();
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
