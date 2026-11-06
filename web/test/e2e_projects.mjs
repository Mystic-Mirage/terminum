// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The session list's rows and projects against a fake terminumd (fake.mjs): each row's Clone
// (AC-07) and its menu on a right click or a long press (Clone, Add to Project, Close, moves),
// projects as groups of their own in their colors with [+], ⋯ Rename (AC-92), the unread mark
// (AC-93), the row's program at the right of its second line, ⌘Z and ⌘⇧Z (AC-03), and the
// search's magnifier out of sight while its field is open.

import { check, runningIn } from "./check.mjs";
import { drawn, group, item, openPage, pause, PHONE, WIDE } from "./fake.mjs";

const ID = (n) => `tmux:local:terminum:%${n}`;
const LAYOUT = [group("main", [item(ID(1), "api", { sub: ["claude", "~/work/api"], dir: "/Users/alex/work/api" }),
  item(ID(2), "docs", { index: "2", sub: ["zsh", "~/work/docs"] }), item(ID(4), "tests", { index: "3", sub: ["zsh", "~/work/api"], unread: true })], { wid: "local/main", new: "New window" }),
  group("notes", [item(ID(3), "notes", { sub: ["fish", "~/notes"] })], { wid: "local/notes", new: "New window" })];
const P1 = { id: "p1", name: "Group 1", color: 0, host: null, dir: "/Users/alex/work/api", members: [ID(1), ID(3)] };
const P2 = { id: "p2", name: "Group 2", color: 1, host: null, dir: "/Users/alex/work/docs", members: [ID(2)] };
const menuItems = (page) => page.evaluate(() => [...document.querySelectorAll("#rowmenu[open] button:not([data-action=x])")].filter((b) => b.getClientRects().length)   // shown: a phone has no Split Right or Left
  .map((b) => `${b.getAttribute("aria-label") ?? b.textContent}${b.disabled ? "-" : ""}`));   // a split's word is drawn: its name in its label
const last = (sent, t) => sent.filter((m) => m.t === t).pop();

export async function webProjects(browser, name) {
  runningIn(`${name} 1400×900`);
  const f = await openPage(browser, { device: WIDE, layout: LAYOUT });
  const { page, sent } = f;
  await drawn(page);
  const look = await page.evaluate(() => {
    const row = document.querySelector('#sessions .item[data-id$="%2"]');
    const s = row.querySelector(".s");
    return { news: document.querySelectorAll("#sessions h2 .new").length, clones: document.querySelectorAll("#sessions li > .clone").length,
      first: s.firstElementChild?.textContent, prog: s.querySelector(".progs")?.textContent,
      right: Math.round(s.getBoundingClientRect().right - s.querySelector(".progs").getBoundingClientRect().right),
      unread: !!document.querySelector('#sessions .item.unread[data-id$="%4"]'), plain: !!document.querySelector('#sessions .item.unread[data-id$="%2"]') };
  });
  check("AC-07 every row has Clone, every tmux group's header New Window (AC-92)", look.news === 2 && look.clones === 4, JSON.stringify(look));
  check("AC-01 a row's second line: its folder first, its program at the right edge", look.first === "~/work/docs" && look.prog === "zsh" && look.right <= 1, JSON.stringify(look));
  check("AC-93 a row whose agent finished unseen is unread, others not", look.unread && !look.plain, JSON.stringify(look));
  // a tmux session's group: its menu on a right click on its header or its ⋯ (New Window, moves among
  // the groups, Close Group, which asks first)
  const groupAt = (wid) => page.click(`#sessions section[data-wid="${wid}"] h2 .label`, { button: "right" });
  const order = () => page.evaluate(() => [...document.querySelectorAll("#sessions section.grp[data-wid]")].map((s) => s.dataset.wid).join());
  await groupAt("local/main");
  const gmenu = await menuItems(page);
  await page.click('#rowmenu button[data-action="down"]');
  await pause(page, 150);
  const downed = await order();
  await groupAt("local/main");
  await page.click('#rowmenu button[data-action="top"]');          // back, for the checks after
  await pause(page, 150);
  await page.hover('#sessions section[data-wid="local/main"] h2 .label');
  await page.click('#sessions section[data-wid="local/main"] h2 .hostbtn');
  const byDots = await menuItems(page);
  await page.click('#rowmenu button[data-action="new"]');
  const gnew = last(sent, "clone");
  const gkills = sent.filter((m) => m.t === "kill").length;
  await groupAt("local/notes");
  await page.click('#rowmenu button[data-action="close"]');
  await page.waitForSelector("#confirmbox[open]", { timeout: 3000 }).catch(() => {});
  const gasks = await page.evaluate(() => document.getElementById("confirmtitle").textContent);
  await page.click("#confirmok");
  await pause(page, 100);
  const gkilled = sent.filter((m) => m.t === "kill").slice(gkills).map((m) => m.id);
  check("AC-07 a session group's menu (a right click on its header, or its ⋯): New Window, Rename, moves among the groups, Close Group (asked first)",
    JSON.stringify(gmenu) === JSON.stringify(["New Window", "Rename…", "Move to the Top-", "Move Up-", "Move Down", "Move to the Bottom", "Close Group…"]) && downed === "local/notes,local/main"
    && await order() === "local/main,local/notes" && JSON.stringify(byDots) === JSON.stringify(gmenu) && JSON.stringify(gnew) === JSON.stringify({ t: "clone", id: ID(4) })
    && gasks === "Close the group notes and its session?" && JSON.stringify(gkilled) === JSON.stringify([ID(3)]), JSON.stringify({ gmenu, downed, byDots, gnew, gasks, gkilled }));
  // a collapsed group keeps its actions: its menu reads its sessions from the list, not from its drawn rows
  const fold = () => page.click('#sessions section[data-wid="local/main"] h2 .toggle');
  await fold();
  await pause(page, 100);
  await groupAt("local/main");
  const folded = { menu: await menuItems(page), rows: await page.evaluate(() => document.querySelectorAll('#sessions section[data-wid="local/main"] li').length) };
  await page.click('#rowmenu button[data-action="new"]');
  folded.cloned = last(sent, "clone");
  await fold();
  await pause(page, 100);
  check("AC-07 a collapsed session group's menu keeps New Window and Close Group",
    folded.rows === 0 && JSON.stringify(folded.menu) === JSON.stringify(gmenu) && JSON.stringify(folded.cloned) === JSON.stringify({ t: "clone", id: ID(4) }), JSON.stringify(folded));
  // Rename… names the group's tmux session; the group keeps its place among the groups and its fold
  await fold();
  await pause(page, 100);
  await groupAt("local/main");
  await page.click('#rowmenu button[data-action="rename"]');
  await page.waitForSelector("#namebox[open]", { timeout: 3000 }).catch(() => {});
  const naming = await page.evaluate(() => ({ value: document.getElementById("namefield").value, text: document.getElementById("nametext").textContent }));
  await page.fill("#namefield", "  work  ");
  await page.click("#nameok");
  await pause(page, 100);
  const regrouped = { naming, sent: last(sent, "renamegroup"), order: last(sent, "project")?.ids,
    folded: await page.evaluate(() => JSON.parse(localStorage.getItem("collapsed") ?? "[]")) };
  await fold();
  await pause(page, 100);
  check("AC-07 a session group's Rename… asks with its name and names its tmux session; its place and fold are kept",
    naming.value === "main" && /Rename main/.test(naming.text) && JSON.stringify(regrouped.sent) === JSON.stringify({ t: "renamegroup", group: "local/main", name: "work" })
      && regrouped.order?.indexOf("g:local/work") === regrouped.order.indexOf("g:local/main") - 1 && regrouped.folded.some((k) => k.includes("\nwork\n")), JSON.stringify(regrouped));
  // the unread row's hatching: pale lines of whole pixels over its whole box (a layer under its text), the blob gone; `ink` the lines' color
  const mark = (pg = page) => pg.evaluate(() => {
    const b = document.querySelector('#sessions .item[data-id$="%4"]'), h = getComputedStyle(b, "::before"), r = b.getBoundingClientRect();
    const mask = h.maskImage && h.maskImage !== "none" ? h.maskImage : h.webkitMaskImage;
    const whole = h.position === "absolute" && Math.abs(parseFloat(h.width) - r.width) < 1 && Math.abs(parseFloat(h.height) - r.height) < 1;
    return { hatched: /crispEdges/.test(mask ?? "") && whole && h.zIndex === "-1", ink: h.backgroundColor, said: b.getAttribute("aria-description"), blob: !!document.querySelector("#sessions .unread:not(.item)") };
  });
  const clone = (pg, n) => pg.evaluate((n) => getComputedStyle(document.querySelector(`#sessions li:has(> .item[data-id$="%${n}"]) > .clone`)).opacity, n);
  const m1 = await mark();
  const clone0 = await clone(page, 2);
  await page.hover('#sessions .item[data-id$="%4"]');
  const hovered = await mark();
  await page.mouse.move(700, 700);
  check("AC-93 an unread row is hatched over its whole box, hover or not, and says so to a screen reader; no blob",
    m1.hatched && hovered.ink === m1.ink && m1.said === "Unread: its agent finished" && !m1.blob, JSON.stringify({ m1, hovered }));
  await page.hover('#sessions .item[data-id$="%2"]');
  const clone1 = await clone(page, 2), other = await clone(page, 1);
  check("AC-07 Clone shows only while the pointer is over its row", clone0 === "0" && clone1 === "0.8" && other === "0", JSON.stringify({ clone0, clone1, other }));
  // a header: its count ends where the rows' programs end; its buttons show while the pointer is over it, in the count's place
  const head = (pg = page) => pg.evaluate(() => {
    const h = document.querySelector('#sessions section[data-wid="local/main"] h2'), c = h.querySelector(".count"), a = h.querySelector(".acts");
    const right = (e) => Math.round(e.getBoundingClientRect().right);
    return { count: c.offsetParent ? right(c) : null, progs: [...document.querySelectorAll("#sessions .progs")].map(right), acts: getComputedStyle(a).opacity,
      under: getComputedStyle(a).pointerEvents === "none" };
  });
  await page.mouse.move(700, 700);
  const rest = await head();
  await page.hover('#sessions section[data-wid="local/main"] h2 .label');
  const over = await head();
  await page.mouse.move(700, 700);
  check("AC-01 a group's count ends where its rows' programs end; its buttons out of sight and out of reach",
    rest.count !== null && rest.progs.every((r) => r === rest.count) && rest.acts === "0" && rest.under, JSON.stringify(rest));
  check("AC-01 the pointer over a header shows its buttons in the count's place", over.count === null && over.acts === "1" && !over.under, JSON.stringify(over));
  await page.hover('#sessions .item[data-id$="%2"]');
  await page.click('#sessions li:has(> .item[data-id$="%2"]) > .clone');
  check("AC-07 Clone asks for a window beside it", last(sent, "clone")?.id === ID(2), JSON.stringify(last(sent, "clone")));
  if (name === "chromium") {                          // WebKit's Tab skips buttons (macOS's default), and a script's focus after a click is no keyboard's
    await page.mouse.move(700, 700);
    await page.focus('#sessions .item[data-id$="%1"]');
    await page.keyboard.press("Tab");                // the keyboard's focus on its Clone
    const keyed = await clone(page, 1);
    await page.evaluate(() => document.activeElement.blur());
    check("AC-07 Clone shows with the keyboard's focus on it, no pointer over its row", keyed === "1", keyed);
  }

  await page.click('#sessions .item[data-id$="%2"]', { button: "right" });
  const items = await menuItems(page);
  const focused = await page.evaluate(() => document.activeElement?.dataset.action);
  check("AC-07 the row's menu opens with its focus on Clone, never on Close Session above it (Enter asks nothing)", focused === "clone", String(focused));
  check("AC-07 a right click opens the row's menu, Close Session first; the moves its place allows",
    items.join() === "Close Session,Clone,Add to Project…,Plan and Notes…,Mark as Unread,Diagnostics…,Split Below,Split Right,Split Left,Move to the Top,Move Up,Move Down,Move to the Bottom,Move to Session…", items.join());
  // each item with its icon, Close Session's a red ×; the three splits side by side, each drawn
  const menuLook = await page.evaluate(() => {
    const shown = [...document.querySelectorAll("#rowmenu[open] button:not([data-action=x])")].filter((b) => b.getClientRects().length);
    const close = document.querySelector('#rowmenu button[data-action="close"]'), x = close.querySelector("svg");
    const sp = [...document.querySelectorAll("#rowmenu .splits button")].map((b) => b.getBoundingClientRect());
    return { bare: shown.filter((b) => !b.querySelector(":scope > svg")).map((b) => b.textContent),
      red: getComputedStyle(x).color === getComputedStyle(close).color && getComputedStyle(close).color !== getComputedStyle(document.querySelector('#rowmenu button[data-action="clone"]')).color,
      row: sp.length === 3 && sp.every((r) => Math.abs(r.top - sp[0].top) < 1 && r.width > 40) && sp[0].right <= sp[1].left && sp[1].right <= sp[2].left,
      drawn: [...document.querySelectorAll("#rowmenu .splits svg")].every((s) => s.querySelector(".fill")) };
  });
  check("AC-07 every item of the row's menu has its icon, Close Session a red ×; Split Below, Right and Left in one row, each a drawing of its split",
    !menuLook.bare.length && menuLook.red && menuLook.row && menuLook.drawn, JSON.stringify(menuLook));
  // Mark as Unread: terminumd marks it on every page; an unread row's menu has Mark as Read (AC-93)
  let k0 = sent.length;
  await page.click('#rowmenu button[data-action="unread"]');
  const markedUnread = sent.slice(k0).find((m) => m.t === "unread");
  await page.click('#sessions .item[data-id$="%4"]', { button: "right" });
  const readItem = await page.$eval('#rowmenu button[data-action="unread"]', (b) => b.textContent);
  k0 = sent.length;
  await page.click('#rowmenu button[data-action="unread"]');
  const back = sent.slice(k0).find((m) => m.t === "unread");
  check("AC-93 Mark as Unread asks terminumd to mark the row; an unread row's menu has Mark as Read, which takes it away",
    markedUnread?.id?.endsWith("%2") && markedUnread.on === true && readItem === "Mark as Read" && back?.id?.endsWith("%4") && back.on === false && await page.isHidden("#rowmenu"), JSON.stringify({ markedUnread, readItem, back }));
  await page.click('#sessions .item[data-id$="%2"]', { button: "right" });   // its menu again, for the checks below
  check("AC-07 the menu names its session and has × to close it", await page.evaluate(() => document.querySelector("#rowmenu .mhead span")?.textContent === "docs"));
  await page.click('#rowmenu button[data-action="x"]');
  check("AC-07 × closes the menu", await page.isHidden("#rowmenu"));
  // modal: the page shaded, a press outside closes it and reaches nothing under it
  await page.click('#sessions .item[data-id$="%2"]', { button: "right" });
  const shade = await page.evaluate(() => getComputedStyle(document.getElementById("rowmenu"), "::backdrop").backgroundColor);
  const subs = sent.filter((m) => m.t === "sub").length;
  const r = await page.evaluate(() => { const b = document.querySelector('#sessions .item[data-id$="%1"]').getBoundingClientRect(); return [b.left + 40, b.top + 10]; });
  await page.mouse.click(r[0], r[1]);
  await pause(page, 150);
  check("AC-07 the menu is modal: the page shaded; a press outside closes it and opens nothing",
    !/rgba\(0, 0, 0, 0\)|transparent/.test(shade) && await page.isHidden("#rowmenu") && sent.filter((m) => m.t === "sub").length === subs, JSON.stringify({ shade, subs }));
  await page.click('#sessions .item[data-id$="%2"]', { button: "right" });
  await page.click('#rowmenu button[data-action="down"]');
  check("AC-07 Move Down reorders the session's windows", JSON.stringify(last(sent, "reorder")?.ids) === JSON.stringify([ID(1), ID(4), ID(2)]), JSON.stringify(last(sent, "reorder")));
  await page.click('#sessions .item[data-id$="%2"]', { button: "right" });
  await page.click('#rowmenu button[data-action="project"]');
  await page.click('#rowmenu button[data-action="newproject"]');
  await page.waitForSelector("#namebox[open]", { timeout: 3000 });
  const asked = await page.evaluate(() => ({ value: document.getElementById("namefield").value, yes: document.getElementById("nameok").textContent,
    border: getComputedStyle(document.getElementById("namebox")).borderTopWidth, pad: getComputedStyle(document.getElementById("namefield")).paddingLeft }));
  await page.fill("#namefield", "Rasp");
  await page.press("#namefield", "Enter");
  await pause(page, 100);
  check("AC-92 New project… asks its name at once (Group 1 suggested), Enter creates it",
    asked.value === "Group 1" && asked.yes === "Create" && JSON.stringify(last(sent, "project")) === JSON.stringify({ t: "project", action: "add", id: ID(2), name: "Rasp" }), JSON.stringify({ asked, m: last(sent, "project") }));
  check("AC-92 the name dialog looks as the page's dialogs (no frame, a padded field)", asked.border === "0px" && asked.pad === "12px", JSON.stringify(asked));

  f.say({ t: "projects", items: [P1, P2] });
  await pause(page, 200);
  const grouped = await page.evaluate(() => {
    const secs = [...document.querySelectorAll("#sessions section")];
    const p = secs[0], ids = (sec) => [...sec.querySelectorAll(".item")].map((i) => i.dataset.id.split(":").pop());
    const bars = [...p.querySelectorAll(".pbar")].map((b) => b.style.getPropertyValue("--dot"));
    return { first: p.dataset.pid, name: p.querySelector("h2 .label").textContent, rows: ids(p), main: ids(secs.find((s) => s.dataset.wid === "local/main")),
      notes: !!secs.find((s) => s.dataset.wid === "local/notes"), same: new Set(bars).size === 1,
      mark: [!!p.querySelector("h2 .toggle > .gkind.isproject"), !p.querySelector("h2 .toggle > .gkind svg"), !!secs.find((s) => s.dataset.wid).querySelector("h2 .toggle > .gkind.istmux svg"),
        getComputedStyle(p.querySelector("h2 .gkind.isproject"), "::before").width === "4px",
        (() => { const g = secs.find((s) => s.dataset.wid), c = (e) => getComputedStyle(e).color; return c(g.querySelector("h2 .gkind svg")) === c(g.querySelector("h2 .label")); })()],
      apart: bars[0] !== secs[1].querySelector(".pbar").style.getPropertyValue("--dot"),
      // the space under its last session and above its name, from its tinted box's edges
      pad: (() => { const r = (e) => e.getBoundingClientRect(), lis = p.querySelectorAll("li");
        return [Math.round(r(p.querySelector("h2 .label")).top - r(p).top), Math.round(r(p).bottom - r(lis[lis.length - 1]).bottom)]; })() };
  });
  check("AC-92 projects come first, their sessions out of their tmux groups; a group left empty goes",
    grouped.first === "p1" && grouped.name === "Group 1" && grouped.rows.join() === "%1,%3" && grouped.main.join() === "%4" && !grouped.notes, JSON.stringify(grouped));
  check("AC-92 a project's rows share its color, another project's differs", grouped.same && grouped.apart, JSON.stringify(grouped));
  check("AC-92 left of the name a project has its dot, 4 px, a tmux group its >_ in the name's color", grouped.mark.every(Boolean), JSON.stringify(grouped.mark));
  check("AC-92 under a project's last session as much space as above its name", grouped.pad[0] > 0 && grouped.pad[0] === grouped.pad[1], JSON.stringify(grouped.pad));
  // a project with no session stands, its [+] asks for the folder once (AC-110)
  f.say({ t: "projects", items: [P1, P2, { id: "p9", name: "Billing", color: 4, host: null, dir: "", members: [] }] });
  await pause(page, 200);
  const emptyRow = await page.evaluate(() => { const s = document.querySelector('#sessions section[data-pid="p9"]'); return s && { name: s.querySelector("h2 .label").textContent, rows: s.querySelectorAll(".item").length }; });
  check("AC-110 a project with no session stands in the list", emptyRow?.name === "Billing" && emptyRow.rows === 0, JSON.stringify(emptyRow));
  await page.hover('#sessions section[data-pid="p9"] > h2');
  await page.click('#sessions section[data-pid="p9"] > h2 .new');
  await page.fill("#namefield", "/Users/alex/billing");
  await page.keyboard.press("Enter");
  await pause(page, 100);
  check("AC-110 its [+] asks for the folder and opens a window there", JSON.stringify(last(sent, "projectwindow")) === JSON.stringify({ t: "projectwindow", project: "p9", dir: "/Users/alex/billing" }), JSON.stringify(last(sent, "projectwindow")));
  f.say({ t: "projects", items: [{ ...P1, members: [ID(1), ID(3), ID(4)] }, P2] });
  await pause(page, 200);
  const m2 = await mark();
  check("AC-93 in a project the hatching takes the project's name color", m2.hatched && m2.ink !== m1.ink, JSON.stringify({ m1, m2 }));
  f.say({ t: "projects", items: [P1, P2] });
  await pause(page, 200);
  await page.hover('#sessions section[data-pid="p1"] h2 .label');   // its buttons show under the pointer
  await page.click('#sessions section[data-pid="p1"] h2 .new');
  check("AC-92 a project's [+] asks for a window after its last session, in the project",
    JSON.stringify(last(sent, "clone")) === JSON.stringify({ t: "clone", id: ID(3), project: "p1" }), JSON.stringify(last(sent, "clone")));
  await page.hover('#sessions section[data-pid="p1"] h2 .label');   // its buttons show under the pointer
  await page.click('#sessions section[data-pid="p1"] h2 .hostbtn');
  await page.click('#rowmenu button[data-action="rename"]');
  await page.fill("#namefield", "API");
  await page.press("#namefield", "Enter");          // Enter renames (the form's first button is Cancel)
  await pause(page, 100);                          // the dialog's close comes after the click
  check("AC-92 ⋯ Rename… names the project", JSON.stringify(last(sent, "project")) === JSON.stringify({ t: "project", action: "rename", project: "p1", name: "API" }), JSON.stringify(last(sent, "project")));
  await page.dblclick('#sessions section[data-pid="p2"] h2 .label');
  await page.waitForSelector("#namebox[open]", { timeout: 3000 });
  await page.fill("#namefield", "Docs");
  await page.click("#nameok");
  await pause(page, 100);
  const byDouble = last(sent, "project");
  check("AC-92 a double click on a project's name renames it too", byDouble?.action === "rename" && byDouble.project === "p2" && byDouble.name === "Docs", JSON.stringify(byDouble));
  // a right click on a project's header: its menu as a row's (New Window, Rename, moves among the
  // projects, Delete Project, Close Project, which asks first and closes every session of it)
  const headAt = (pid) => page.click(`#sessions section[data-pid="${pid}"] h2 .label`, { button: "right" });
  await headAt("p1");
  const pmenu = await menuItems(page);
  await page.click('#rowmenu button[data-action="down"]');
  const moved = last(sent, "project");
  await headAt("p2");
  const second = await menuItems(page);
  await page.keyboard.press("Escape");
  await headAt("p1");
  await page.click('#rowmenu button[data-action="new"]');
  const made = last(sent, "clone");
  check("AC-92 a right click on a project's header opens its menu: New Window, Rename, moves in the list, Delete Project, Close Project",
    JSON.stringify(pmenu) === JSON.stringify(["New Window", "Rename…", "Move to the Top-", "Move Up-", "Move Down", "Move to the Bottom", "Delete Project…", "Close Project…"])
    && JSON.stringify(moved?.ids?.slice(0, 2)) === JSON.stringify(["p:p2", "p:p1"]) && moved.action === "order" && second.includes("Move Up-") && second.includes("Move Down")
    && JSON.stringify(made) === JSON.stringify({ t: "clone", id: ID(3), project: "p1" }), JSON.stringify({ pmenu, moved, second, made }));
  const kills = sent.filter((m) => m.t === "kill").length;
  await headAt("p1");
  await page.click('#rowmenu button[data-action="close"]');
  await page.waitForSelector("#confirmbox[open]", { timeout: 3000 }).catch(() => {});
  const asks = await page.evaluate(() => `${document.getElementById("confirmtitle").textContent} ${document.getElementById("confirmtext").textContent}`);
  await page.click('#confirmbox button[value="cancel"]');
  const kept = sent.filter((m) => m.t === "kill").length === kills;
  await headAt("p1");
  await page.click('#rowmenu button[data-action="close"]');
  await page.waitForSelector("#confirmbox[open]", { timeout: 3000 }).catch(() => {});
  await page.click("#confirmok");
  await pause(page, 100);
  const killed = sent.filter((m) => m.t === "kill").slice(kills).map((m) => m.id);
  check("AC-92 Close Project asks first (Cancel closes nothing), then closes each of its sessions",
    /^Close the project .+ and its 2 sessions\? The programs running in them end\.$/.test(asks) && kept && JSON.stringify(killed) === JSON.stringify([ID(1), ID(3)]), JSON.stringify({ asks, kept, killed }));
  await page.click('#sessions .item[data-id$="%3"]', { button: "right" });
  const inProject = await menuItems(page);
  await page.click('#rowmenu button[data-action="up"]');
  check("AC-92 a project's session moves in the project; its menu removes it from the project",
    inProject.includes("Remove from Group 1") && JSON.stringify(last(sent, "project")?.keys) === JSON.stringify([ID(3), ID(1)]), JSON.stringify({ inProject, m: last(sent, "project") }));

  await page.click('#sessions .item[data-id$="%3"]');
  await pause(page, 200);
  await page.click("#term");
  const n = sent.filter((m) => m.t === "in").length;
  await page.keyboard.press("Meta+KeyZ");
  await page.keyboard.press("Meta+Shift+KeyZ");
  await pause(page, 150);
  const keys = sent.filter((m) => m.t === "in").slice(n).map((m) => m.data);
  check("AC-03 ⌘Z sends the program's undo (Ctrl+_), ⌘⇧Z fish's redo (Alt+/)", keys.join() === "\x1f,\x1b/", JSON.stringify(keys));
  // with nothing holding the keys (a selection made with the pointer leaves them on the page) ⌘Z still undoes
  await page.evaluate(() => document.activeElement?.blur());
  const n2 = sent.filter((m) => m.t === "in").length;
  await page.keyboard.press("Meta+KeyZ");
  await pause(page, 150);
  const free = sent.filter((m) => m.t === "in").slice(n2).map((m) => m.data);
  check("AC-03 ⌘Z with nothing holding the keys still sends the program's undo", free.join() === "\x1f", JSON.stringify(free));
  await page.click('#sessions .item[data-id$="%4"]');
  await pause(page, 200);
  await page.click("#term");
  await page.keyboard.press("Meta+Shift+KeyZ");
  await pause(page, 150);
  check("AC-03 ⌘⇧Z where the program has no redo says so", (await page.textContent("#toast")).includes("zsh has no redo"));

  await page.click("#findfab");
  await pause(page, 150);
  // the magnifier stays in sight, ringed as the sent history's button is when open; the field beside it,
  // its row level with the magnifier's middle; every pane 14 px left of its button, its first row so
  const magnifier = await page.evaluate(() => {
    const g = (id) => document.getElementById(id), r = (id) => g(id).getBoundingClientRect(), mid = (b) => Math.round(b.top + b.height / 2);
    return { open: !g("findbox").hidden, seen: getComputedStyle(g("findfab")).visibility, ring: getComputedStyle(g("findfab")).boxShadow,
      level: mid(r("findfab")) - mid(r("findq")), gap: Math.round(r("findfab").left - r("findbox").right), others: getComputedStyle(g("sentfab")).visibility };
  });
  await page.click("#findfab");
  await page.click("#sentfab");
  await pause(page, 200);
  const sentRing = await page.evaluate(() => getComputedStyle(document.getElementById("sentfab")).boxShadow);
  // the sent pane is a window as View's (e2e_panewin.mjs): 14 px left of the floating buttons, 12 px under the terminal's top
  const sentLevel = await page.evaluate(() => { const r = (e) => e.getBoundingClientRect(), f = r(document.getElementById("sentfab")), p = r(document.getElementById("sentpane")), t = r(document.getElementById("term"));
    return { gap: Math.round(f.left - p.right), top: Math.round(p.top - t.top) }; });
  await page.click("#sentfab");
  check("the search's magnifier stays in sight while its field is open, ringed as the sent history's button; the field 14 px left of its button, level with it; the sent pane a window 14 px left of the buttons, 12 px under the terminal's top",
    magnifier.open && magnifier.seen === "visible" && magnifier.ring === sentRing && magnifier.ring !== "none" && Math.abs(magnifier.level) <= 1 && magnifier.gap === 14 && magnifier.others === "visible" && sentLevel.gap === 14 && sentLevel.top === 12,
    JSON.stringify({ magnifier, sentRing, sentLevel }));
  check("no page errors", !f.errors.length, f.errors.join(" | "));
  await f.close();

  runningIn(`${name} 390×844`);
  const p = await openPage(browser, { device: PHONE, layout: LAYOUT });
  await drawn(p.page);
  await p.page.click("#navbtn");
  await pause(p.page, 300);
  await p.page.evaluate(() => {
    const el = document.querySelector('#sessions .item[data-id$="%2"]'), r = el.getBoundingClientRect();
    el.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true, pointerType: "touch", clientX: r.left + 40, clientY: r.top + 10 }));
  });
  await pause(p.page, 450);
  await p.page.evaluate(() => {
    const el = document.querySelector('#sessions .item[data-id$="%2"]'), r = el.getBoundingClientRect();
    el.dispatchEvent(new PointerEvent("pointerup", { bubbles: true, pointerType: "touch", clientX: r.left + 40, clientY: r.top + 10 }));
  });
  await pause(p.page, 100);
  const held = await menuItems(p.page);
  // the lift's click lands on what is under the finger now, the menu's first items among them
  const under = await p.page.evaluate(() => {
    const r = document.querySelector('#sessions .item[data-id$="%2"]').getBoundingClientRect(), b = document.querySelector("#rowmenu[open] [data-action=close]")?.getBoundingClientRect();
    for (const [x, y] of [[r.left + 40, r.top + 10], ...(b ? [[b.left + b.width / 2, b.top + b.height / 2]] : [])]) document.elementFromPoint(x, y)?.click();
    return !!b;
  });
  await pause(p.page, 100);
  check("AC-07 the finger's lift asks nothing: Close Session, now first, stays unpressed", under && !await p.page.evaluate(() => document.getElementById("confirmbox").open));
  check("AC-07 a finger resting on a row and lifted without moving opens its menu", held[0] === "Close Session" && held[1] === "Clone" && held.includes("Split Below") && !held.includes("Split Right") && held.includes("Plan and Notes…") && held.includes("Diagnostics…") && held.length === 12, held.join());
  const pm = await mark(p.page);
  check("AC-93 on a phone too an unread row is hatched", pm.hatched && !pm.blob, JSON.stringify(pm));
  check("AC-07 on a phone (no hover) Clone is always there", await clone(p.page, 2) === "0.8");
  const ph = await head(p.page);
  check("AC-01 on a phone (no hover) a header's buttons are always there, its count before them", ph.acts === "1" && ph.count !== null, JSON.stringify(ph));
  check("no page errors", !p.errors.length, p.errors.join(" | "));
  await p.close();
}
