// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// What the Message history (/history/) and the Dashboard (/dashboard/) share (terminum.md AC-151,
// AC-152): the sign-in (kanbanutil.js's api), the page's theme, the user's stylesheet, the URL's state
// and the words for days and times. The page draws; terminumd counts.
import * as signin from "./signin.js";
import { pageColors } from "./contrast.js";
import { withScheme } from "./schemes.js";
import { api, signInElsewhere } from "./kanbanutil.js";
import { el, projectInk } from "./listparts.js";

export { api };
export const $ = (id) => document.getElementById(id);

/** The filters' projects, the first day kept, the theme; the page's colors from it. */
export async function loadMeta() {
  const meta = await api("/api/messages/meta");
  if (meta.theme) pageColors(withScheme(meta.theme, meta.scheme));
  return meta;
}

signInElsewhere();                                 // with no sign-in the page goes to /sessions/, whose form signs in
signin.showKeptCss();
signin.loadCss();

/** A project's ink from the session list's color (Default and gone ones: the muted). */
export const inkOf = (p) => projectInk(p && p.color !== null && p.color !== undefined ? { color: p.color } : null);
/** The project of messages sent in no project, in both pages' addresses (terminumd's search.rs NONE). */
export const NONE = "-";
/** A project's name; one the session list no longer has, or none. */
export const projectLabel = (p) => p?.name || (p?.id ? "Gone project" : "No project");
/** A dot in `color`. */
export function dotOf(color) {
  const d = el("span", "pdot");
  d.style.setProperty("--proj", color);
  return d;
}

/** The single choice menu of a `.pick`: rows {label, on, pick, color?}; a choice closes it. */
export function menu(details, rows) {
  const box = details.querySelector(".box > .rows") ?? details.querySelector(".box");
  box.replaceChildren(...rows.map(({ label, on, pick, color }) => {
    const b = el("button");
    b.type = "button";
    b.setAttribute("role", "menuitemradio");
    b.setAttribute("aria-checked", String(on));
    if (color) b.append(dotOf(color));
    b.append(el("span", "pname", label));
    b.addEventListener("click", () => { details.open = false; pick(); });
    return b;
  }));
}

/** The address's query as an object, and back: only what is set, in a fixed order, so links compare equal. */
export function readQuery(keys) {
  const q = new URLSearchParams(location.search);
  return Object.fromEntries(keys.map((k) => [k, q.get(k) ?? ""]));
}
export function writeQuery(state, keys, push) {
  const q = new URLSearchParams();
  for (const k of keys) if (state[k]) q.set(k, state[k]);
  const url = `${location.pathname}${q.size ? `?${q}` : ""}`;
  if (url !== `${location.pathname}${location.search}`) history[push ? "pushState" : "replaceState"](null, "", url);
}

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const WEEKDAYS = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
export const WEEK = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/** A `YYYY-MM-DD` as a Date at its local noon (no zone shifts it to another day). */
export const dayDate = (d) => new Date(`${d}T12:00:00`);
/** `YYYY-MM-DD` of a Date, local. */
export const dayOf = (t) => `${t.getFullYear()}-${String(t.getMonth() + 1).padStart(2, "0")}-${String(t.getDate()).padStart(2, "0")}`;
export const today = () => dayOf(new Date());
export const addDays = (d, n) => { const t = dayDate(d); t.setDate(t.getDate() + n); return dayOf(t); };

/** "Today", "Yesterday", "Mon 2 Nov", "Mon 2 Nov 2025". */
export function dayLabel(d) {
  if (d === today()) return "Today";
  if (d === addDays(today(), -1)) return "Yesterday";
  const t = dayDate(d);
  const year = t.getFullYear() === new Date().getFullYear() ? "" : ` ${t.getFullYear()}`;
  return `${WEEKDAYS[t.getDay()]} ${t.getDate()} ${MONTHS[t.getMonth()]}${year}`;
}
/** "2 Nov" */
export const shortDay = (d) => { const t = dayDate(d); return `${t.getDate()} ${MONTHS[t.getMonth()]}`; };
export const monthOf = (d) => MONTHS[dayDate(d).getMonth()];

const hm = (t) => clock(t.getHours() * 60 + t.getMinutes());
/** A message's time: "14:32" today, "Tue 14:32" this week, "3 Nov 14:32", "3 Nov 2025". */
export function timeLabel(ms) {
  const t = new Date(ms), d = dayOf(t), age = (Date.now() - ms) / 86_400_000;
  if (d === today()) return hm(t);
  if (age < 6) return `${WEEKDAYS[t.getDay()]} ${hm(t)}`;
  if (t.getFullYear() === new Date().getFullYear()) return `${shortDay(d)} ${hm(t)}`;
  return `${shortDay(d)} ${t.getFullYear()}`;
}

/** Minutes of a day as "09:40". */
export const clock = (min) => `${String(Math.floor(min / 60) % 24).padStart(2, "0")}:${String(min % 60).padStart(2, "0")}`;
/** Milliseconds as "4.2 h" (active time is a lower bound: a tenth of an hour, never minutes). */
export const hours = (ms) => (ms >= 3_600_000 ? `${(ms / 3_600_000).toFixed(1)} h` : `${Math.round(ms / 60_000)} min`);
/** A count, with thousands kept short past 9,999. */
export const count = (n) => (n > 9999 ? `${(n / 1000).toFixed(1)}k` : n.toLocaleString("en-US"));
