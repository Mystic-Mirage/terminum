// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
/** The board's new-task window, for a session and its project (AC-106). */
export const newTaskUrl = (session, project) => `/kanban/?new=1&session=${encodeURIComponent(session)}${project ? `&for=${encodeURIComponent(project)}` : ""}`;
