// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The page's notices and keys against a fake terminumd (fake.mjs), at both sizes: an error banner
// and a stall banner close with their × until they say something new (AC-14, AC-81); a paste with
// no text sends nothing and says so, and the Paste field (no clipboard read) sends with ⌘Enter or
// Ctrl+Enter (AC-03); View's Terminal key is a key as the others (AC-70);
// on a phone ⟨…⟩ puts the on-screen keyboard and the hot keys away first (AC-18).
import { check, runningIn } from "./check.mjs";
import { drawn, openPage, pause, SIZES } from "./fake.mjs";

const shown = (page, id) => page.evaluate((id) => !document.getElementById(id).hidden, id);

export async function webNotes(browser, name) {
  for (const [size, device] of SIZES) {
    runningIn(`${name} ${size}`);
    const phone = size.startsWith("390");
    const f = await openPage(browser, { device });
    const { page } = f;
    await drawn(page);
    await pause(page, 200);

    f.say({ t: "error", msg: "tmux did not take the paste" });
    await pause(page, 100);
    const text = await page.evaluate(() => document.getElementById("status").textContent);
    await page.click("#status .x");
    const closed = !await shown(page, "status");
    f.say({ t: "error", msg: "tmux did not take the paste" });
    await pause(page, 100);
    const same = await shown(page, "status");
    f.say({ t: "error", msg: "Connection to devbox lost" });
    await pause(page, 100);
    const other = await shown(page, "status");
    check("AC-14 an error banner has an ×: closed it stays away until another error comes", text === "tmux did not take the paste" && closed && !same && other,
      JSON.stringify({ text, closed, same, other }));
    f.say({ t: "error", msg: "" });
    f.say({ t: "stall", server: "local", ms: 5000 });
    await pause(page, 1300);                         // its seconds count on
    await page.click("#stall .x");
    await pause(page, 1200);
    check("AC-14 a stall banner closed with its × stays away while its seconds count on", !await shown(page, "stall"));
    f.say({ t: "stall", server: "local", ok: true });

    // a paste with no text (a phone's clipboard that holds none): nothing sent, a toast says so
    const from = f.sent.length;
    await page.evaluate(() => {
      const dt = new DataTransfer();
      dt.setData("text/plain", "");
      document.getElementById("kbd").dispatchEvent(new ClipboardEvent("paste", { clipboardData: dt, bubbles: true, cancelable: true }));
    });
    await pause(page, 150);
    const toast = await page.evaluate(() => document.getElementById("toast").textContent);
    check("AC-03 a paste with no text sends nothing and says so", !f.sent.slice(from).some((m) => m.t === "in") && /no text/.test(toast), JSON.stringify({ toast, sent: f.sent.slice(from) }));

    // no clipboard read (denied, or not HTTPS): the Paste field; ⌘Enter (Ctrl+Enter on the phone size) sends it
    await page.evaluate(() => {
      for (const k of ["read", "readText"]) Object.defineProperty(navigator.clipboard, k, { value: () => Promise.reject(new Error("denied")), configurable: true });
      document.getElementById("hotkeys").hidden = false;
      document.querySelector('#hotkeys button.key[data-action="paste"]').click();
    });
    await page.waitForSelector("#pastebox[open]", { timeout: 3000 });
    await page.fill("#pastetext", "echo pasted");
    const before = f.sent.length;
    await page.press("#pastetext", phone ? "Control+Enter" : "Meta+Enter");
    await pause(page, 200);
    const pasted = { open: await page.evaluate(() => document.getElementById("pastebox").open), sent: f.sent.slice(before).filter((m) => JSON.stringify(m).includes("echo pasted")).map((m) => m.t) };
    check(`AC-03 the Paste field sends with ${phone ? "Ctrl" : "⌘"}+Enter and closes`, !pasted.open && pasted.sent.length === 1, JSON.stringify(pasted));
    await page.evaluate(() => { document.getElementById("hotkeys").hidden = true; });

    if (phone) {
      // ⟨…⟩ puts the keyboard and the hot keys away before the column opens
      await page.evaluate(() => { document.getElementById("hotkeys").hidden = false; document.getElementById("kbd").focus(); });   // both up
      await page.tap("#foldfab");
      await pause(page, 200);
      const after = await page.evaluate(() => ({ kbd: document.activeElement === document.getElementById("kbd"), keys: !document.getElementById("hotkeys").hidden,
        open: document.body.classList.contains("unfolded") }));
      check("AC-18 a phone: a tap on ⟨…⟩ puts the keyboard and the hot keys away, then unfolds", !after.kbd && !after.keys && after.open, JSON.stringify(after));
      await page.tap("#foldfab");

      // View's Terminal key: a key as the header's ⋯
      await page.evaluate(() => { const v = document.querySelector('button[data-view="file"]'); v.disabled = false; v.click(); });
      await pause(page, 300);
      const look = await page.evaluate(() => { const a = getComputedStyle(document.getElementById("backterm")), m = getComputedStyle(document.getElementById("menubtn"));
        return { fill: a.backgroundColor === m.backgroundColor, radius: a.borderRadius === m.borderRadius, image: a.backgroundImage === m.backgroundImage, h: a.height }; });
      check("AC-70 a phone: View's Terminal key has the keys' look (fill, radius, matte), a finger's height", look.fill && look.radius && look.image && parseFloat(look.h) >= 44, JSON.stringify(look));
    }
    check("no page errors (notes)", !f.errors.length, f.errors.join(" | "));
    await f.close();
  }
  runningIn(name);
}
