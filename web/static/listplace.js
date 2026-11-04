// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// Where the shown session stands in the list, kept for when it closes (AC-92): the page then shows
// the first session of the same project or tmux group, else the top of the list. terminumd says that
// a closed pane left its project before it sends the list without that pane, so the place is taken
// from a list that still has the pane, and a "projects" that takes it out of its project changes
// nothing (a taken-out session stands in its group from the next list on).

/** sid(): the shown session; parts(): the list's parts as drawn (listParts in nav.js).
 *  {note(), joined(), next()}. */
export function listPlace({ sid, parts }) {
  let place;
  const part = () => parts().find((it) => it.rows.some((row) => row.id === sid()));
  return {
    /** A list came, or a session was just shown: its place, when the list has it. */
    note() { place = part()?.place ?? place; },
    /** A "projects" came: only a project the session is in now moves it (added, or moved to another). */
    joined() { const now = part(); if (now?.project) place = now.place; },
    /** The session to show now that the shown one is gone; none when the list is empty. */
    next() {
      const all = parts(), at = all.find((it) => it.place === place) ?? all[0];
      return (at?.tops[0] ?? at?.rows[0])?.id;
    },
  };
}
