// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// What the board's parts share: terminumd's routes, small DOM helpers, dialogs and files.
import * as signin from "./signin.js";
import { PROJECT_HUES, el, hueColor, iconButton } from "./listparts.js";
import { renderers } from "./renderlib.js";
import { toast } from "./toast.js";

export const COLUMNS = ["TODO", "IN PROGRESS", "WAIT FOR HUMAN", "FAILED", "DONE"];
export const SEVERITIES = ["P0", "P1", "P2", "P3"];
export const THIS_MAC = "This Mac";
export const DEFAULT = "default";                  // terminumd's Default project
/** A server's choices: none is this Mac. */
export const hostOptions = (hosts) => [["", THIS_MAC], ...hosts.map((h) => [h, h])];
/** An agent's color: its place in the projects' palette (terminum-dispatcher.md AC-138). */
export const agentColor = (a) => hueColor(PROJECT_HUES[(a.color ?? 0) % PROJECT_HUES.length]);
export const $ = (id) => document.getElementById(id);

/** A page with no sign-in goes to /sessions/, whose form signs in. */
const toSignIn = () => { location.href = "/sessions/"; };
signin.onRefused(toSignIn);

/** A JSON route (a write always has a body: terminumd asks its type); a refusal throws terminumd's
 *  message, with its status. */
export async function api(path, { method = "GET", body } = {}) {
  const sent = method === "GET" ? undefined : JSON.stringify(body ?? {});
  const r = await fetch(path, { method, headers: sent ? { "Content-Type": "application/json" } : undefined, body: sent });
  const m = await r.json().catch(() => ({}));
  if (r.status === 401) toSignIn();
  if (!r.ok) {
    const e = new Error(m.error ?? `terminumd answered ${r.status}`);
    e.status = r.status;
    throw e;
  }
  return m;
}

/** One short message, where the action was taken cannot hold it. */
export const say = (text) => toast(text, 4000);

/** A text pressed ⌘Enter in goes where its button would. */
export const onSend = (area, go) => area.addEventListener("keydown", (e) => { if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) { e.preventDefault(); go(); } });

export function button(cls, text, onClick) {
  const b = el("button", cls, text);
  b.type = "button";
  if (onClick) b.addEventListener("click", onClick);
  return b;
}

/** `select`'s options, each [value, label]; `chosen` if it is one of them, else the first. */
export function fillSelect(select, options, chosen) {
  select.replaceChildren(...options.map(([value, label]) => { const o = el("option", null, label); o.value = value; return o; }));
  select.value = options.some(([value]) => value === chosen) ? chosen : options[0]?.[0] ?? "";
}

/** A project's `agents` for `select`: a busy one not to be chosen; the first of `wanted` free, else the first free.
 *  `none`: the label of a choice of no agent, first. */
export function fillAgents(select, agents, busy, wanted, none = null) {
  const free = agents.filter((a) => !busy(a));
  const pick = wanted.find((id) => free.some((a) => a.id === id)) ?? free[0]?.id ?? "";
  fillSelect(select, [...(none ? [["", none]] : []), ...agents.map((a) => [a.id, busy(a) ? `${a.name} (${busy(a)})` : a.name])], pick);
  for (const o of select.options) o.disabled = !!o.value && !free.some((a) => a.id === o.value);
}

/** The agent a task is on, in its color (terminum-dispatcher.md AC-138). */
export function agentChip(agent) {
  const a = el("span", "cagent", agent.name);
  a.style.setProperty("--ag", agentColor(agent));
  a.title = `${agent.name} does it, or did`;
  return a;
}

/** The sessions a task can be linked to. */
export const sessionOptions = (meta) => [["", "None"], ...meta.sessions.map((s) => [s.key, s.title || s.key])];

/** A dialog's line of error: `text`, or hidden with none. */
export const showError = (box, text) => { box.textContent = text ?? ""; box.hidden = !text; };

/** Whether what is dragged is files. */
export const hasFiles = (e) => e.dataTransfer?.types.includes("Files");

/** Files pasted into `area` or dropped on `zone` go to `add`; `lit(on)` says one is over it. */
export function onFiles(area, zone, add, lit = () => {}) {
  area.addEventListener("paste", (e) => { const list = [...(e.clipboardData?.files ?? [])]; if (list.length) { e.preventDefault(); add(list); } });
  zone.addEventListener("dragover", (e) => { if (hasFiles(e)) { e.preventDefault(); lit(true); } });
  zone.addEventListener("dragleave", (e) => { if (!zone.contains(e.relatedTarget)) lit(false); });
  zone.addEventListener("drop", (e) => {
    const list = [...(e.dataTransfer?.files ?? [])];
    lit(false);
    if (list.length) { e.preventDefault(); add(list); }
  });
}

/** Keeps a file with terminumd: its {id, name, bytes, mime}. Over `most` bytes is said before it is sent. */
export async function upload(file, most) {
  if (file.size > most) throw new Error(`${file.name} is over ${Math.round(most / 2 ** 20)} MB`);
  const r = await fetch(`/api/task-files?name=${encodeURIComponent(file.name)}`, { method: "POST", headers: { "Content-Type": "application/octet-stream" }, body: file });
  const m = await r.json().catch(() => ({}));
  if (!r.ok) throw new Error(m.error ?? `${file.name}: terminumd answered ${r.status}`);
  return m;
}

const fileUrl = (id) => `/api/task-files/${id}`;

/** Markdown of a kept file: terminumd calls a file image/* only when it shows as one; anything else links. */
export const fileMarkdown = (f) => `${f.mime?.startsWith("image/") ? "!" : ""}[${f.name.replace(/[[\]]/g, "")}](${fileUrl(f.id)})`;

/** Saves a file through a fetch with the sign-in, never its address (terminum.md AC-09). */
export async function download(id, name) {
  const r = await fetch(fileUrl(id));
  if (!r.ok) throw new Error(`${name}: terminumd answered ${r.status}`);
  const url = URL.createObjectURL(await r.blob());
  const a = el("a");
  a.href = url;
  a.download = name;
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 60000);
}

/** Markdown by the Panel's rules (raw HTML off, javascript: links dropped, remote images not
 *  loaded); a task's own pictures are fetched with the sign-in, its files download. */
export async function renderInto(box, text) {
  box.textContent = text;
  try {
    box.innerHTML = (await renderers()).taskMarkdownHtml(text);
  } catch { return; }
  for (const img of box.querySelectorAll("img[data-raw]")) {
    signin.blobUrl(img.dataset.raw).then((got) => {
      if (!got.url) return;
      const done = () => URL.revokeObjectURL(got.url);   // a picture decoded needs its address no more
      img.addEventListener("load", done, { once: true });
      img.addEventListener("error", done, { once: true });
      img.src = got.url;
    });
  }
  for (const a of box.querySelectorAll("a")) {
    const id = /^\/api\/task-files\/([0-9a-f]{32})$/.exec(a.getAttribute("href") ?? "")?.[1];
    if (id) a.addEventListener("click", (e) => { e.preventDefault(); download(id, a.textContent).catch((err) => say(err.message)); });
    else { a.target = "_blank"; a.rel = "noopener noreferrer"; }
  }
}

/** 1,234 as 1.2k, 4,800,000 as 4.8M. */
export function countOf(n) {
  return n >= 1e6 ? `${(n / 1e6).toFixed(1).replace(/\.0$/, "")}M` : n >= 1e3 ? `${(n / 1e3).toFixed(1).replace(/\.0$/, "")}k` : String(n);
}
