// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The Plan pane's UI checklist (docs/UI-GUIDELINES.md §6, AC-97, AC-98) at one size: a light theme's
// contrast (the percent, the track, the header's progress, the pane's text), --size 10 and 20, the
// keyboard (Tab, a focus ring, Enter, Esc gives focus back), a long title, doing and session name at
// the 300 px sidebar and at 390 px, the empty state, `hooks: false` said, and the pane with a banner
// and a toast. Called by e2e_plans.mjs.
import { check } from "./check.mjs";
import { drawn, group, item, openPage, pause, THEME } from "./fake.mjs";

const A = "tmux:local:terminum:%1", B = "tmux:local:terminum:%2";
const LONG = "a-very-long-session-name-that-goes-on-and-on-and-on-and-on";
const TITLE = "T".repeat(40) + " " + "t".repeat(39), DOING = "d".repeat(60) + " " + "D".repeat(59);
const HOOKS = "terminum's hooks are not running in this session (Codex: trust them in /hooks).";
const LIGHT = { ...THEME, bg: "#ffffff", fg: "#1f2328", dark: false };

/** WCAG's ratio of a node's text (or a given color) against the first opaque background under it. */
export const MEASURE = () => {
  const cv = document.createElement("canvas").getContext("2d", { willReadFrequently: true });
  const rgb = (s) => { cv.clearRect(0, 0, 1, 1); cv.fillStyle = "#000"; cv.fillStyle = s; cv.fillRect(0, 0, 1, 1); const d = cv.getImageData(0, 0, 1, 1).data; return [d[0], d[1], d[2], d[3] / 255]; };   // any CSS color (color-mix's color(srgb …) too)
  const lum = ([r, g, b]) => [r, g, b].map((v) => { v /= 255; return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4; })
    .reduce((a, v, i) => a + v * [0.2126, 0.7152, 0.0722][i], 0);
  const under = (e) => { for (let n = e; n; n = n.parentElement) { const c = rgb(getComputedStyle(n).backgroundColor); if (c[3] > 0.9) return c; }
    return rgb(getComputedStyle(document.body).backgroundColor); };
  const blend = (c, bg, a) => c.slice(0, 3).map((v, i) => v * a + bg[i] * (1 - a));
  window.__ratio = (e, color = getComputedStyle(e).color) => {
    const bg = under(e.parentElement ?? e), c = rgb(color), a = (c[3] ?? 1) * Number(getComputedStyle(e).opacity);
    const [x, y] = [lum(blend(c, bg, a)), lum(bg)].sort((p, q) => q - p);
    return Math.round((x + 0.05) / (y + 0.05) * 100) / 100;
  };
};

const overlaps = (a, b) => a.left < b.right - 1 && b.left < a.right - 1 && a.top < b.bottom - 1 && b.top < a.bottom - 1;

export async function planLook(browser, name, size, device) {
  const at = `${name} ${size}`, phone = size.startsWith("390");
  const now = Math.floor(Date.now() / 1000);
  const plan = { done: 3, total: 7, pct: 43, eta: now + 3600, updated: now - 300, stale: false, partial: false };
  let reply = { plan: { title: TITLE, doing: DOING, items: [{ text: "one", status: "completed" }, { text: "two", status: "in_progress" }],
    done: 1, total: 2, pct: 50, eta: null, eta_given: null, updated: now - 60, source: "hooks", partial: false, agent: "codex" }, hooks: false };
  const f = await openPage(browser, { device, layout: [group("main", [item(A, LONG, { plan }), item(B, LONG + "-2", { index: "2", plan })])],
    hook: (m, say) => {
      if (m.t === "sub") say({ t: "theme", sid: m.id, theme: LIGHT });
      if (m.t === "plan") say({ t: "plan", key: m.key, ...reply });
    } });
  const { page } = f;
  await drawn(page);
  await pause(page, 250);
  await page.evaluate(MEASURE);

  // a light theme: the row's percent and track, the column's progress circle
  const row = await page.evaluate((a) => {
    const r = document.querySelector(`.item[data-id="${a}"]`), p = r.querySelector(".plpct"), t = r.querySelector(".pltrack .plfill");
    const ct = document.getElementById("planfabt"), arc = document.querySelector("#planfab .arc");
    return { light: document.documentElement.classList.contains("light"), pct: __ratio(p), fill: __ratio(t, getComputedStyle(t).backgroundColor),
      head: __ratio(ct), arc: __ratio(ct, getComputedStyle(arc).stroke) };
  }, A);
  check(`${at} light theme: a row's percent ≥ 4.5:1, its track's fill ≥ 3:1, the progress circle's percent ≥ 4.5:1 and its ring ≥ 3:1`,
    row.light && row.pct >= 4.5 && row.fill >= 3 && row.head >= 4.5 && row.arc >= 3, JSON.stringify(row));

  // long names: ellipsis, the percent in the row, no overlap, no sideways scroll
  // a phone: back to the list by ☰
  if (phone) { await page.click("#navbtn"); await pause(page, 300); }
  const longRow = await page.evaluate((a) => {
    const r = document.querySelector(`.item[data-id="${a}"]`), p = r.querySelector(".plpct").getBoundingClientRect(), rr = r.getBoundingClientRect();
    const t = r.querySelector(".t") ?? r.querySelector(".title"), tr = t?.getBoundingClientRect();
    return { pct: [Math.round(p.left), Math.round(p.right)], row: [Math.round(rr.left), Math.round(rr.right)], cut: t ? t.scrollWidth > t.clientWidth : null,
      ell: t ? getComputedStyle(t).textOverflow : null, apart: tr ? tr.right <= p.left + 1 : null, scroll: document.documentElement.scrollWidth - innerWidth };
  }, A);
  check(`${at} a long session name: ends in …, its percent stays inside the row and clear of it, no sideways scroll`,
    longRow.cut && longRow.ell === "ellipsis" && longRow.apart && longRow.pct[1] <= longRow.row[1] && longRow.scroll <= 0, JSON.stringify(longRow));
  if (phone) { await page.click(`.item[data-id="${A}"]`); await pause(page, 300); }

  // the keyboard: Tab reaches the Plan button with a ring, Enter opens, Tab reaches the pane's buttons, Esc closes and gives focus back
  await page.evaluate(() => document.activeElement?.blur());
  const TAB = name === "webkit" ? "Alt+Tab" : "Tab";   // Safari's default: Tab between fields only, ⌥Tab every control
  let reached = false;
  for (let i = 0; i < 40 && !reached; i++) { await page.keyboard.press(TAB); reached = await page.evaluate(() => document.activeElement?.id === "curplan"); }
  const ring = await page.evaluate(() => { const s = getComputedStyle(document.getElementById("curplan")); return { style: s.outlineStyle, w: parseFloat(s.outlineWidth) }; });
  check(`${at} Tab reaches the header's Plan button, with a visible focus ring`, reached && ring.style !== "none" && ring.w >= 1, JSON.stringify({ reached, ring }));
  await page.keyboard.press("Enter");
  await page.waitForSelector("#planbody .plitems", { timeout: 3000 }).catch(() => {});
  let btns = [];
  for (let i = 0; i < 8; i++) { await page.keyboard.press(TAB);
    btns.push(await page.evaluate(() => { const e = document.activeElement, s = getComputedStyle(e);
      return { in: !!e.closest("#planpane"), tag: e.tagName, ring: s.outlineStyle !== "none" && parseFloat(s.outlineWidth) >= 1 }; })); }
  const pressed = btns.filter((b) => b.tag === "BUTTON");
  check(`${at} Enter opens the pane; Tab walks its buttons, each with a focus ring, and stays in the pane`,
    !await page.evaluate(() => document.getElementById("planpane").hidden) && pressed.length >= 3 && pressed.every((b) => b.ring) && btns.every((b) => b.in), JSON.stringify(btns));

  // the pane's text on the light theme, the long title and doing wrap inside it, hooks: false said
  const pane = await page.evaluate((hooks) => {
    const p = document.getElementById("planpane"), r = p.getBoundingClientRect(), b = document.getElementById("planbody");
    const inside = [...b.querySelectorAll(".pltitle, .pldoing, .plmeta, .pltext")].every((e) => { const q = e.getBoundingClientRect(); return q.left >= r.left - 1 && q.right <= r.right + 1; });
    return { title: __ratio(b.querySelector(".pltitle")), doing: __ratio(b.querySelector(".pldoing")), meta: Math.min(...[...b.querySelectorAll(".plmeta")].map((e) => __ratio(e))),
      head: __ratio(p.querySelector("h2")), inside, sideways: p.scrollWidth > p.clientWidth + 1, hooks: [...b.querySelectorAll(".plhooks")].map((e) => e.textContent).join() === hooks,
      inView: r.left >= -1 && r.right <= innerWidth + 1 };
  }, HOOKS);
  check(`${at} light theme: the pane's title, doing, meta and heading ≥ 4.5:1`, Math.min(pane.title, pane.doing, pane.meta, pane.head) >= 4.5, JSON.stringify(pane));
  check(`${at} an 80-character title and a 120-character doing wrap inside the pane, no sideways scroll`, pane.inside && !pane.sideways && pane.inView, JSON.stringify(pane));
  check(`${at} the plan's hooks: false is said in one line`, pane.hooks, JSON.stringify(pane));

  // with a banner and a toast: they do not cover each other
  await page.evaluate(() => import("/banners.js").then((b) => b.banner("status", "tmux is gone: a long banner to check the pane does not cover it")));
  await pause(page, 150);
  const stack = await page.evaluate(() => {
    const box = (s) => { const e = document.querySelector(s); return e && e.getClientRects().length ? e.getBoundingClientRect().toJSON() : null; };   // a phone's sheet is fixed: no offsetParent
    return { pane: box("#planpane"), banner: box("#status"), bar: box("#bar") };
  });
  check(`${at} the Plan pane and a banner: the pane does not cover the banner`, !!stack.banner && !!stack.pane && !overlaps(stack.pane, stack.banner), JSON.stringify(stack));

  // --size 10 and 20: the pane, the header's Plan button and the progress circle fit and do not overlap
  for (const px of [10, 20]) {
    await page.evaluate((px) => document.documentElement.style.setProperty("--size", `${px}px`), px);
    await pause(page, 150);
    const s = await page.evaluate(() => {
      const r = (q) => document.querySelector(q)?.getBoundingClientRect(), p = r("#planpane"), c = r("#curplan"), t = r("#current .t"), o = r("#planfab");
      const ct = document.getElementById("planfabt");
      return { pane: p && p.left >= -1 && p.right <= innerWidth + 1, head: !!c && c.width >= 30 && (!t || c.left >= t.right - 1) && c.right <= innerWidth, w: c && Math.round(c.width), c: c && [Math.round(c.left), Math.round(c.right)], t: t && [Math.round(t.left), Math.round(t.right)], iw: innerWidth,
        tw: Math.round(ct.getBoundingClientRect().width * 10) / 10, sw: Math.round(document.querySelector("#planfab svg").getBoundingClientRect().width),
        ring: o.right <= innerWidth && (() => { const s = document.querySelector("#planfab svg").getBoundingClientRect(), x = ct.getBoundingClientRect(); return x.width <= s.width * 30.8 / 36; })(), sideways: document.documentElement.scrollWidth - innerWidth };
    });
    check(`${at} --size ${px}: the pane in view, the Plan button shown (≥ 30px) after the title, the circle's percent whole, no sideways scroll`, s.pane && s.head && s.ring && s.sideways <= 0, JSON.stringify(s));
  }

  // Esc closes the pane and gives the focus back to the Plan button
  await page.focus("#planpane");
  await page.keyboard.press("Escape");
  await pause(page, 150);
  const back = await page.evaluate(() => ({ open: !document.getElementById("planpane").hidden, focus: document.activeElement?.id }));
  check(`${at} Esc closes the pane and gives the focus back to the header's Plan button`, !back.open && back.focus === "curplan", JSON.stringify(back));

  // the empty state: no plan, no task
  reply = { plan: null };
  await page.click("#curplan", { timeout: 5000 });
  await pause(page, 250);
  const empty = await page.evaluate(() => [...document.querySelectorAll("#planbody p, #planbody h3")].map((e) => e.textContent));
  check(`${at} the empty state: "No plan: …" and no task (terminum-kanban.md AC-113), no hooks line`, empty.some((t) => t.startsWith("No plan: ")) && empty.includes("No task in this session.") && empty.length === 2, JSON.stringify(empty));
  check(`no page errors (plans look) ${size}`, !f.errors.length, f.errors.join(" | "));
  await f.close();
}
