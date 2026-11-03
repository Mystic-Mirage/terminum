// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// Bookmarks and reminders belong to the project (AC-92) their session was in when they were
// made, while it lives, else to that session alone (AC-33, AC-91); terminumd orphans them once
// that project went (deleted, or its last session closed) or, outside projects, that session closed. Here: which session shows one, and which ones
// none of the listed sessions shows.

/** Pane id -> the id of its project, of terminumd's projects. */
export const projectsOf = (projects) => new Map(projects.flatMap((p) => p.members.map((k) => [k, p.id])));

/** Whether bookmark `b` shows in pane `id`, whose project is `project` (none outside projects). */
export const shownIn = (b, id, project) => !b.orphaned && (b.project ? b.project === project : b.sid === id);

/** Whether bookmark `b` shows in no pane of `panes` (layout rows; `of`: projectsOf): orphaned, or
 *  none of its sessions is listed now (its host out of reach). */
export const nowhere = (b, panes, of) => !panes.some((it) => shownIn(b, it.id, of.get(it.id)));
