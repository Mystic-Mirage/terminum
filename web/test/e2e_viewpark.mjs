// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// AC-70 checks for e2e_app.mjs: Views waiting for their sessions are paused (fb-park), each kept
// as it was, and told to resume when shown again.
import { check } from "./check.mjs";

/** page: e2e_app's page (its View frames load /blank.html); tell(m) as the bridge; win: a layout window. */
export async function webViewPark({ page, tell, win }) {
  const $eval = (fn, arg) => page.evaluate(fn, arg);
  const ids = Array.from({ length: 12 }, (_, i) => `P${i + 1}`);
  tell({ t: "layout", groups: [{ ...win, items: [...win.items, ...ids.map((id) => ({ ...win.items[0], id, title: id }))] }] });
  await page.waitForFunction(() => document.querySelector("#sessions .item[data-id='P12']"));
  const srcs = {};
  for (const id of ids) {
    await page.click(`#sessions .item[data-id='${id}']`);
    srcs[id] = `/blank.html?park-${id}`;
    await $eval(([src]) => {
      const b = document.querySelector('button[data-view="file"]');
      b.disabled = false; b.click();
      document.getElementById("file").src = src;
    }, [srcs[id]]);
    await page.waitForFunction((src) => document.getElementById("file").contentWindow?.location.href.endsWith(src)
      && document.getElementById("file").contentDocument?.readyState === "complete", srcs[id]);
    // as the Panel would: P3 holds unsaved edits; every frame logs what the page tells it
    await $eval((dirty) => {
      const w = document.getElementById("file").contentWindow;
      w.fbBusy = () => dirty;
      w.got = [];
      w.addEventListener("message", (e) => { if (e.data?.type === "fb-park") w.got.push(e.data.park); });
    }, id === "P3");
  }
  await page.click("#sessions .item[data-id='S2']");
  await page.waitForTimeout(150);
  const waiting = await $eval(() => [...document.querySelectorAll("#filewin iframe[data-sid]")].map((f) => ({ sid: f.dataset.sid, got: f.contentWindow?.got ?? null })));
  const sids = waiting.map((w) => w.sid).sort();
  check("AC-70 web app: every waiting View stays loaded, as it was", sids.length === 12 && sids.includes("P1"), JSON.stringify(sids));
  check("AC-70 web app: a waiting View is told to pause its event stream", waiting.every((w) => w.got?.at(-1) === true), JSON.stringify(waiting));
  await page.click("#sessions .item[data-id='P6']");
  // postMessage arrives on a later task
  await page.waitForFunction(() => document.getElementById("file").contentWindow?.got?.at(-1) === false, null, { timeout: 2000 }).catch(() => {});
  const back = await $eval(() => ({ got: document.getElementById("file").contentWindow?.got, src: document.getElementById("file").getAttribute("src") }));
  check("AC-70 web app: a waiting View shown again is told to resume", back.got?.at(-1) === false && back.src === "/blank.html?park-P6", JSON.stringify(back));
}
