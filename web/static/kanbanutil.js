// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// What the board's parts share: terminumd's routes, small DOM helpers, dialogs and files.
export const COLUMNS = ["TODO", "IN PROGRESS", "WAIT FOR HUMAN", "FAILED", "DONE"];
const $ = (id) => document.getElementById(id);
const MOST_BYTES = 100 << 20;

export function el(tag, cls, text) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined && text !== null) e.textContent = text;
  return e;
}

/** A JSON route; a refusal throws terminumd's message, with its status. */
export async function api(path, { method = "GET", body } = {}) {
  const r = await fetch(path, { method, headers: body ? { "Content-Type": "application/json" } : undefined, body: body ? JSON.stringify(body) : undefined });
  const m = await r.json().catch(() => ({}));
  if (!r.ok) {
    const e = new Error(m.error ?? `terminumd answered ${r.status}`);
    e.status = r.status;
    throw e;
  }
  return m;
}

let toastTimer = 0;
/** One short message, where the action was taken cannot hold it. */
export function say(text, ms = 4000) {
  const t = $("toast");
  t.textContent = text;
  t.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => { t.hidden = true; }, ms);
}

/** Yes or no in the page's dialog. */
export function ask(title, more, yes) {
  const box = $("confirmbox");
  $("confirmtitle").textContent = title;
  $("confirmtext").textContent = more ?? "";
  $("confirmtext").hidden = !more;
  $("confirmok").textContent = yes;
  $("confirmacts").querySelectorAll("[data-extra]").forEach((b) => b.remove());
  box.returnValue = "";
  box.showModal();
  return new Promise((resolve) => box.addEventListener("close", () => resolve(box.returnValue === "ok"), { once: true }));
}

/** Several ways on: the value of the button pressed, or null for Cancel and Esc. */
export function choose(title, more, options) {
  const box = $("confirmbox"), acts = $("confirmacts");
  $("confirmtitle").textContent = title;
  $("confirmtext").textContent = more ?? "";
  $("confirmtext").hidden = !more;
  $("confirmok").hidden = true;
  acts.querySelectorAll("[data-extra]").forEach((b) => b.remove());
  for (const [value, label] of options) {
    const b = el("button", null, label);
    b.type = "submit";
    b.value = value;
    b.dataset.extra = "1";
    acts.append(b);
  }
  box.returnValue = "";
  box.showModal();
  return new Promise((resolve) => box.addEventListener("close", () => {
    $("confirmok").hidden = false;
    resolve(box.returnValue && box.returnValue !== "cancel" ? box.returnValue : null);
  }, { once: true }));
}

/** A name in the page's dialog; `save` throwing keeps it open with terminumd's message. */
export function askName(text, value, yes, save, most = 40) {
  const box = $("namebox"), field = $("namefield"), error = $("nameerror");
  $("nametext").textContent = text;
  $("nameok").textContent = yes;
  field.value = value;
  field.maxLength = most;
  error.hidden = true;
  box.showModal();
  field.select();
  return new Promise((resolve) => {
    const go = async () => {
      try {
        await save(field.value.trim());
        box.close();
      } catch (err) {
        error.textContent = err.message;
        error.hidden = false;
      }
    };
    // Enter is the named button: the form's first submit button is Cancel
    const submit = (e) => { if (e.submitter?.value === "ok") { e.preventDefault(); go(); } };
    const key = (e) => { if (e.key === "Enter" && !e.isComposing) { e.preventDefault(); go(); } };
    const form = box.querySelector("form");
    form.addEventListener("submit", submit);
    field.addEventListener("keydown", key);
    box.addEventListener("close", () => { form.removeEventListener("submit", submit); field.removeEventListener("keydown", key); resolve(); }, { once: true });
  });
}

/** Keeps a file with terminumd: its {id, name, bytes, mime}. Over 100 MB is said before it is sent. */
export async function upload(file) {
  if (file.size > MOST_BYTES) throw new Error(`${file.name} is over 100 MB`);
  const r = await fetch(`/api/task-files?name=${encodeURIComponent(file.name)}`, { method: "POST", headers: { "Content-Type": "application/octet-stream" }, body: file });
  const m = await r.json().catch(() => ({}));
  if (!r.ok) throw new Error(m.error ?? `${file.name}: terminumd answered ${r.status}`);
  return m;
}

const fileUrl = (id) => `/api/task-files/${id}`;

/** Markdown of a kept file: an image shows, anything else links. */
export const fileMarkdown = (f) => `${f.mime?.startsWith("image/") && f.mime !== "image/svg+xml" ? "!" : ""}[${f.name.replace(/[[\]]/g, "")}](${fileUrl(f.id)})`;

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

let lib = null;
const renderers = () => (lib ??= import("/fb/renderers.js"));

/** Markdown by the Panel's rules (raw HTML off, javascript: links dropped, remote images not
 *  loaded); a task's own pictures are fetched with the sign-in, its files download. */
export async function renderInto(box, text) {
  box.textContent = text;
  try {
    const r = await renderers();
    box.innerHTML = r.taskMarkdownHtml(text);
  } catch { return; }
  for (const img of box.querySelectorAll("img[data-raw]")) {
    fetch(img.dataset.raw).then((r) => (r.ok ? r.blob() : null)).then((b) => { if (b) img.src = URL.createObjectURL(b); }).catch(() => {});
  }
  for (const a of box.querySelectorAll("a")) {
    const id = /^\/api\/task-files\/([0-9a-f]{32})$/.exec(a.getAttribute("href") ?? "")?.[1];
    if (id) a.addEventListener("click", (e) => { e.preventDefault(); download(id, a.textContent).catch((err) => say(err.message)); });
    else { a.target = "_blank"; a.rel = "noopener noreferrer"; }
  }
}
