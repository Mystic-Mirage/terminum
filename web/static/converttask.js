// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// Convert to task (terminum-kanban.md AC-116): a bookmark or reminder becomes a TODO task on its project's board in
// one small window: its title, severity (P2), project and session (the one shown and its project), nothing else to
// type. Its text is the record's, quoted, with a link to the session it came from; the task counts as made in that
// session (AC-150); the board does not open, and the record goes once the task is kept.
import { api, fillSelect, showError } from "./kanbanutil.js";
import { quote } from "./marks.js";
import { titleOf } from "./marktext.js";
import { sessionUrl } from "./tasklink.js";

const $ = (id) => document.getElementById(id);
const DEFAULT = "default";

/** sid(): the session shown; panes(): every session listed; projects(): the list's projects; made(task): after it was kept. */
export function recordTasks({ sid, panes, projects, made }) {
  const box = $("convertbox");
  let record = null;

  const projectOf = (key) => projects().find((p) => p.members?.includes(key))?.id ?? DEFAULT;
  const titleOfPane = (key) => panes().find((p) => p.id === key)?.title;

  function open(item) {
    record = item;
    const here = sid() ?? item.sid ?? "";
    $("converttitle").value = titleOf(item, 200);
    fillSelect($("convertsev"), ["P0", "P1", "P2", "P3"].map((s) => [s, s]), "P2");
    fillSelect($("convertproj"), [[DEFAULT, "Default"], ...projects().map((p) => [p.id, p.name])], projectOf(here));
    fillSelect($("convertsession"), [["", "None"], ...panes().map((p) => [p.id, p.title || p.id])], here);
    showError($("converterror"), "");
    box.showModal();
    $("converttitle").select();
  }

  async function keep() {
    const from = record.sid && titleOfPane(record.sid) ? record.sid : null;   // a closed session links nowhere
    const where = from ? `\n\nFrom [${titleOfPane(from)}, line ${record.n}](${sessionUrl(from)})` : "";
    const body = {
      title: $("converttitle").value, text: (record.note ? `${record.note}\n\n` : "") + quote(record.text) + where,
      severity: $("convertsev").value, project: $("convertproj").value, session: $("convertsession").value || null,
      made_in: from ?? (sid() || null), record: record.id,
    };
    const go = $("convertok");
    go.disabled = true;
    try {
      const t = await api("/api/tasks", { method: "POST", body });
      box.close();
      made(t);
    } catch (e) { showError($("converterror"), e.message); } finally { go.disabled = false; }
  }

  $("convertok").addEventListener("click", keep);
  $("converttitle").addEventListener("keydown", (e) => { if (e.key === "Enter" && !e.isComposing) { e.preventDefault(); keep(); } });
  return { open };
}
