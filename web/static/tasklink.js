// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The way from the session page to the board.
const SELECTION = "tm.taskselection";

export const taskUrl = (id) => `/kanban/${id}`;

/** The board's new-task window, for a session and its project (AC-106). */
export const newTaskUrl = (session, project) => `/kanban/?new=1&session=${encodeURIComponent(session)}${project ? `&for=${encodeURIComponent(project)}` : ""}`;

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
