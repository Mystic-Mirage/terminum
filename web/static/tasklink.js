// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The ways between the session page and the board.
import { read, write } from "./listparts.js";

const SELECTION = "tm.taskselection";

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

/** Text for that window to take, once; in private mode it opens empty. */
export const leaveSelection = (text) => write(SELECTION, text);

export function takeSelection() {
  const text = read(SELECTION) ?? "";
  write(SELECTION, null);
  return text;
}
