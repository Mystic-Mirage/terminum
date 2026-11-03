// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// History messages held while the terminal is frozen (a selection, a finger down): appends are
// merged by range and lines older than the server's limit are dropped, so the queue holds at most
// one limit's worth of lines and the resume replays that much, however long the hold.

/** The queue q with message m added (q is not changed). */
export function queueHist(q, m) {
  const out = q.slice();
  const last = out.at(-1);
  if (m.mode === "append" && last?.mode === "append" && m.first <= last.first + last.lines.length && m.first >= last.first) {
    out[out.length - 1] = { ...m, first: last.first, lines: last.lines.slice(0, m.first - last.first).concat(m.lines) };
  } else out.push(m);
  // lines below the newest limit would only be trimmed again after the resume
  const oldest = m.oldest ?? 0;
  return out.flatMap((x, i) => {
    if (x.mode !== "append" || x.first >= oldest) return [x];
    const cut = Math.min(x.lines.length, oldest - x.first);
    if (cut === x.lines.length && out.slice(i + 1).some((y) => y.mode === "append")) return [];   // a later append sets the screen's top
    return [{ ...x, first: x.first + cut, lines: x.lines.slice(cut) }];
  });
}
