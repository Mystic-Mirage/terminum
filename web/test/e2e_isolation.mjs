// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// AC-81, AC-132 against a fake terminumd (fake.mjs): a host that drops keeps its rows and the shown
// pane; what is said of a stalling server stays on its own panes and resizes nothing.

import { check, runningIn } from "./check.mjs";
import { drawn, group, item, openPage, pause, SIZES } from "./fake.mjs";

const L1 = "tmux:local:terminum:%1", L2 = "tmux:local:terminum:%2", H1 = "tmux:devbox:default:%5", H2 = "tmux:devbox:default:%6";
const local = () => group("main", [item(L1, "local-one"), item(L2, "local-two")], { server: "local:terminum" });
const host = (state, items, why = "") => group("devbox", items, { host: "devbox", session: "terminum", server: "devbox:default", state, why });
const rows = (extra = {}) => [item(H1, "host-one", { host: "devbox", ...extra }), item(H2, "host-two", { host: "devbox", ...extra })];
const STALL = { t: "stall", server: "devbox", key: "devbox:default", what: "tmux on devbox", ms: 3100 };

export async function webIsolation(browser, name) {
  for (const [size, device] of SIZES) {
    runningIn(`${name} ${size}`);
    const f = await openPage(browser, { device, layout: [host("attached", rows()), local()] });
    const { page, sent, say } = f;
    await drawn(page);
    const shown = () => sent.filter((m) => m.t === "sub").pop()?.id;
    // a click on a row, whatever covers it on a phone: what is checked is what follows
    const pick = async (id) => { await page.evaluate((i) => document.querySelector(`#sessions .item[data-id="${i}"]`)?.click(), id); await pause(page, 100); };
    // the host's group above the local one: its rows going would move the local rows
    const listed = async (h) => { say({ t: "layout", groups: [h, local()] }); await pause(page, 120); };

    // the link drops: the rows stay, dimmed, and the shown pane with them
    await pick(H1);
    const before = await page.evaluate((id) => document.querySelector(`#sessions .item[data-id="${id}"]`)?.getBoundingClientRect().top, L2);
    await listed(host("reconnecting", rows({ offline: true, state: null }), "reconnecting (1 s)"));
    const down = await page.evaluate(([id, other]) => {
      const row = document.querySelector(`#sessions .item[data-id="${id}"]`);
      return { dim: row?.classList.contains("offline"), opacity: row && getComputedStyle(row.querySelector(".txt")).opacity,
        top: document.querySelector(`#sessions .item[data-id="${other}"]`)?.getBoundingClientRect().top };
    }, [H1, L2]);
    check("AC-81 a dropped link: the shown pane stays", shown() === H1, shown());
    check("AC-81 a dropped link: its rows stay, dimmed", down.dim === true && Number(down.opacity) < 1, JSON.stringify(down));
    check("AC-81 a dropped link moves other rows by its state's one line, no more", Math.abs(down.top - before) < 30, JSON.stringify({ before, top: down.top }));
    await listed(host("attached", rows()));
    check("AC-81 back: the same pane, its rows as before", shown() === H1 && !await page.locator(`#sessions .item.offline`).count(), shown());

    // no rows at all (terminumd restarted, the host not read yet): the page waits for its list
    await listed(host("reconnecting", [], "reading its list"));
    const header = await page.textContent("#current .t");
    check("AC-132 a host not read yet: the page stays on its pane, its header too", shown() === H1 && header !== "Choose a session", JSON.stringify({ shown: shown(), header }));
    await listed(host("failed", [], "ssh: connect timed out"));
    check("AC-132 a host failing: the page stays on its pane", shown() === H1, shown());
    await listed(host("attached", rows().slice(1)));
    check("AC-92 a host's list, read, without the pane: it closed, the page goes on", shown() === H2, shown());
    await listed(host("attached", rows()));

    // no more tries by itself: the button says so, and asks for one
    const silent = "tmux on devbox does not answer: its client is 3.8, its running server 3.4. Restart its server, or bring that version back.";
    await listed(host("failed", rows({ offline: true, state: null }), silent));
    check("AC-132 the stopped host's pane stays", shown() === H2, shown());
    const said = await page.evaluate(() => { const g = document.querySelector("#sessions .hostgrp"); return { why: g.querySelector(".gstate")?.textContent, button: !!g.querySelector("button.reconnect") }; });
    check("AC-81 a host with no answer from tmux says why, with no button until it stops trying", said.why === silent && !said.button, JSON.stringify(said));
    await listed({ ...host("failed", rows({ offline: true, state: null }), silent), reconnect: true });
    await page.evaluate(() => document.querySelector("#sessions .hostgrp button.reconnect").click());   // a phone's list is closed
    await pause(page, 100);
    const asked = sent.filter((m) => m.t === "hostaction").pop();
    check("AC-81 Reconnect asks one try of that host", asked?.name === "devbox" && asked?.action === "retry", JSON.stringify(asked));
    await listed(host("attached", rows()));
    check("AC-81 attached again: no button", !await page.locator("#sessions button.reconnect").count());

    // a stall of the host, on a local pane: nothing said, nothing resized; its group says it
    await pick(L1);
    await pause(page, 200);
    const term = () => page.evaluate(() => { const r = document.getElementById("term").getBoundingClientRect(); return `${Math.round(r.top)} ${Math.round(r.height)}`; });
    const banner = () => page.evaluate(() => { const e = document.getElementById("stall"); return e.hidden ? "" : e.textContent; });
    const sizes = () => sent.filter((m) => m.t === "size").length;
    const was = await term();
    say({ ...STALL, ok: false });
    await pause(page, 200);
    check("AC-132 a host's stall says nothing over a local pane", await banner() === "", await banner());
    const state = await page.evaluate(() => [...document.querySelectorAll("#sessions .hostgrp")].map((g) => g.textContent).join());
    check("AC-132 its group in the list says it", state.includes("not responding"), state);

    // on its own pane: said, over the terminal, taking no room
    await pick(H1);
    await pause(page, 200);
    const onHost = await term(), m = sizes();
    check("AC-132 on its own pane the stall is said", /tmux on devbox is not responding/.test(await banner()), await banner());
    const covered = await page.evaluate(() => {
      const b = document.getElementById("stall").getBoundingClientRect(), t = document.getElementById("term").getBoundingClientRect();
      return Math.round(b.top) === Math.round(t.top) && b.height > 0;
    });
    check("AC-132 the line stands over the terminal's top", covered);
    say({ ...STALL, ok: true });
    await pause(page, 300);
    check("AC-132 the line going moves nothing, asks no new size", await term() === onHost && sizes() === m, JSON.stringify({ onHost, now: await term(), sizes: sizes() - m }));
    say({ ...STALL, ok: false });
    await pause(page, 300);
    check("AC-132 the line coming moves nothing, asks no new size", await term() === onHost && sizes() === m, JSON.stringify({ onHost, now: await term(), sizes: sizes() - m }));
    await pick(L1);
    await pause(page, 150);
    check("AC-132 leaving its pane takes the line off at once", await banner() === "", await banner());
    check("AC-132 local pane back as it was", await term() === was, JSON.stringify({ was, now: await term() }));
    say({ ...STALL, ok: true });
    await pause(page, 100);

    // a new connection: what it knew of stalls is over (terminumd may have restarted)
    say({ ...STALL, ok: false });
    await pause(page, 100);
    say({ t: "stall", clear: true });
    await pause(page, 100);
    const after = await page.evaluate(() => [...document.querySelectorAll("#sessions .hostgrp")].map((g) => g.textContent).join());
    check("AC-132 a new connection forgets the stalls it knew", !after.includes("not responding"), after);

    // a window that comes late takes nobody away
    await pick(L2);
    say({ t: "created", id: H2, stay: true });
    await pause(page, 150);
    check("AC-132 a window told to stay leaves the user where they went", shown() === L2, shown());
    say({ t: "created", id: H2 });
    await pause(page, 150);
    check("AC-07 a window asked from here is shown", shown() === H2, shown());
    check(`no page errors (isolation) ${size}`, !f.errors.length, f.errors.join());
    await page.context().close();
  }
}
