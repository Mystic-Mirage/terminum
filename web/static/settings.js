// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The user's customization from terminumd (AC-18, AC-19): settings.json's page defaults (text
// size, widgets, folds) for a browser with no choice of its own, its messages
// (a wrong setting) as toasts, each once, the hot keys layout, and ~/.terminum/custom.css,
// after the page's own stylesheets, fetched with the sign-in (signin.js: it is the user's,
// never served without one) and again when terminumd says it changed. The copy last fetched is
// shown at once at load (it came after the connection: the page drew a moment without it).
import { hotRows } from "./hotkeys.js";
import { loadCss, showKeptCss } from "./signin.js";

const SAY_FOR = 4000;        // ms a message stays

/** toast(text, ms); setLayout(rows | null): the hot keys panel; onPage(page): the defaults changed. */
export function pageSettings({ toast, setLayout, onPage }) {
  let page = {}, css, loaded = false;
  showKeptCss();
  const said = new Set(), queue = [];
  let saying = false;

  function say(text) {
    if (!text || said.has(text)) return;
    said.add(text);
    queue.push(text);
    if (!saying) next();
  }
  function next() {
    const text = queue.shift();
    saying = !!text;
    if (!text) return;
    toast(text, SAY_FOR);
    setTimeout(next, SAY_FOR);
  }

  /** A `settings` message: {page, hotkeys: {rows, error}, messages, css}. */
  function apply(m) {
    page = m.page && typeof m.page === "object" ? m.page : {};
    onPage(page);
    for (const text of [...(m.messages ?? []), ...(page.messages ?? [])]) say(String(text));
    if (m.hotkeys) {
      const got = m.hotkeys.error ? { error: m.hotkeys.error } : m.hotkeys.rows ? hotRows(m.hotkeys.rows) : { rows: null };
      if (got.error) say(String(got.error));
      setLayout(got.error ? null : got.rows);                       // a wrong file: the default panel stays
    }
    if (!loaded || (m.css !== undefined && m.css !== css)) {
      css = m.css;
      loaded = true;
      loadCss();
    }
  }
  return { apply, page: () => page };
}
