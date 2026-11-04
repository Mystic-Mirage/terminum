// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The web page's browser checks share these helpers: a check prints PASS or FAIL with the
// browser and size it ran in (e2e_all.mjs sets them), and results counts them per run.

import os from "node:os";

/** How much later than on a quiet machine timers and frames may come: the load per core, 1 to
 *  4, or TM_TEST_SLACK. A check of timing waits and budgets by `slack(ms)`; a quiet machine keeps
 *  them exact, a busy one (another build, another suite) no longer fails them for its load. */
export const SLACK = Number(process.env.TM_TEST_SLACK) || Math.min(4, Math.max(1, os.loadavg()[0] / os.cpus().length));
export const slack = (ms) => Math.round(ms * SLACK);
if (SLACK > 1) console.log(`timing slack ×${SLACK.toFixed(1)} (load ${os.loadavg()[0].toFixed(0)} on ${os.cpus().length} cores)`);

export const results = [];
let where = "";
/** The browser and size the next checks run in, e.g. "webkit 390×844"; "" for none. */
export const runningIn = (label) => { where = label ? `[${label}] ` : ""; };
export const check = (name, ok, extra = "") => {
  results.push({ ok: !!ok, where });
  console.log(`${ok ? "PASS" : "FAIL"} ${where}${name}${extra ? " — " + extra : ""}`);
};
export const within = async (name, fn, ms = 5000) => {
  const t0 = Date.now();
  try { await fn(ms); check(name, true, `${Date.now() - t0} ms`); } catch (e) { check(name, false, e.message.split("\n")[0]); }
};
/** In the page (page.evaluate's function): the browser's focus and visibility played, then `ev`
    fired where the browser fires it (visibilitychange on the document); a test cannot take the
    window's focus away. */
export function playLook([focused, hidden, ev]) {
  document.hasFocus = () => focused;
  Object.defineProperty(document, "hidden", { get: () => hidden, configurable: true });
  (ev === "visibilitychange" ? document : window).dispatchEvent(new Event(ev));
}
/** Nothing to stop: the web checks run against fakes in the browser (no server, no tmux). */
export const cleanup = () => {};
/** playwright-core, ui/'s devDependency (PLAYWRIGHT_CORE may name another copy). */
export const playwright = () => import(process.env.PLAYWRIGHT_CORE ?? new URL("../../ui/node_modules/playwright-core/index.mjs", import.meta.url).href);
