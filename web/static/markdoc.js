// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// Many bookmarks or reminders at once (AC-91): the list's "Open all" shows them as one Markdown
// document, full screen, as View shows a file: each its note as a heading's text, its text
// quoted, its session, host and time; Esc or × closes it.
import { quote, when } from "./marks.js";
import { markdownInto } from "./widgetui.js";

const $ = (id) => document.getElementById(id);

/** The Markdown of `items` (bookmarks' wire form); alive(sid): a session's title now, or null. */
export function markdownOf(items, alive) {
  return items.map((b) => [`### ${(b.note || b.text.split("\n")[0]).replace(/\n/g, " ").slice(0, 200)}`, "", quote(b.text), "",
    `*${[(b.orphaned ? null : alive(b.sid)) ?? `${b.title} (closed)`, b.host, when(b.at)].filter(Boolean).join(" · ")}*`].join("\n")).join("\n\n");
}

export function markDoc({ alive }) {
  const box = $("markdoc");
  $("markdocclose").onclick = () => box.close();
  return {
    async open(items, title) {
      $("markdoctitle").textContent = `${title} · ${items.length}`;
      await markdownInto($("markdocbody"), markdownOf(items, alive));
      if (!box.open) box.showModal();
      $("markdocbody").scrollTop = 0;
    },
  };
}
