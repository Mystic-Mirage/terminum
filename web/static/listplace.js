// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// Where the shown session stands in the list, kept for when it closes (AC-92): the page then shows
// the first session of the same project or tmux group, else the top of the list. terminumd tells
// the list without a closed pane before the projects without it, so that list finds no place and the
// one kept stands.

/** sid(): the shown session; parts(): the list's parts as drawn (listParts in nav.js).
 *  {note(), next()}. */
export function listPlace({ sid, parts }) {
  let place;
  return {
    /** A list or the projects came, or a session was just shown: its place, when the list has it. */
    note() { place = parts().find((it) => it.rows.some((row) => row.id === sid()))?.place ?? place; },
    /** The session to show now that the shown one is gone; none when the list is empty. */
    next() {
      const all = parts(), at = all.find((it) => it.place === place) ?? all[0];
      return (at?.tops[0] ?? at?.rows[0])?.id;
    },
  };
}
