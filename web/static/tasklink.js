// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The ways between the session page and the board.
const SELECTION = "tm.taskselection";
const RECORD = "tm.taskrecord";

export const taskUrl = (id) => `/kanban/${id}`;

/** The board's new-task window, for a session and its project (AC-106). */
export const newTaskUrl = (session, project) => `/kanban/?new=1&session=${encodeURIComponent(session)}${project ? `&for=${encodeURIComponent(project)}` : ""}`;

/** A session's own address on its page: `#/<key>/`, as readable as a key can stay (AC-134). */
export const sessionHash = (key) => `#/${encodeURIComponent(key).replaceAll("%3A", ":")}/`;

/** The session page showing session `key`; `then` "plan": with its Plan pane open. */
export const sessionUrl = (key, then) => `/sessions/${then ? `?then=${then}` : ""}${sessionHash(key)}`;

/** The session this page's address names, if any. */
export function addressedSession() {
  const said = /^#\/(.+?)\/?$/.exec(location.hash)?.[1];
  try { return said ? decodeURIComponent(said) : null; } catch { return null; }
}

/** What that window takes, once; in private mode it opens empty. */
function leave(key, value) {
  try { localStorage.setItem(key, value); } catch { /* private mode */ }
}

function take(key) {
  try {
    const value = localStorage.getItem(key);
    localStorage.removeItem(key);
    return value;
  } catch { return null; }
}

export const leaveSelection = (text) => leave(SELECTION, text);
export const takeSelection = () => take(SELECTION) ?? "";

/** A bookmark or reminder to make a task of (terminum-kanban.md AC-116): its id, title and text. */
export const leaveRecord = (record) => leave(RECORD, JSON.stringify(record));
export function takeRecord() {
  try { return JSON.parse(take(RECORD) ?? "null"); } catch { return null; }
}
