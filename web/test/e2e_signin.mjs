// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// AC-09 checks of the page's sign-in against a fake terminumd (fake.mjs): the password form
// gets a sign-in that goes into the origin's storage and never out again: once the server proved
// it knows it, the page makes a credential for its boot, which every request carries as
// X-TM-Session (an image too: fetched, shown from a blob:), the WebSocket as its subprotocol; a
// new terminumd (another boot) is proved again; a server that cannot prove itself gets nothing;
// a signed-out page
// drops its sign-in and shows the form; while no password is set the form says how to set one.

import crypto from "node:crypto";
import { check } from "./check.mjs";
import { drawn, openPage, pause } from "./fake.mjs";

const SECRET = "a".repeat(32) + "0123456789abcdef".repeat(2);
/** A request that gives the secret away (it never goes out after /auth/login's answer). */
const leaks = (r) => r.headers["x-tm-session"] === SECRET || r.query.includes(SECRET) || r.body.includes(SECRET) || (r.headers["sec-websocket-protocol"] ?? "").includes(SECRET);
/** A request signed in with a credential. */
const carries = (r) => !!r.headers["x-tm-session"];
const said = (page) => page.evaluate(() => document.getElementById("loginerr").textContent);

export async function webSignin(browser) {
  // a browser that never signed in: the form; the right password gets the sign-in
  let auth = { login: "password" }, tried = [];
  const login = (body) => {
    tried.push(body.password);
    if (body.password !== "correct horse") return [401, { reason: "wrong", retry: 0 }];
    auth = 204;
    return [200, { secret: SECRET }];
  };
  const f = await openPage(browser, { secret: SECRET, auth: () => auth, login, css: "body { --custom-probe: kept; }", settings: { page: {}, css: "1" } });
  await f.page.waitForSelector("#login:not([hidden])", { timeout: 5000 });
  const before = { hello: f.requests.some((r) => r.path === "/api/hello"), protocols: f.sockets[0]?.protocols ?? [] };
  await f.page.fill("#password", "wrong guess");
  await f.page.press("#password", "Enter");
  await f.page.waitForFunction(() => document.getElementById("loginerr").textContent.length > 0, null, { timeout: 3000 });
  const wrong = await said(f.page);
  await f.page.fill("#password", "correct horse");
  await f.page.press("#password", "Enter");
  await drawn(f.page);
  const state = await f.page.evaluate(() => ({ stored: localStorage.getItem("tm.session"), path: location.pathname + location.search,
    form: !document.getElementById("login").hidden, typed: document.getElementById("password").value }));
  await f.page.evaluate(() => fetch("/fb/api/state").catch(() => null));
  await f.page.evaluate(() => import("/signin.js").then((m) => m.blobUrl("/fb/api/raw?path=%2Ftmp%2Fa.png")));
  const img = f.requests.find((r) => r.path === "/fb/api/raw");
  const hello = f.requests.find((r) => r.path === "/api/hello"), first = f.requests.findIndex(carries);
  const q = new URLSearchParams(hello?.query ?? "");
  check("AC-09 sign-in: a new browser gets the password form and holds nothing (no proof, no header, no subprotocol)",
    !before.hello && !before.protocols.length && !f.requests.slice(0, 3).some(carries), JSON.stringify(before));
  check("AC-09 sign-in: a wrong password says so; the right one's sign-in goes into storage, never the address, and the form goes",
    wrong === "Wrong password." && JSON.stringify(tried) === JSON.stringify(["wrong guess", "correct horse"])
      && state.stored === SECRET && state.path === "/" && !state.form && state.typed === "", JSON.stringify({ wrong, tried, state }));
  check("AC-09 sign-in: the server proves itself first (/api/hello with a 32-hex nonce and the sign-in's id)",
    !!hello && /^[0-9a-f]{32}$/.test(q.get("n")) && q.get("id") === crypto.createHash("sha256").update(SECRET).digest("hex").slice(0, 16)
      && f.requests.indexOf(hello) < first, JSON.stringify({ hello, first }));
  const token = f.credential();
  check("AC-09 sign-in: requests carry the credential for the server's boot as X-TM-Session, the WebSocket the subprotocols tm and it, an image the header (never ?t=); the secret never goes out",
    f.requests.some((r) => r.path === "/fb/api/state" && r.headers["x-tm-session"] === token)
      && JSON.stringify(f.sockets.at(-1)?.protocols) === JSON.stringify(["tm", token])
      && img?.headers["x-tm-session"] === token && img.query === "?path=%2Ftmp%2Fa.png" && !f.requests.some(leaks) && !f.requests.some((r) => r.query.includes(token)),
    JSON.stringify({ protocols: f.sockets.at(-1)?.protocols, img }));

  // a new terminumd (an upgrade, a restart) draws another boot: the page proves it again and sends the request once more
  f.restart();
  const hellos = f.requests.filter((r) => r.path === "/api/hello").length;
  const status = await f.page.evaluate(() => fetch("/fb/api/state").then((r) => r.status, (e) => String(e)));
  const tries = f.requests.filter((r) => r.path === "/fb/api/state").slice(-2);
  check("AC-09 sign-in: a credential of another boot (401 bad_session): proved again, a new credential, the request sent once more; no form",
    status === 404 && f.requests.filter((r) => r.path === "/api/hello").length === hellos + 1 && tries.length === 2
      && tries[0].headers["x-tm-session"] === token && tries[1].headers["x-tm-session"] === f.credential() && token !== f.credential()
      && await f.page.isHidden("#login") && !f.requests.some(leaks), JSON.stringify({ status, tries: tries.map((r) => r.headers["x-tm-session"]?.slice(0, 8)) }));
  // several requests in flight when it restarts: each is proved again and sent once more, none is "signed out"
  f.restart();
  const many = await f.page.evaluate(() => Promise.all([1, 2, 3, 4, 5].map((i) => fetch(`/fb/api/state?i=${i}`).then((r) => r.status, (e) => String(e)))));
  check("AC-09 sign-in: 5 requests in flight across a restart all go through again (no 401 left, no form)",
    many.every((st) => st === 404) && await f.page.isHidden("#login") && !f.requests.some(leaks), JSON.stringify(many));

  // custom.css as last fetched is applied at once at the next load (it came after the connection), and goes with the sign-in (AC-19)
  await f.page.waitForFunction(() => localStorage.getItem("tm.customcss")?.includes("kept"), null, { timeout: 3000 }).catch(() => {});
  let held = 0;
  await f.page.route("**/custom.css", () => { held++; });               // never answered: what shows is the copy kept
  await f.page.reload();
  await drawn(f.page);
  const early = await f.page.evaluate(() => getComputedStyle(document.body).getPropertyValue("--custom-probe").trim());
  check("AC-19 custom.css: the copy last fetched applies at once at load, while the new fetch is still on its way", early === "kept" && held > 0, JSON.stringify({ early, held }));
  await f.page.unroute("**/custom.css");

  // Sign out in the menu: this browser's sign-in ends, at terminumd and here
  await f.page.evaluate(() => document.getElementById("signout").click());
  await f.page.waitForSelector("#login:not([hidden])", { timeout: 3000 }).catch(() => {});
  const out = f.requests.find((r) => r.path === "/auth/logout");
  const left = await f.page.evaluate(() => [localStorage.getItem("tm.session"), localStorage.getItem("tm.customcss")]);
  check("AC-09 sign-in: Sign out tells terminumd which sign-in ends, forgets it (custom.css's copy with it) and shows the form",
    out?.headers["x-tm-session"] === f.credential() && left[0] === null && left[1] === null && await f.page.isVisible("#login"), JSON.stringify({ out: !!out, left }));
  check("AC-09 sign-in: no page errors", !f.errors.length, f.errors.join(" | "));
  await f.close();

  // signed out elsewhere (terminum signout, a new password): the form, the sign-in dropped, the page kept
  let auth2 = 204;
  const gone = await openPage(browser, { stored: SECRET, secret: SECRET, auth: () => auth2 });
  await drawn(gone.page);
  auth2 = { login: "password" };
  await gone.page.evaluate(() => { window.kept = true; });
  gone.sockets[0].close();
  await gone.page.waitForSelector("#login:not([hidden])", { timeout: 3000 }).catch(() => {});
  const after = await gone.page.evaluate(() => ({ kept: window.kept === true, stored: localStorage.getItem("tm.session") }));
  check("AC-09 sign-in: a signed-out page says \"Signed out: sign in again.\" in the form, drops its sign-in and stays (no reload)",
    await said(gone.page) === "Signed out: sign in again." && after.kept && after.stored === null, JSON.stringify(after));
  await gone.close();

  // a Sign out terminumd could not save: the sign-in ends here all the same, and the form says what it answered
  const unsaved = await openPage(browser, { stored: SECRET, secret: SECRET, auth: 204 });
  await drawn(unsaved.page);
  const why503 = "Signed out, but signins.json could not be written (disk full): until terminum signout succeeds on the Mac, a restart of terminumd would let this sign-in back.";
  await unsaved.page.route("**/auth/logout", (route) => route.fulfill({ status: 503, contentType: "application/json", body: JSON.stringify({ error: "signout_unsaved", message: why503 }) }));
  await unsaved.page.evaluate(() => document.getElementById("signout").click());
  await unsaved.page.waitForSelector("#login:not([hidden])", { timeout: 3000 }).catch(() => {});
  check("AC-09 sign-in: a Sign out terminumd could not save forgets the sign-in and says terminumd's message in the form",
    await said(unsaved.page) === why503 && await unsaved.page.evaluate(() => localStorage.getItem("tm.session")) === null, await said(unsaved.page));
  await unsaved.close();
  const unheard = await openPage(browser, { stored: SECRET, secret: SECRET, auth: 204 });
  await drawn(unheard.page);
  await unheard.page.route("**/auth/logout", (route) => route.abort());
  await unheard.page.evaluate(() => document.getElementById("signout").click());
  await unheard.page.waitForSelector("#login:not([hidden])", { timeout: 3000 }).catch(() => {});
  check("AC-09 sign-in: a Sign out terminumd never heard forgets the sign-in and says it may still be live, and how to end it",
    (await said(unheard.page)).includes("run: terminum signout") && await unheard.page.evaluate(() => localStorage.getItem("tm.session")) === null, await said(unheard.page));
  await unheard.close();

  // no password set yet: the form says how to set one and waits
  const none = await openPage(browser, { secret: SECRET, auth: { login: "none", message: "No password is set yet. On the Mac, run: terminum password" } });
  await none.page.waitForSelector("#login:not([hidden])", { timeout: 5000 });
  check("AC-09 sign-in: while no password is set the form says \"On the Mac, run: terminum password\" and cannot be sent",
    await said(none.page) === "No password is set yet. On the Mac, run: terminum password" && await none.page.isDisabled("#password"),
    await said(none.page));
  await none.close();

  // another program on the port: it cannot prove the secret, so it never gets it
  const evil = await openPage(browser, { stored: SECRET, secret: "not-the-secret" });
  await evil.page.waitForFunction(() => document.getElementById("status").textContent.length > 0, null, { timeout: 5000 }).catch(() => {});
  await pause(evil.page, 2500);                    // a retry or two
  const why = await evil.page.evaluate(() => document.getElementById("status").textContent);
  const evilImg = await evil.page.evaluate(() => import("/signin.js").then((m) => m.blobUrl("/fb/api/raw?path=%2Ftmp%2Fa.png")));
  check("AC-09 sign-in: a server that cannot prove itself never sees the secret: no header, no image fetched with it, no WebSocket; the page says why",
    !evil.requests.some(leaks) && !evil.requests.some(carries) && evil.sockets.length === 0 && !evilImg.url && !!evilImg.error && why === "terminum's server could not prove itself"
      && evil.requests.filter((r) => r.path === "/api/hello").length >= 2, JSON.stringify({ why, sockets: evil.sockets.length, asked: evil.requests.map((r) => r.path) }));
  await evil.close();

  // a sign-in terminumd does not know (revoked, another install): dropped, nothing sent
  const unknown = await openPage(browser, { stored: SECRET, secret: "unknown_signin", auth: { login: "password" } });
  await unknown.page.waitForSelector("#login:not([hidden])", { timeout: 5000 }).catch(() => {});
  const asked = unknown.requests.length;
  await pause(unknown.page, 2500);
  const was = await unknown.page.evaluate(() => localStorage.getItem("tm.session"));
  check("AC-09 sign-in: a sign-in /api/hello does not know (404 unknown_signin) is dropped once: the form, nothing sent, nothing retried",
    await said(unknown.page) === "Signed out: sign in again." && was === null && !unknown.requests.some(leaks) && !unknown.requests.some(carries) && unknown.sockets.length === 0
      && unknown.requests.length === asked, JSON.stringify({ was, more: unknown.requests.slice(asked).map((r) => r.path) }));
  await unknown.close();
}
