// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The new-task window (AC-106): a title, Markdown with Preview, files dropped or pasted into the
// text, in TODO unless said. A draft survives closing and a reload on this browser.
import { $, api, el, fileMarkdown, fillSelect, onFiles, renderInto, sessionOptions, upload } from "./kanbanutil.js";

const DRAFT = "tm.taskdraft";

function read(key) { try { return JSON.parse(localStorage.getItem(key) ?? "null"); } catch { return null; } }
function write(key, v) { try { if (v) localStorage.setItem(key, JSON.stringify(v)); else localStorage.removeItem(key); } catch { /* private mode: this page's only */ } }

export function taskForm({ getMeta, getFilter }) {
  const box = $("newbox");
  let files = [];

  const error = (text) => { $("newerror").textContent = text ?? ""; $("newerror").hidden = !text; };
  const draft = () => ({ title: $("newtitle").value, text: $("newtext").value, project: $("newproj").value, severity: $("newsev").value, owner: $("newowner").value, session: $("newsession").value, files });
  const keep = () => write(DRAFT, draft());

  function showFiles() {
    $("newfiles").replaceChildren(...files.map((f) => {
      const li = el("li", null, f.name);
      const x = el("button", "icon", "×");
      x.type = "button";
      x.setAttribute("aria-label", `Remove ${f.name}`);
      x.addEventListener("click", () => {
        files = files.filter((o) => o.id !== f.id);
        $("newtext").value = $("newtext").value.replaceAll(fileMarkdown(f), "");
        showFiles();
        keep();
      });
      li.append(x);
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
    for (const f of list) {
      try {
        const kept = await upload(f, getMeta().most_bytes);
        files.push(kept);
        insert(`${fileMarkdown(kept)}\n`);
      } catch (e) { error(e.message); }
    }
    showFiles();
    keep();
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
    const d = draft();
    $("newcreate").disabled = true;
    try {
      await api("/api/tasks", { method: "POST", body: { title: d.title, text: d.text, project: d.project, severity: d.severity, owner: d.owner || undefined, session: d.session || undefined, files: files.map((f) => f.id) } });
      write(DRAFT, null);
      files = [];
      box.close();
    } catch (e) { error(e.message); $("newtitle").focus(); } finally { $("newcreate").disabled = false; }
  }

  $("newcreate").addEventListener("click", create);
  $("newdiscard").addEventListener("click", () => { write(DRAFT, null); files = []; box.close(); });
  $("newclose").addEventListener("click", () => { keep(); box.close(); });
  box.addEventListener("cancel", keep);
  box.querySelector("form").addEventListener("input", keep);
  $("newwrite").addEventListener("click", () => view(false));
  $("newpreview").addEventListener("click", () => view(true));
  $("newpick").addEventListener("change", (e) => { add([...e.target.files]); e.target.value = ""; });
  onFiles($("newtext"), box, add, (on) => box.classList.toggle("dropping", on));

  return { open };
}
