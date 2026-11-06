// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The ways between the session page and the board.
const SELECTION = "tm.taskselection";

export const taskUrl = (id) => `/kanban/${id}`;

/** The board's new-task window, for a session and its project (AC-106). */
export const newTaskUrl = (session, project) => `/kanban/?new=1&session=${encodeURIComponent(session)}${project ? `&for=${encodeURIComponent(project)}` : ""}`;

/** The session page showing session `key` (terminum-kanban.md AC-134). */
export const sessionUrl = (key) => `/sessions/?open=${encodeURIComponent(key)}`;

/** The session this page's address asks for, once: the address goes back to the page's. */
export function askedSession() {
  const asked = new URLSearchParams(location.search), key = asked.get("open");
  if (key) {
    asked.delete("open");
    history.replaceState(null, "", `${location.pathname}${asked.size ? `?${asked}` : ""}`);
  }
  return key;
}

/** Text for that window to take, once; in private mode it opens empty. */
export function leaveSelection(text) {
  try { localStorage.setItem(SELECTION, text); } catch { /* private mode */ }
}

export function takeSelection() {
  try {
    const text = localStorage.getItem(SELECTION) ?? "";
    localStorage.removeItem(SELECTION);
    return text;
  } catch { return ""; }
}
