// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// Every browser check of the web page (web/static), against fakes of terminumd played in the
// browser (no server, no tmux), in Chromium and in WebKit; the checks that depend on the size
// run at a wide (1400×900) and a phone (390×844, touch) size themselves.
//
//   node web/test/e2e_all.mjs [chromium|webkit|firefox] [check…]     (needs `cd ui && npm ci && npm run build`)
// Firefox only when named (`node ui/node_modules/playwright-core/cli.js install firefox`), and
// only for the checks that run without a touch screen (FIREFOX_TOO).

import { check, playwright, results, runningIn } from "./check.mjs";
import { webApp } from "./e2e_app.mjs";
import { webBoxNav } from "./e2e_boxnav.mjs";
import { webClip } from "./e2e_clip.mjs";
import { webCodex } from "./e2e_codex.mjs";
import { webCtxTrace } from "./e2e_ctxtrace.mjs";
import { webEdit } from "./e2e_edit.mjs";
import { webLook } from "./e2e_look.mjs";
import { webMarks } from "./e2e_marks.mjs";
import { webProgCopy } from "./e2e_progcopy.mjs";
import { webMotion } from "./e2e_motion.mjs";
import { webMove } from "./e2e_move.mjs";
import { webScreen } from "./e2e_screen.mjs";
import { webSignin } from "./e2e_signin.mjs";
import { webSent } from "./e2e_sent.mjs";
import { webSheets } from "./e2e_sheets.mjs";
import { webFold } from "./e2e_fold.mjs";
import { webHeader } from "./e2e_header.mjs";
import { webNotes } from "./e2e_notes.mjs";
import { webSplit } from "./e2e_split.mjs";
import { webTerm } from "./e2e_term.mjs";
import { webTmux } from "./e2e_tmux.mjs";
import { webTouch } from "./e2e_touch.mjs";
import { webTrace } from "./e2e_trace.mjs";
import { webWidgets } from "./e2e_widgets.mjs";
import { webWidgetFolds } from "./e2e_widgetfolds.mjs";
import { webWidgetTables } from "./e2e_widgettables.mjs";
import { webWidgetPng } from "./e2e_widgetpng.mjs";
import { webWidgetSteady } from "./e2e_widgetsteady.mjs";
import { webTableForms } from "./e2e_tableforms.mjs";
import { webDialogCenter } from "./e2e_dialogcenter.mjs";
import { webIdeKeys } from "./e2e_idekeys.mjs";
import { webScreenFolds } from "./e2e_screenfolds.mjs";
import { webProjects } from "./e2e_projects.mjs";
import { webListDrag } from "./e2e_listdrag.mjs";
import { webPhoneRow } from "./e2e_phonerow.mjs";
import { webRows } from "./e2e_rows.mjs";
import { webResize } from "./e2e_resize.mjs";
import { webOpenEnd } from "./e2e_openend.mjs";
import { webSwitch } from "./e2e_switch.mjs";
import { webDock } from "./e2e_dock.mjs";
import { webFoot } from "./e2e_foot.mjs";
import { webMarksScope } from "./e2e_marksscope.mjs";
import { webListOrder } from "./e2e_listorder.mjs";
import { webPlans } from "./e2e_plans.mjs";
import { webLastLines } from "./e2e_lastlines.mjs";
import { webPlansFind } from "./e2e_plansfind.mjs";
import { webPathFold } from "./e2e_pathfold.mjs";
import { webMarkNav } from "./e2e_marknav.mjs";
import { webPaneWin } from "./e2e_panewin.mjs";
import { webReview } from "./e2e_review_regressions.mjs";
import { webTips } from "./e2e_tips.mjs";
import { webHold } from "./e2e_hold.mjs";
import { webListPlace } from "./e2e_listplace.mjs";

const CHECKS = { webTerm, webApp, webMove, webTrace, webCtxTrace, webClip, webWidgets, webWidgetFolds, webWidgetTables, webWidgetPng, webWidgetSteady, webTableForms, webDialogCenter, webMarks, webProgCopy, webTouch, webEdit, webBoxNav,
  webSignin, webTmux, webScreen, webLook, webMotion, webCodex, webSplit, webSent, webIdeKeys, webScreenFolds, webProjects, webListDrag, webPhoneRow, webRows, webResize, webOpenEnd, webSwitch, webDock, webSheets, webFold, webHeader, webNotes, webFoot, webMarksScope, webListOrder, webPlans, webLastLines, webPlansFind, webPathFold, webMarkNav, webPaneWin, webReview, webTips, webHold, webListPlace };
const BROWSERS = ["chromium", "webkit"], KNOWN = [...BROWSERS, "firefox"];
// Checks a user saw fail in Firefox and that need no touch screen (Firefox has no isMobile).
const FIREFOX_TOO = ["webProgCopy"];
// Checks that drive the page with Chromium's own tools: a CDP touch screen, synthetic Touch events.
const CHROMIUM_ONLY = { webClip: "synthetic Touch events", webTouch: "a CDP touch screen" };

const args = process.argv.slice(2);
const browsers = args.filter((a) => KNOWN.includes(a));
const only = args.filter((a) => !KNOWN.includes(a));
const unknown = only.filter((a) => !Object.hasOwn(CHECKS, a));
if (unknown.length) {
  console.error(`unknown check: ${unknown.join(", ")}\nchecks: ${Object.keys(CHECKS).join(" ")}`);
  process.exit(2);
}
const pw = await playwright();
for (const name of browsers.length ? browsers : BROWSERS) {
  const browser = await pw[name].launch();
  try {
    for (const [which, run] of Object.entries(CHECKS)) {
      if (only.length && !only.includes(which)) continue;
      if (name === "firefox" && !FIREFOX_TOO.includes(which)) continue;
      if (name !== "chromium" && CHROMIUM_ONLY[which]) { console.log(`SKIP [${name}] ${which}: needs ${CHROMIUM_ONLY[which]}`); continue; }
      runningIn(name);
      try { await run(browser, name); } catch (e) { check(`${which} ran to its end`, false, String(e.stack ?? e).split("\n").slice(0, 3).join(" | ")); }
      for (const ctx of browser.contexts()) await ctx.close();   // a page a failed check left open
    }
  } finally { await browser.close(); }
}
runningIn("");
const by = new Map();
for (const r of results) {
  const k = r.where.trim() || "-";
  const n = by.get(k) ?? { pass: 0, fail: 0 };
  n[r.ok ? "pass" : "fail"]++;
  by.set(k, n);
}
for (const [k, n] of by) console.log(`${k}: ${n.pass} passed, ${n.fail} failed`);
const failed = results.filter((r) => !r.ok).length;
// A run that checked nothing proves nothing: it fails, never "PASS (0 checks)".
if (!results.length) { console.log("FAIL (0 checks ran)"); process.exit(1); }
console.log(failed ? `FAIL (${failed} of ${results.length})` : `PASS (${results.length} checks)`);
process.exit(failed ? 1 : 0);
