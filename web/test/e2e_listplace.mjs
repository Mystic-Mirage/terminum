// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// AC-92 against a fake terminumd (fake.mjs): when the shown session closes the page shows the first
// session of its project, outside projects of its tmux group, else the top of the list as drawn;
// terminumd tells the list without a closed pane, then the projects without it.

import { check, runningIn } from "./check.mjs";
import { drawn, group, item, openPage, pause, SIZES } from "./fake.mjs";

const ID = (n) => `tmux:local:terminum:%${n}`;
const row = (n) => item(ID(n), `s${n}`, { index: String(n) });
const main = (...ns) => group("main", ns.map(row));
const notes = (...ns) => group("notes", ns.map(row));
const project = (id, ...ns) => ({ id, name: id, color: 0, host: null, dir: "/Users/alex", members: ns.map(ID) });

export async function webListPlace(browser, name) {
  for (const [size, device] of SIZES) {
    runningIn(`${name} ${size}`);
    const f = await openPage(browser, { device, layout: [main(1, 2), notes(3, 4)] });
    const { page, sent, say } = f;
    await drawn(page);
    const shown = () => sent.filter((m) => m.t === "sub").pop()?.id;
    const listed = async (...groups) => { say({ t: "layout", groups }); await pause(page, 100); };
    /** With `layout` and `projects` shown on `from`: terminumd tells its list `left`, then the projects `after` if they changed. */
    const closes = async ({ layout, projects, from, after, left }) => {
      say({ t: "projects", items: projects });
      say({ t: "layout", groups: layout });
      await pause(page, 60);
      await page.evaluate((id) => { document.querySelector(`#sessions .item[data-id="${id}"]`)?.click(); }, from);
      await pause(page, 60);
      const was = shown();
      say({ t: "layout", groups: left });
      await pause(page, 30);
      if (after) say({ t: "projects", items: after });
      await pause(page, 100);
      return { was, now: shown() };
    };

    // terminumd's list starts with main: its first row was shown, whatever the closed session's place
    const inProject = await closes({ layout: [main(1, 2), notes(3, 4)], projects: [project("p1", 2, 3, 4)], from: ID(2),
      after: [project("p1", 3, 4)], left: [main(1), notes(3, 4)] });
    check("AC-92 a project's shown session closes: its project's first session",
      inProject.was === ID(2) && inProject.now === ID(3), JSON.stringify(inProject));
    const inProjectLate = await closes({ layout: [main(1, 2), notes(3, 4)], projects: [project("p1", 2, 3, 4)], from: ID(2),
      left: [main(1), notes(3, 4)] });
    check("AC-92 the same while its project keeps it (a member added in the last 5 s)", inProjectLate.now === ID(3), JSON.stringify(inProjectLate));
    const inGroup = await closes({ layout: [main(1), notes(3, 4)], projects: [], from: ID(4), left: [main(1), notes(3)] });
    check("AC-92 outside projects: its tmux group's first session, not the list's", inGroup.was === ID(4) && inGroup.now === ID(3), JSON.stringify(inGroup));
    const onlyOne = await closes({ layout: [main(1, 2), notes(3, 4)], projects: [project("p1", 2), project("p2", 3, 4)], from: ID(2),
      after: [project("p2", 3, 4)], left: [main(1), notes(3, 4)] });
    check("AC-92 a project's only session closes: the first session at the top of the list as drawn (the next project's), not terminumd's first",
      onlyOne.was === ID(2) && onlyOne.now === ID(3), JSON.stringify(onlyOne));
    const groupGone = await closes({ layout: [main(1), notes(3)], projects: [project("p2", 1)], from: ID(3), left: [main(1)] });
    check("AC-92 a group's only session closes: the top of the list", groupGone.was === ID(3) && groupGone.now === ID(1), JSON.stringify(groupGone));
    const out = await closes({ layout: [main(1, 2), notes(3)], projects: [project("p1", 1, 2)], from: ID(1), after: [project("p1", 2)], left: [main(1, 2), notes(3)] });
    check("AC-92 a session taken out of its project and still listed stays shown", out.now === ID(1), JSON.stringify(out));
    await listed(main(2), notes(3));
    check("AC-92 a session taken out of its project closes: its tmux group's first", shown() === ID(2), shown());
    await closes({ layout: [main(1, 2), notes(3)], projects: [], from: ID(1), after: [project("p1", 1, 3)], left: [main(1, 2), notes(3)] });
    await listed(main(2), notes(3));
    check("AC-92 a session added to a project closes: that project's first", shown() === ID(3), shown());
    await closes({ layout: [main(1), notes(3)], projects: [], from: ID(3), left: [main(1)] });
    await listed();
    check("no page errors (the shown session closes)", !f.errors.length, f.errors.join(" | "));
    await f.close();
  }
  runningIn(name);
}
