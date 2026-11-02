// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The web app's icons: Lucide (https://lucide.dev), drawn on its 24-unit grid with its 2-unit
// stroke. The path data below is from lucide-static 1.52.0, under this license:
//
//   ISC License. Copyright (c) for portions of Lucide are held by Cole Bemis 2013-2022 as part
//   of Feather (MIT). All other copyright (c) for Lucide are held by Lucide Contributors 2022.
//   Permission to use, copy, modify, and/or distribute this software for any purpose with or
//   without fee is hereby granted, provided that the above copyright notice and this permission
//   notice appear in all copies. THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL
//   WARRANTIES WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF MERCHANTABILITY
//   AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY SPECIAL, DIRECT, INDIRECT, OR
//   CONSEQUENTIAL DAMAGES OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS,
//   WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF OR IN
//   CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.

const SVG = "http://www.w3.org/2000/svg";

const FOLDER = "M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z";
const FRAME = "M5 3h14a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z";

// name -> shapes: a string is a path, [cx, cy, r] a circle, { fill } a filled path
const ICONS = {
  hourglass: ["M5 22h14", "M5 2h14", "M17 22v-4.172a2 2 0 0 0-.586-1.414L12 12l-4.414 4.414A2 2 0 0 0 7 17.828V22",
    "M7 2v4.172a2 2 0 0 0 .586 1.414L12 12l4.414-4.414A2 2 0 0 0 17 6.172V2"],
  "circle-help": [[12, 12, 10], "M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3", "M12 17h.01"],
  "triangle-alert": ["m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3", "M12 9v4", "M12 17h.01"],
  group: ["M3 7V5c0-1.1.9-2 2-2h2", "M17 3h2c1.1 0 2 .9 2 2v2", "M21 17v2c0 1.1-.9 2-2 2h-2", "M7 21H5c-1.1 0-2-.9-2-2v-2",
    "M8 7h5a1 1 0 0 1 1 1v3a1 1 0 0 1-1 1H8a1 1 0 0 1-1-1V8a1 1 0 0 1 1-1z", "M11 12h5a1 1 0 0 1 1 1v3a1 1 0 0 1-1 1h-5a1 1 0 0 1-1-1v-3a1 1 0 0 1 1-1z"],
  plus: ["M5 12h14", "M12 5v14"],
  check: ["M20 6 9 17l-5-5"],
  bell: ["M10.268 21a2 2 0 0 0 3.464 0", "M3.262 15.326A1 1 0 0 0 4 17h16a1 1 0 0 0 .74-1.673C19.41 13.956 18 12.499 18 8A6 6 0 0 0 6 8c0 4.499-1.411 5.956-2.738 7.326"],
  terminal: ["M12 19h8", "m4 17 6-6-6-6"],
  ellipsis: [[12, 12, 1], [19, 12, 1], [5, 12, 1]],
  pencil: ["M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z", "m15 5 4 4"],
  search: ["m21 21-4.34-4.34", [11, 11, 8]],
  "chevron-down": ["m6 9 6 6 6-6"],
  "chevron-up": ["m18 15-6-6-6 6"],
  bookmark: ["m19 21-7-4-7 4V5a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2v16z"],
  x: ["M18 6 6 18", "m6 6 12 12"],
  // the output widgets (AC-56); Lucide's rects written as paths
  copy: ["M10 8h10a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H10a2 2 0 0 1-2-2V10a2 2 0 0 1 2-2z", "M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"],
  "copy-plus": ["M15 12v6", "M12 15h6", "M10 8h10a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H10a2 2 0 0 1-2-2V10a2 2 0 0 1 2-2z", "M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"],
  "maximize-2": ["M15 3h6v6", "m21 3-7 7", "m3 21 7-7", "M9 21H3v-6"],
  // a host's other tmux sessions, to attach (AC-89)
  import: ["M12 3v12", "m8 11 4 4 4-4", "M8 5H4a2 2 0 0 0-2 2v10a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2V7a2 2 0 0 0-2-2h-4"],
  // the session list's menus (rowmenu.js)
  "folder-plus": ["M12 10v6", "M9 13h6", FOLDER],
  "folder-minus": ["M9 13h6", FOLDER],
  "list-checks": ["m3 17 2 2 4-4", "m3 7 2 2 4-4", "M13 6h8", "M13 12h8", "M13 18h8"],
  mail: ["m22 7-8.991 5.727a2 2 0 0 1-2.009 0L2 7", "M4 4h16a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2z"],
  "mail-open": ["M21.2 8.4c.5.38.8.97.8 1.6v10a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V10a2 2 0 0 1 .8-1.6l8-6a2 2 0 0 1 2.4 0l8 6Z", "m22 10-8.97 5.7a1.94 1.94 0 0 1-2.06 0L2 10"],
  history: ["M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8", "M3 3v5h5", "M12 7v5l4 2"],
  activity: ["M22 12h-2.48a2 2 0 0 0-1.93 1.46l-2.35 8.36a.25.25 0 0 1-.48 0L9.24 2.18a.25.25 0 0 0-.48 0l-2.35 8.36A2 2 0 0 1 4.49 12H2"],
  "arrow-up-to-line": ["M5 3h14", "m18 13-6-6-6 6", "M12 7v14"],
  "arrow-up": ["m5 12 7-7 7 7", "M12 19V5"],
  "arrow-down": ["M12 5v14", "m19 12-7 7-7-7"],
  "arrow-down-to-line": ["M12 17V3", "m6 11 6 6 6-6", "M19 21H5"],
  "arrow-right-left": ["m16 3 4 4-4 4", "M20 7H4", "m8 21-4-4 4-4", "M4 17h16"],
  "chevron-left": ["m15 18-6-6 6-6"],
  clipboard: ["M16 4h2a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h2", "M9 2h6a1 1 0 0 1 1 1v2a1 1 0 0 1-1 1H9a1 1 0 0 1-1-1V3a1 1 0 0 1 1-1z"],
  "undo-2": ["M9 14 4 9l5-5", "M4 9h10.5a5.5 5.5 0 0 1 5.5 5.5a5.5 5.5 0 0 1-5.5 5.5H11"],
  "redo-2": ["m15 14 5-5-5-5", "M20 9H9.5A5.5 5.5 0 0 0 4 14.5A5.5 5.5 0 0 0 9.5 20H13"],
  ungroup: ["M6 4h6a1 1 0 0 1 1 1v4a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1z", "M12 14h6a1 1 0 0 1 1 1v4a1 1 0 0 1-1 1h-6a1 1 0 0 1-1-1v-4a1 1 0 0 1 1-1z"],
  // a split of the terminal: the window, its divider and the new pane filled (terminum's own, on Lucide's grid)
  "split-bottom": [FRAME, "M3 12h18", { fill: "M3 12h18v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" }],
  "split-right": [FRAME, "M12 3v18", { fill: "M12 3h7a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2h-7z" }],
  "split-left": [FRAME, "M12 3v18", { fill: "M12 3H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h7z" }],
};

/** An inline Lucide icon at `size` px, drawn in the text color. */
export function icon(name, size = 16) {
  const svg = document.createElementNS(SVG, "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("class", "lu");
  svg.style.setProperty("--s", `${size}px`);         // the page's svg rule sizes by it
  svg.setAttribute("aria-hidden", "true");
  for (const s of ICONS[name] ?? []) {
    let el;
    if (typeof s === "string") { el = document.createElementNS(SVG, "path"); el.setAttribute("d", s); }
    else if (!Array.isArray(s)) { el = document.createElementNS(SVG, "path"); el.setAttribute("d", s.fill); el.setAttribute("class", "fill"); }
    else {
      el = document.createElementNS(SVG, "circle");
      [["cx", s[0]], ["cy", s[1]], ["r", s[2]]].forEach(([k, v]) => el.setAttribute(k, String(v)));
    }
    svg.append(el);
  }
  return svg;
}
