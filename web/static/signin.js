// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The page's sign-in (AC-09). The password (POST /auth/login, the form in conn.js) gets a
// secret that this page keeps in its origin's storage (that origin only: 127.0.0.1:48080, or
// the tailnet name), never in its address or a cookie, and never sends: once the server proved
// it knows it (/api/hello), the page makes a credential for that terminumd's boot (an HMAC of
// it), which every same-origin request carries as X-TM-Session (an image is fetched with it:
// blobUrl), the WebSocket as its subprotocol. The next terminumd draws another boot, so a
// program that takes the port while terminumd is down gets nothing it would accept.
// Imported first by app.js, before anything fetches.
import { hello } from "./proof.js";
import { shownUrl } from "./shown.js";

const KEY = "tm.session";
/** The user's custom.css as last fetched: shown at the next load before it is fetched again; it
 *  needs the sign-in, so it goes with it. */
const CSS_KEY = "tm.customcss";
let secret = null;
/** The current proof's credential (memory only), for the WebSocket (protocols). */
let token = null;
try { secret = localStorage.getItem(KEY); } catch { secret = null; }

/** The sign-in the password got: kept, and sent from now on. */
export function adopt(s) {
  secret = s;
  proof = null; token = null;
  try { localStorage.setItem(KEY, s); } catch { /* private mode: kept for this page only */ }
}

/** Whether the page holds a sign-in. */
export const held = () => secret !== null;

/** A signed-out sign-in is dropped: it is never sent again. */
export function forget() {
  secret = null;
  proof = null; token = null;
  try { localStorage.removeItem(KEY); localStorage.removeItem(CSS_KEY); } catch { /* nothing kept */ }
}

// One proof per connection: asked before the secret first goes out, and again after the
// connection dropped (invalidate), as another program may hold the port now.
let proof = null;
/** The proof for the current connection (asked when there is none): proof.js hello's answer. */
function proving() {
  const s = secret;
  if (!s) return Promise.resolve({ state: "ok" });
  if (!proof) {
    const mine = proof = hello(s, realFetch).catch(() => ({ state: "bad" }));
    mine.then((r) => {
      if (proof !== mine) return;
      slow = r.waited ?? 0;
      if (r.state === "ok") token = r.cred;
      else proof = null;                             // asked again next time
    });
  }
  return proof;
}
let slow = 0;
/** The ms the last proof waited for an answer that never came (0: it answered, or nothing took the request). */
export const unanswered = () => slow;
/** The proof's state for the current connection: "ok", "bad", "unknown" or "down". */
export const prove = () => proving().then((r) => r.state);
/** The connection dropped: prove the server again, and make a new credential, before a request goes out. */
export function invalidate() { proof = null; token = null; }

const realFetch = window.fetch.bind(window);
/** A request no answer came to says so in terminum's words: the browser's say nothing. */
const reached = (go) => go().catch((e) => { throw e.name === "AbortError" || e.name === "TimeoutError" ? e : new TypeError("terminum's server does not answer."); });
let refused = () => {};
/** fn(known) when a request of the page's is refused as signed out: a 401 (known: false, ask
 *  /auth/check why) or a sign-in terminumd does not know (known: true). */
export function onRefused(fn) { refused = fn; }

/** One same-origin request with the credential of the proof it waited for (never another
 *  request's, which a restart may have dropped meanwhile): [answer, that proof]. */
async function signed(input, init) {
  const mine = proving(), p = await mine;
  if (p.state === "unknown") refused(true);
  if (p.state !== "ok") throw new TypeError(p.state === "unknown" ? "Signed out: sign in again" : p.state === "down" ? "terminum's server is not running" : "terminum's server could not prove itself");
  const headers = new Headers(init.headers ?? (input instanceof Request ? input.headers : undefined));
  if (p.cred) headers.set("X-TM-Session", p.cred);
  try { return [await reached(() => realFetch(input, { ...init, headers })), mine]; }
  catch (e) { if (proof === mine) invalidate(); throw e; }   // the server may be gone: prove the next one again
}

// Every same-origin request carries the credential; a refused one is reported. A 401
// bad_session may be a new terminumd, of another boot (an upgrade, a restart): it is proved
// again and the request, which no handler ran, sent once more; only a second 401 is reported.
// Requests elsewhere (none: CSP connect-src 'self') get nothing.
window.fetch = async (input, init = {}) => {
  const url = new URL(input instanceof Request ? input.url : String(input), location.href);
  if (url.origin !== location.origin) return realFetch(input, init);
  if (!secret) return reached(() => realFetch(input, init));
  let [r, mine] = await signed(input, init);
  const again = !(input instanceof Request) && !(init.body instanceof ReadableStream);   // a body that can be sent twice
  const stale = r.status === 401 && again && secret && (await r.clone().json().catch(() => null))?.error === "bad_session";
  if (stale) {
    if (proof === mine) invalidate();               // not a newer proof another request made meanwhile
    [r] = await signed(input, init);
  }
  if (r.status === 401 && url.pathname !== "/auth/check" && url.pathname !== "/auth/login") refused(false);
  return r;
};

/** A same-origin file for an <img>, which sends no header: fetched with the sign-in and shown
 *  from a blob: address (an SVG's a data:, shown.js), so no address on the page carries it
 *  (Copy Image Address, AC-09). {url} (the caller revokes it) or {error}: terminumd's message. */
export async function blobUrl(url) {
  let r;
  try { r = await fetch(url); } catch (e) { return { error: e.message || String(e) }; }
  if (!r.ok) return { error: (await r.json().catch(() => ({}))).message ?? `HTTP ${r.status}` };
  return { url: await shownUrl(await r.blob()) };
}

/** The WebSocket's subprotocols: ["tm", credential] once the server proved itself (prove). */
export const protocols = () => (secret && token ? ["tm", token] : undefined);
/** terminumd's WebSocket, on the page's own host. */
export const socketUrl = () => `${location.protocol === "https:" ? "wss:" : "ws:"}//${location.host}/ws`;

/** The copy last fetched of the user's custom.css (AC-19), shown at once at load. */
export function showKeptCss() {
  try { if (held()) document.getElementById("customcss").textContent = localStorage.getItem(CSS_KEY) ?? ""; } catch { /* no storage: fetched next */ }
}
let cssLoads = 0;
/** The user's custom.css, fetched with the sign-in, shown and kept; the newest asked stands. */
export async function loadCss() {
  const mine = ++cssLoads;
  const r = await fetch("/custom.css", { cache: "no-store" }).catch(() => null);
  const text = r?.ok ? await r.text().catch(() => null) : null;
  if (text === null || mine !== cssLoads) return;
  document.getElementById("customcss").textContent = text;
  try { localStorage.setItem(CSS_KEY, text); } catch { /* no storage: fetched at every load */ }
}

let updating = false;
/** terminumd names the build `id`: a page of another (open across an upgrade, or restored from memory)
 *  reloads once, never in a loop; it waits while `busy()`. `say(words)` hears why it has not yet. */
export function reloadOnce(id, busy, say = () => {}) {
  if (id === document.querySelector('meta[name="build"]').content || updating) return;
  if (sessionStorage.getItem("web.reloaded-for") === id) return say("An update is ready: reload the page.");
  updating = true;
  const attempt = () => {
    if (busy()) {
      say("An update is ready: the page reloads after you save.");
      setTimeout(attempt, 1000);
      return;
    }
    try { sessionStorage.setItem("web.reloaded-for", id); } catch { /* private mode: at worst asks again */ }
    location.reload();
  };
  attempt();
}
