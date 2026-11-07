// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// A project's folder picker (terminum-kanban.md AC-139): its server's folders, a row each as Files draws them,
// going up and in. terminumd says where it opens and lists through Files' own `/fb/api/ls`; trouble is said
// in the picker and the field stays typeable.
import { icon } from "./icons.js";
import { $, api, button, el } from "./kanbanutil.js";
import { settled } from "./panefiles.js";

const PAGE = 500;                      // a listing's page (CLAUDE.md §1): past it, the rest is typed
const isFolder = (e) => e.k === "d" || e.k === "L";

export function folderPicker() {
  const box = $("folderbox");
  let host = "", at = "", asked = 0, chosen = null;

  const say = (text) => { $("foldererror").textContent = text; $("foldererror").hidden = !text; };
  const up = (path) => path.replace(/\/[^/]*\/?$/, "") || "/";
  const into = (path, name) => `${path === "/" ? "" : path}/${name}`;
  const row = (name, path, glyph, title) => {
    const b = button("folderrow", "", () => show(path));
    b.title = title;
    b.append(icon(glyph, 15), el("span", "name", name));
    const li = el("li");
    li.append(b);
    return li;
  };

  /** `path`'s folders; a later press wins over an answer still on its way. */
  async function show(path) {
    const mine = ++asked;
    at = path;
    $("folderpath").textContent = path;
    say("");
    const q = new URLSearchParams({ path, limit: String(PAGE), hidden: "0" });
    if (host) q.set("host", host);
    let why = "";
    const page = await settled(() => api(`/fb/api/ls?${q}`).catch((e) => { why = e.message; return null; }));
    if (mine !== asked) return;
    const rows = path === "/" ? [] : [row("..", up(path), "corner-left-up", "Up")];
    if (page?.status === "ready") {
      rows.push(...page.entries.filter(isFolder).map((e) => row(e.n, into(path, e.n), "folder", into(path, e.n))));
      // folders come first: a page with a file in it holds them all
      if (page.total > page.entries.length && page.entries.every(isFolder)) say(`The first ${PAGE} folders shown: type the rest in the field.`);
      else if (rows.length < 2) rows.push(el("li", "colempty", "No folders in it."));
    } else say(why || page?.error || "The folder could not be listed.");
    $("folderok").disabled = page?.status !== "ready";
    $("folderlist").replaceChildren(...rows);
  }

  $("folderok").addEventListener("click", () => { chosen?.(at); box.close(); });
  box.addEventListener("close", () => { asked++; chosen = null; });

  /** Opens on `server` ("" for this Mac) at `dir`, or its home; `pick(path)` when Choose is pressed. */
  async function open(server, dir, pick) {
    host = server;
    chosen = pick;
    $("folderhead").textContent = `A folder on ${server || "this Mac"}`;
    $("folderlist").replaceChildren();
    $("folderpath").textContent = "";
    $("folderok").disabled = true;
    say("");
    box.showModal();
    const q = new URLSearchParams({ dir });
    if (server) q.set("host", server);
    try { await show((await api(`/api/task-projects/folder?${q}`)).path); } catch (e) { say(e.message); }
  }
  return { open };
}
