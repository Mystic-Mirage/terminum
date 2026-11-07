// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The board against a real private terminumd and tmux (e2e_live.mjs; terminum-kanban.md AC-105 to
// AC-111, AC-113): `node web/test/e2e_live.mjs board chromium` runs this alone.
import fs from "node:fs";
import path from "node:path";
import { check, until, within } from "./check.mjs";

const PNG = Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==", "base64");

export async function liveBoard(page, { base, tmux, work, shots }) {
  await sameKinds(page, base, false);
  const card = (id) => `.card[data-id="${id}"]`;
  const inColumn = (status) => page.evaluate((s) => [...document.querySelectorAll(`.col[data-status="${s}"] .card`)].map((c) => c.dataset.id), status);
  const make = async (title, { severity, text } = {}) => {
    await page.click("#newtask");
    await page.waitForSelector("#newbox[open]");
    await page.fill("#newtitle", title);
    if (text !== undefined) await page.fill("#newtext", text);
    if (severity) await page.selectOption("#newsev", severity);
    await page.click("#newcreate");
    await page.waitForSelector("#newbox", { state: "hidden", timeout: 5000 });
  };
  // a card comes when terminumd told the page: it takes a moment after its window closed
  const idOf = async (title) => {
    let id;
    await until(async () => (id = await page.evaluate((t) => [...document.querySelectorAll(".card")].find((c) => c.querySelector(".ctitle").textContent === t)?.dataset.id, title)));
    return id;
  };

  await page.goto(`${base}/`);
  check("AC-105 / leads to /sessions/", new URL(page.url()).pathname === "/sessions/", page.url());
  await page.click("#tokanban");
  await page.waitForSelector("#board .col", { timeout: 5000 });
  check("AC-105 Kanban in the page's head loads /kanban/", new URL(page.url()).pathname === "/kanban/", page.url());
  const heads = await page.evaluate(() => [...document.querySelectorAll('.col:not(#sidecol) .colhead span:first-child')].map((h) => h.textContent));
  check("AC-105 five columns", heads.join() === "TODO,IN PROGRESS,WAIT FOR HUMAN,FAILED,DONE", heads.join());

  // the window: a title, Markdown, a file dropped in, in TODO
  await page.click("#newtask");
  await page.fill("#newtitle", "Fix the retry loop");
  await page.fill("#newtext", "**why**\n<img src=x onerror=\"window.ranit=1\">\n");
  const shot = path.join(work, "shot.png");
  fs.writeFileSync(shot, PNG);
  await page.setInputFiles("#newpick", shot);
  await within("AC-106 a file kept links at the cursor", () => page.waitForFunction(() => document.getElementById("newtext").value.includes("/api/task-files/"), null, { timeout: 5000 }));
  await page.click("#newpreview");
  await within("AC-106 Preview renders the Markdown", () => page.waitForSelector("#newshown strong", { timeout: 5000 }));
  check("AC-106 raw HTML stays off in Preview", await page.evaluate(() => !document.querySelector("#newshown img:not([data-raw])") && !window.ranit));
  await within("AC-109 a task's own picture shows, fetched with the sign-in", () => page.waitForFunction(() => document.querySelector("#newshown img")?.naturalWidth > 0, null, { timeout: 5000 }));
  await page.click("#newwrite");
  // a draft survives a reload
  await page.click("#newclose");
  await page.reload();
  await page.waitForSelector("#board .col");
  await page.click("#newtask");
  check("AC-106 a draft survives closing and a reload", await page.inputValue("#newtitle") === "Fix the retry loop", await page.inputValue("#newtitle"));
  check("AC-106 with its files", (await page.textContent("#newfiles")).includes("shot.png"));
  await page.fill("#newtitle", "");
  await page.click("#newcreate");
  check("AC-106 no title is refused by terminumd, said in place, the window stays", await until(async () => await page.textContent("#newerror") === "Title: 1–200 characters") && await page.isVisible("#newbox"));
  await page.fill("#newtitle", "Fix the retry loop");
  await page.click("#newcreate");
  await page.waitForSelector("#newbox", { state: "hidden", timeout: 5000 });
  await page.waitForFunction(() => document.querySelector('.col[data-status="TODO"] .card'), null, { timeout: 5000 });
  const first = await idOf("Fix the retry loop");
  check("AC-106 it stands in TODO with a T-number", /^T-\d+$/.test(first ?? "") && (await inColumn("TODO")).includes(first), first);
  await page.click("#newtask");
  check("AC-106 the draft went with Create", await page.inputValue("#newtitle") === "");
  await page.click("#newdiscard");

  // severity order, new on top of its own
  await make("second p2");
  await make("urgent", { severity: "P0" });
  const [second, urgent] = [await idOf("second p2"), await idOf("urgent")];
  // this test's own cards: an earlier check may have left tasks on the same board
  const ours = async () => (await inColumn("TODO")).filter((id) => [first, second, urgent].includes(id)).join();
  await until(async () => (await ours()).split(",").length === 3);
  check("AC-105 a column reads by severity, new on top of its own", await ours() === [urgent, second, first].join(), await ours());
  await page.dragAndDrop(card(first), card(second), { targetPosition: { x: 20, y: 3 } });
  check("AC-105 a drag orders within a severity", await until(async () => await ours() === [urgent, first, second].join()), await ours());

  // Move to…, the history
  await page.hover(card(second));
  await page.click(`${card(second)} .cmove`);
  await page.click('#moverows button:has-text("WAIT FOR HUMAN")');
  check("AC-107 Move to… moves it", await until(async () => (await inColumn("WAIT FOR HUMAN")).includes(second)));
  await page.dragAndDrop(card(urgent), '.col[data-status="DONE"]');
  check("AC-107 a drag across columns moves it", await until(async () => (await inColumn("DONE")).includes(urgent)));
  await page.click(card(second));
  await page.waitForSelector("#taskbox[open] .history");
  check("AC-105 a card's window has its own address", new URL(page.url()).pathname === `/kanban/${second}`, page.url());
  check("AC-107 its history says who moved it", /moved it from TODO to WAIT FOR HUMAN/.test(await page.textContent("#taskbody .history")));

  // a comment: CLOSE keeps it, nothing moves
  await page.fill(".newcomment textarea", "Use **v2**");
  await page.click('.newcomment button:has-text("CLOSE")');
  await page.waitForSelector("#taskbox", { state: "hidden", timeout: 5000 });
  await page.click(card(second));
  await within("AC-108 a comment shows as Markdown", () => page.waitForSelector("#taskbody .ev-comment .markdown strong", { timeout: 5000 }));
  check("AC-108 CLOSE moves nothing", (await inColumn("WAIT FOR HUMAN")).includes(second));
  // an edit of the text
  await page.click('.tasktext button:has-text("Edit")');
  await page.fill(".tasktext textarea", "typed and not saved");
  await page.selectOption("#taskbody .fieldgrid label:has-text(\"Severity\") select", "P3");
  await page.waitForTimeout(500);
  check("AC-107 a field changed keeps a text being typed", await page.inputValue(".tasktext textarea") === "typed and not saved");
  await page.fill(".tasktext textarea", "new text");
  await page.click('.tasktext button:has-text("Save")');
  check("AC-107 a text changed says so", await until(async () => /changed its text/.test(await page.textContent("#taskbody .history"))));
  // a comment deleted and a purge go through terminumd's write checks
  await page.click('.ev-comment button[aria-label="Delete"]');
  await page.click("#confirmok");
  check("AC-108 a comment is deleted", await until(() => page.evaluate(() => !document.querySelector("#taskbody .ev-comment"))));
  check("AC-108 and the history says who did it", /deleted a comment/.test(await page.textContent("#taskbody .history")));
  await page.click('.tasktext button:has-text("Purge text")');
  await page.click("#confirmok");
  check("AC-109 a text is purged", await until(async () => /purged its text/.test(await page.textContent("#taskbody .history"))));
  await page.click("#taskclose");
  await page.waitForFunction(() => location.pathname === "/kanban/");   // closing goes Back, which would cut the load below short

  // a deep link, files by header only
  await page.goto(`${base}/kanban/${first}`);
  await within("AC-105 /kanban/T-n opens its window", () => page.waitForSelector("#taskbox[open]", { timeout: 5000 }));
  check("AC-109 its picture is listed", (await page.textContent("#taskbody .filelist")).includes("shot.png"));
  const seen = await page.evaluate(async () => {
    const m = /\/api\/task-files\/([0-9a-f]{32})/.exec(document.querySelector("#taskbody").innerHTML);
    return m?.[1] ?? [...document.querySelectorAll("img[data-raw]")].map((i) => i.dataset.raw)[0]?.split("/").pop();
  });
  const bare = await fetch(`${base}/api/task-files/${seen}`).then((r) => r.status);
  check("AC-109 a file needs the header sign-in", bare === 401, String(bare));
  const secret = await page.evaluate(() => localStorage.getItem("tm.session"));
  const byAddress = await fetch(`${base}/api/task-files/${seen}?t=${secret}`).then((r) => r.status);
  check("AC-09 never an address", byAddress === 401, String(byAddress));
  await page.click("#taskbody .filelist button[aria-label^=\"Delete\"]");
  await page.click("#confirmok");
  check("AC-109 a file can be deleted for good", await until(async () => /deleted the file shot.png/.test(await page.textContent("#taskbody .history"))));
  check("AC-109 its bytes are gone", await page.evaluate(async (id) => (await fetch(`/api/task-files/${id}`)).status, seen) === 404);
  await page.click("#taskclose");

  // a session: one IN PROGRESS at a time, closed is said
  const pane = tmux("new-window", "-d", "-P", "-F", "#{pane_id}", "-n", "boardwork", "-c", work);
  let key = null;
  await until(async () => (key = await page.evaluate(async (p) => (await (await fetch("/api/tasks/meta")).json()).sessions.find((s) => s.key.endsWith(`:${p}`))?.key, pane)), 8000);
  check("AC-113 a session can be chosen", typeof key === "string", String(key));
  await page.reload();
  await page.waitForSelector("#board .col");
  const linked = await page.evaluate(async ({ a, b, k }) => {
    const p = (id, body) => fetch(`/api/tasks/${id}`, { method: "PATCH", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
    await p(a, { session: k });
    await p(b, { session: k });
    const one = await p(a, { status: "IN PROGRESS" });
    const two = await p(b, { status: "IN PROGRESS" });
    return { one: one.status, two: two.status, why: (await two.json()).error };
  }, { a: first, b: second, k: key });
  check("AC-113 a session holds one IN PROGRESS", linked.one === 200 && linked.two === 400 && /already works on/.test(linked.why), JSON.stringify(linked));
  await page.goto(`${base}/sessions/`);
  await page.waitForSelector(`#sessions .item[data-id="${key}"] .taskmark`, { timeout: 8000 });
  check("AC-113 its row shows its open tasks", (await page.textContent(`#sessions .item[data-id="${key}"]`)).includes(first));
  await page.click(`#sessions .item[data-id="${key}"]`, { button: "right" });
  await page.click('#rowmenu button[data-action="close"]');
  check("AC-113 closing it names them", (await page.textContent("#confirmtext")).includes(`${first}, ${second} are open in it.`), await page.textContent("#confirmtext"));
  await page.click('#confirmbox button[value="cancel"]');
  await page.click(`#sessions .item[data-id="${key}"]`, { button: "right" });
  await page.click('#rowmenu button[data-action="newtask"]');
  await page.waitForSelector("#newbox[open]", { timeout: 8000 });
  check("AC-106 New Task in a row's menu fills in its session", await page.inputValue("#newsession") === key, await page.inputValue("#newsession"));
  check("AC-106 and leaves the address clean", new URL(page.url()).search === "", page.url());
  await page.click("#newdiscard");
  await page.click(`.col[data-status="IN PROGRESS"] .card[data-id="${first}"] .ctitle`);
  await page.waitForSelector("#taskbox[open]");
  await page.click("#taskclose");
  tmux("kill-pane", "-t", pane);
  check("AC-113 a closed session is said on the card", await until(async () => /session closed/.test(await page.textContent(card(first))), 10000), await page.textContent(card(first)));
  check("AC-113 and its status stays", (await inColumn("IN PROGRESS")).includes(first));

  // a project made on the board, in the session list too; filters in the URL
  await page.click("#fproject summary");
  await page.click("#newproject");
  await page.fill("#namefield", "Billing");
  await page.press("#namefield", "Enter");
  await page.waitForSelector("#namebox", { state: "hidden", timeout: 5000 });
  await page.click("#fproject summary");
  check("AC-110 a project made on the board is offered", await until(() => page.isVisible('#fproject .projrow:has-text("Billing")')));
  check("AC-111 a picker floats on its own background, never its text over a column's", await page.evaluate(() => { const c = getComputedStyle(document.querySelector("#fproject > div")).backgroundColor; return c !== "rgba(0, 0, 0, 0)" && c !== "transparent"; }));
  check("AC-92 a project's color on the board is the session list's: its tokens are here too", await page.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue("--proj-l").trim() !== ""));
  await page.click("#newproject");
  await page.fill("#namefield", "Billing");
  await page.press("#namefield", "Enter");
  check("AC-110 a taken name is said, and stays open", await until(async () => /already named Billing/.test(await page.textContent("#nameerror"))) && await page.isVisible("#namebox"));
  await page.click('#namebox button[value="cancel"]');
  await page.click("#fproject summary");
  await page.click('#fproject .projrow button:has-text("Default")');
  await page.click("#fseverity summary");
  await page.click('#fseverity label:has-text("P0")');
  check("AC-111 a filter is kept in the URL", await until(() => /severity=P0/.test(page.url()) && /project=default/.test(page.url())), page.url());
  check("AC-111 and holds on the columns", await until(async () => (await inColumn("DONE")).join() === urgent && (await inColumn("IN PROGRESS")).length === 0));
  await page.reload();
  await page.waitForSelector("#board .col");
  check("AC-111 a reload keeps it", await until(async () => (await inColumn("DONE")).join() === urgent));
  await page.click("#clearfilters");
  check("AC-111 Clear filters takes off severity, the project stays", await until(() => !/severity/.test(page.url()) && /project=default/.test(page.url())), page.url());
  check("AC-111 and shows the project's", await until(async () => (await inColumn("IN PROGRESS")).length === 1));
  await page.click("#fproject summary");
  await page.click('#fproject button:has-text("All projects")');
  await page.goto(`${base}/kanban/?q=${encodeURIComponent("retry")}`);
  check("AC-111 Find reads the URL", await until(async () => (await inColumn("IN PROGRESS")).join() === first && (await inColumn("DONE")).length === 0));

  // the session list shows the empty project
  await page.click("#tosessions");
  await page.waitForFunction(() => document.querySelectorAll("#sessions .item").length > 0, null, { timeout: 10000 });
  check("AC-110 and the session list shows it, with no session", await until(() => page.isVisible('#sessions section.project .label:has-text("Billing")')));
  await page.click('#sessions section.project:has(.label:has-text("Billing")) > h2', { button: "right" });
  await page.click('#rowmenu button[data-action="deleteproject"]');
  check("AC-110 Delete Project says what moves", /Its 0 sessions and 0 tasks move to Default; nothing is closed\./.test(await page.textContent("#confirmtext")), await page.textContent("#confirmtext"));
  await page.click("#confirmok");
  check("AC-110 and it goes", await until(() => page.evaluate(() => ![...document.querySelectorAll("#sessions section.project .label")].some((l) => l.textContent === "Billing"))));

  // what an agent or a page wrote stays text
  await page.goto(`${base}/kanban/`);
  await page.waitForSelector("#board .col");
  await make('<img src=x onerror="window.ranit=1">');
  await until(async () => (await inColumn("TODO")).length >= 1);
  check("AC-105 a title is text", await page.evaluate(() => !window.ranit && !document.querySelector(".ctitle img")));
  if (shots) {
    await page.screenshot({ path: path.join(shots, "board-wide.png") });
    await page.click(card(first));
    await page.waitForSelector("#taskbox[open]");
    await page.screenshot({ path: path.join(shots, "task-wide.png") });
    await page.click("#taskclose");
    await page.click("#newtask");
    await page.screenshot({ path: path.join(shots, "new-wide.png") });
    await page.click("#newdiscard");
  }
  // the checks after this one go on where they expect: a session shown on the session page
  await page.goto(`${base}/sessions/`);
  await page.waitForFunction(() => document.querySelectorAll("#sessions .item").length > 0, null, { timeout: 10000 });
  await page.click('#sessions .item');
  await page.waitForSelector("#term", { state: "visible", timeout: 5000 });
}

/** The board's controls are the session page's kinds: measured, not read from a sheet (docs/UI-GUIDELINES.md §6). */
async function sameKinds(page, base, phone) {
  const look = (sel) => page.evaluate((s) => {
    const e = [...document.querySelectorAll(s)].find((x) => x.getClientRects().length);
    if (!e) return null;
    const c = getComputedStyle(e);
    return { fill: c.backgroundColor, image: c.backgroundImage === "none" ? "" : "matte", edge: c.boxShadow === "none" ? "" : "edge", radius: c.borderRadius, h: Math.round(e.getBoundingClientRect().height), font: c.fontFamily.split(",")[0] };
  }, sel);
  await page.goto(`${base}/sessions/`);
  await page.waitForSelector("#filter", { state: "attached" });
  await page.waitForTimeout(500);   // colors settle (motion.css), the theme is in
  const theirs = { search: await look(".search"), key: await look(".icon"), track: await look(".seg") };
  await page.goto(`${base}/kanban/`);
  await page.waitForSelector(".card");
  await page.waitForTimeout(500);
  const ours = { search: await look("#filters .search"), key: await look("#boardhead .pick summary, #filtersbtn"), track: await look("#boardhead .seg") };
  check(`look: Find is the session list's field (${phone ? "phone" : "wide"})`, ["fill", "edge", "radius"].every((k) => ours.search?.[k] === theirs.search?.[k]), JSON.stringify([ours.search, theirs.search]));
  check("look: a board key is the page's key", ["image", "edge", "radius"].every((k) => ours.key?.[k] === theirs.key?.[k]), JSON.stringify([ours.key, theirs.key]));
  check("look: its pages' switch is the page's segmented control", theirs.track === null || ["edge", "radius"].every((k) => ours.track?.[k] === theirs.track?.[k]), JSON.stringify([ours.track, theirs.track]));
  const rules = await page.evaluate(() => ({
    grain: [...document.styleSheets].some((s) => (s.href ?? "").endsWith("grain.css")), sheets: [...document.styleSheets].some((s) => (s.href ?? "").endsWith("sheets.css")), motion: [...document.styleSheets].some((s) => (s.href ?? "").endsWith("motion.css")),
    words: [...document.querySelectorAll("#board button, #boardhead button")].filter((b) => /null|undefined/.test(b.textContent)).length,
  }));
  check("look: the board has the page's material, sheets and motion", rules.grain && rules.sheets && rules.motion);
  check("look: no key says null", rules.words === 0, String(rules.words));
  await page.click("#newtask");
  await page.waitForSelector("#newbox[open]");
  const fields = await page.evaluate(() => [...document.querySelectorAll("#newbox select.field, #newbox input.field")].map((e) => e.offsetHeight));
  check("look: the window's fields stand as tall on every engine", Math.max(...fields) - Math.min(...fields) <= 2 && Math.min(...fields) >= 30, fields.join());
  await page.click("#newclose");
}

/** A phone: one column at a time, Move to… and no sideways scroll (AC-105). */
export async function liveBoardPhone(page, { base, shots }) {
  await sameKinds(page, base, true);
  await page.goto(`${base}/kanban/`);
  await page.waitForSelector("#colswitch button");
  const shown = () => page.evaluate(() => [...document.querySelectorAll(".col[data-status]")].filter((c) => { const r = c.getBoundingClientRect(); return r.left > -5 && r.right < innerWidth + 5; }).map((c) => c.dataset.status));
  check("AC-105 a phone shows one column", (await shown()).join() === "TODO", (await shown()).join());
  await page.click('#colswitch button:has-text("DONE")');
  check("AC-105 its switch names them and shows another", await until(async () => (await shown()).join() === "DONE"), (await shown()).join());
  const wide = await page.evaluate(() => document.documentElement.scrollWidth - innerWidth);
  check("AC-105 no sideways scroll on a phone", wide <= 0, String(wide));
  const tap = await page.evaluate(() => Math.round(document.querySelector("#newtask").getBoundingClientRect().height));
  check("AC-105 touch targets", tap >= 40, String(tap));
  const small = await page.evaluate(() => [...document.querySelectorAll("#boardhead button, #boardhead summary, #boardhead a, .card button")].filter((e) => e.getClientRects().length && !e.closest("details:not([open]) > .box") && (e.getBoundingClientRect().height < 34 || e.getBoundingClientRect().width < 34)).map((e) => e.textContent.trim() || e.getAttribute("aria-label")));
  check("AC-105 every key of the head and a card takes a finger", !small.length, small.join());
  const head = await page.evaluate(() => Math.round(document.querySelector("#boardhead").getBoundingClientRect().height));
  check("AC-105 the head leaves the cards the room", head <= 112, String(head));
  check("AC-105 many cards never squeeze the head over the switch", await page.evaluate(() => document.querySelector("#colswitch").getBoundingClientRect().top >= document.querySelector("#boardhead").getBoundingClientRect().bottom));
  await page.evaluate(() => { const b = document.querySelector("#board"); b.scrollLeft = b.clientWidth * 2; });
  check("AC-105 a swipe shows the next column and its tab follows", await until(() => page.evaluate(() => document.querySelector('#colswitch [aria-selected="true"]').dataset.status === "WAIT FOR HUMAN")));
  if (shots) await page.screenshot({ path: path.join(shots, "board-phone.png") });
}
