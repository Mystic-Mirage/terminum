// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// What the session list shares with the board, with nothing of the list behind it.
import { icon } from "./icons.js";
import { textForm } from "./render.js";

/** A JSON value kept on this browser; in private mode, this page's only. */
export function read(key) { try { return JSON.parse(localStorage.getItem(key) ?? "null"); } catch { return null; } }
export function write(key, v) { try { if (v) localStorage.setItem(key, JSON.stringify(v)); else localStorage.removeItem(key); } catch { /* private mode */ } }

export function el(tag, cls, text) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = textForm(String(text));
  return e;
}

/** A small icon button: its class, icon, label for a screen reader, tooltip. */
export function iconButton(cls, name, label, title, onClick) {
  const b = el("button", cls);
  b.type = "button";
  b.title = title;
  b.setAttribute("aria-label", label);
  b.append(icon(name, 14));
  b.addEventListener("click", (e) => { e.stopPropagation(); onClick(b); });
  return b;
}

// Projects' colors (AC-92): ten hues far apart, in pastel (tokens.css), so two projects never read as one
export const PROJECT_HUES = [25, 145, 265, 85, 205, 325, 55, 175, 295, 115];
export const hueColor = (h) => `oklch(var(--proj-l) var(--proj-c) ${h})`;
export const projectHue = (p) => PROJECT_HUES[(p.color ?? 0) % PROJECT_HUES.length];
export const projectColor = (p) => hueColor(projectHue(p));
/** Default has no color of its own; its sessions keep their folders'. */
export const colored = (p) => p?.color >= 0;
/** A project's color for words and dots, Default's the muted. */
export const projectInk = (p) => (colored(p) ? projectColor(p) : "var(--muted)");
