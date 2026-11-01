// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// A pane's diagnostics (AC-100): the row menu's "Diagnostics…" asks terminumd for tmux's own
// fields of the pane (size, cursor, alternate screen, mouse modes, history, command, its folder
// with ~ for the home folder) and shows one plain text for a bug report in the Last Lines window:
// terminum's build, this browser and its window, the row as the list has it (its title is the
// program's), then tmux's fields; its Copy copies it (Safari copies only on a press, never when
// an answer arrives). No screen lines, history or sign-in go into it.

const ROW = ["title", "agent", "state", "host", "alt", "shell", "window", "index", "pane"];   // the folder: tmux's, with ~

/** The text of terminumd's answer `m` ({id, build, server, tmux}) for row `it` (or none), seen
 *  from `env` ({ua, w, h, dpr}: this browser and its window). */
export function diagText(m, it, env) {
  const row = it ? ROW.filter((k) => it[k] !== undefined && it[k] !== null && it[k] !== "").map((k) => `  ${k}: ${it[k]}`) : ["  (not in the list)"];
  const tmux = Object.entries(m.tmux ?? {}).map(([k, v]) => `  ${k}: ${v === "" ? "-" : v}`);
  return ["terminum diagnostics", `build: ${m.build ?? "?"}`, `pane: ${m.id} (${m.server ?? "?"})`,
    `browser: ${env.ua}`, `window: ${env.w}×${env.h} at ${env.dpr}×`, "row:", ...row, "tmux:", ...tmux].join("\n");
}

export const CHECK = "Check it before you share it: it holds the pane's folder, host and title.";

/** send(m); show(heading, meta, text): the window that shows it; say(text): a toast; item(id). */
export function diagnostics({ send, show, say, item }) {
  let asked = null;
  return {
    open(id) { asked = id; send({ t: "diag", id }); },
    /** terminumd's answer: {id, build, server, tmux} or {id, error}. */
    seen(m) {
      if (m.t !== "diag" || m.id !== asked) return;
      asked = null;
      if (m.error) return say(`No diagnostics: ${m.error}`);
      show(`Diagnostics · ${item(m.id)?.title ?? m.id}`, CHECK,
        diagText(m, item(m.id), { ua: navigator.userAgent, w: innerWidth, h: innerHeight, dpr: devicePixelRatio }));
    },
  };
}
