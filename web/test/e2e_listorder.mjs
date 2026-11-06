// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// AC-92 against a fake terminumd (fake.mjs): projects and tmux groups stand in one order: the menus'
// moves and a drag by the header go past either kind, terminumd is told, every page follows; a project's
// header and a tmux group's have the same New Window, a plain [+], their first button.
import { check, runningIn } from "./check.mjs";
import { drawn, group, item, openPage, pause, SIZES } from "./fake.mjs";

const key = (p) => `tmux:local:terminum:%${p}`;
const LAYOUT = [group("main", [item(key(1), "api"), item(key(2), "docs", { index: "2" })]), group("notes", [item(key(3), "notes")]), group("work", [item(key(4), "work")])];
const PROJECT = { id: "p1", name: "Docs", color: 0, host: null, dir: "~/docs", members: [key(2)] };
const parts = (page) => page.evaluate(() => [...document.querySelectorAll("#sessions section.grp[data-part]")].map((s) => s.dataset.part).join());
const lastOrder = (f) => f.sent.filter((m) => m.t === "project" && m.action === "order").at(-1)?.ids;
const menu = async (page, part, action) => {
  await page.evaluate((p) => {
    const h = document.querySelector(`#sessions section[data-part="${p}"] > h2`), r = h.getBoundingClientRect();
    h.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: r.left + 20, clientY: r.top + 5 }));
  }, part);
  await page.click(`#rowmenu button[data-action="${action}"]`);
  await pause(page, 120);
};

export async function webListOrder(browser, name) {
  for (const [size, device] of SIZES) {
    const at = `(${name} ${size})`;
    runningIn(`${name} ${size}`);
    const f = await openPage(browser, { device, layout: LAYOUT });
    const { page } = f;
    await drawn(page);
    f.say({ t: "projects", items: [PROJECT], order: [] });
    await pause(page, 150);
    check(`AC-92 ${at} before any move the project stands first`, (await parts(page)) === "p:p1,g:local/main,g:local/notes,g:local/work", await parts(page));
    await menu(page, "g:local/work", "top");
    check(`AC-92 ${at} a tmux group's Move to the Top stands above the project; terminumd is told`,
      (await parts(page)) === "g:local/work,p:p1,g:local/main,g:local/notes" && lastOrder(f)?.join() === "g:local/work,p:p1,g:local/main,g:local/notes", `${await parts(page)} ${lastOrder(f)}`);
    await menu(page, "p:p1", "bottom");
    check(`AC-92 ${at} a project's Move to the Bottom stands under the groups`, (await parts(page)) === "g:local/work,g:local/main,g:local/notes,p:p1", await parts(page));
    await menu(page, "p:p1", "up");
    check(`AC-92 ${at} Move Up goes past a group`, (await parts(page)) === "g:local/work,g:local/main,p:p1,g:local/notes", await parts(page));
    f.say({ t: "projects", items: [PROJECT], order: ["g:local/notes", "p:p1", "g:local/work", "g:local/main"] });
    await pause(page, 150);
    check(`AC-92 ${at} another page's move is followed`, (await parts(page)) === "g:local/notes,p:p1,g:local/work,g:local/main", await parts(page));
    if (size.startsWith("1400")) {
      // a drag by the header, past a project
      const box = (p) => page.evaluate((s) => { const r = document.querySelector(`#sessions section[data-part="${s}"] > h2 .label`).getBoundingClientRect(); return { x: r.left + 5, y: r.top + r.height / 2 }; }, p);
      const [from, to] = [await box("g:local/main"), await box("g:local/notes")];
      await page.mouse.move(from.x, from.y); await page.mouse.down(); await page.mouse.move(from.x, from.y - 20, { steps: 3 }); await page.mouse.move(to.x, to.y - 4, { steps: 8 }); await page.mouse.up();
      await pause(page, 200);
      check(`AC-92 ${at} a tmux group dragged by its header goes above a project`, (await parts(page)).startsWith("g:local/main,") && lastOrder(f)?.[0] === "g:local/main", await parts(page));
    }
    // a filter on: the parts it hides stay where they are
    const order0 = await parts(page);
    await page.fill("#filter", "notes").catch(() => {});
    await pause(page, 150);
    const filtered = await parts(page);
    if (filtered === "g:local/notes") {
      await page.fill("#filter", "o");           // two parts drawn at least
      await pause(page, 150);
    }
    const shown = (await parts(page)).split(",");
    if (shown.length >= 2) {
      await menu(page, shown.at(-1), "up");
      const sent = lastOrder(f), slots = order0.split(",").map((id, i) => (shown.includes(id) ? i : -1)).filter((i) => i >= 0);
      check(`AC-92 ${at} under a filter a move keeps the parts it hides in their places`, order0.split(",").every((id, i) => slots.includes(i) || sent[i] === id), JSON.stringify({ order0, shown, sent }));
    }
    await page.fill("#filter", "");
    await pause(page, 150);
    // a rename terminumd refuses: the group stays where it was
    f.say({ t: "projects", items: [PROJECT], order: order0.split(",") });
    await pause(page, 120);
    const beforeName = await parts(page);
    await menu(page, "g:local/notes", "rename");
    await page.waitForSelector("#namebox[open]", { timeout: 3000 });
    await page.fill("#namefield", "taken");
    await page.click("#nameok");
    await pause(page, 150);
    const afterName = await parts(page);
    check(`AC-92 ${at} a rename not done yet or refused leaves the group in its place`, afterName === beforeName && lastOrder(f).includes("g:local/taken") && lastOrder(f).includes("g:local/notes")
      && lastOrder(f).indexOf("g:local/taken") === lastOrder(f).indexOf("g:local/notes") - 1, JSON.stringify({ beforeName, afterName, sent: lastOrder(f) }));
    // the hue of a group right over a project is not the project's
    f.say({ t: "projects", items: [PROJECT], order: ["g:local/work", "p:p1", "g:local/main", "g:local/notes"] });
    await pause(page, 150);
    const hues = await page.evaluate(() => [...document.querySelectorAll("#sessions section.grp[data-part]")].slice(0, 2).map((s) => s.style.getPropertyValue("--proj")));
    check(`AC-92 ${at} a tmux group moved to the top right over a project has another color`, hues.length === 2 && hues[0] !== hues[1], JSON.stringify(hues));
    // the same New Window button
    const look = await page.evaluate(() => {
      const of = (p) => { const b = document.querySelector(`#sessions section[data-part="${p}"] > h2 .new`), s = b && getComputedStyle(b); return b && { cls: b.className, first: !b.previousElementSibling, w: s.width, h: s.height, label: b.getAttribute("aria-label"), path: [...b.querySelectorAll("path")].map((x) => x.getAttribute("d")).join("|") }; };
      return { project: of("p:p1"), session: of("g:local/notes") };
    });
    check(`AC-92 ${at} a project's header and a tmux group's have the same New Window: one plain [+], their first button`,
      !!look.project && !!look.session && look.project.cls === look.session.cls && look.project.w === look.session.w && look.project.h === look.session.h && look.project.first && look.session.first
      && look.project.label === "New window in Docs" && look.session.label === "New window in notes" && look.project.path === look.session.path && look.project.path === "M5 12h14|M12 5v14", JSON.stringify(look));
    const before = f.sent.length;
    await page.evaluate(() => document.querySelector('#sessions section[data-part="g:local/notes"] > h2 .new').click());
    await page.evaluate(() => document.querySelector('#sessions section[data-part="p:p1"] > h2 .new').click());
    await pause(page, 100);
    const made = f.sent.slice(before).filter((m) => m.t === "clone");
    check(`AC-92 ${at} each makes a window after its last session`, made.length === 2 && made[0].id === key(3) && !made[0].project && made[1].id === key(2) && made[1].project === "p1", JSON.stringify(made));
    // a group with no session: a new window in that tmux session (an attached one stays listed empty)
    f.say({ t: "layout", groups: [...LAYOUT, group("empty", [], { attached: "empty" })] });
    await pause(page, 150);
    const asked = f.sent.length;
    await page.evaluate(() => document.querySelector('#sessions section[data-part="g:local/empty"] > h2 .new')?.click());
    await pause(page, 100);
    const made0 = f.sent.slice(asked).find((m) => m.t === "new");
    check(`AC-92 ${at} a group with no session: a new window in its tmux session`, made0?.group === "local/empty", JSON.stringify(made0));
    check(`no page error ${at}`, f.errors.length === 0, f.errors.join("; "));
    await f.context.close();
  }
}
