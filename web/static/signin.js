// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The page's sign-in (AC-09). The password (POST /auth/login, the form in conn.js) gets a
// secret that this page keeps in its origin's storage (that origin only: 127.0.0.1:48080, or
// the tailnet name), never in its address or a cookie, and never sends: once the server proved
// it knows it (/api/hello), the page makes a credential for that terminumd's boot (an HMAC of
// it), which every same-origin request carries as X-TM-Session (an image is fetched with it:
// blobUrl), the WebSocket as its subprotocol. The next terminumd draws another boot, so a
// program that takes the port while terminumd is down gets nothing it would accept.
// Imported first by app.js, before anything fetches.
import { shownUrl } from "./shown.js";

const KEY = "tm.session";
/** The user's custom.css as last fetched (settings.js): shown at the next load before it is
 *  fetched again; it needs the sign-in, so it goes with it. */
export const CSS_KEY = "tm.customcss";
const enc = new TextEncoder();
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

const hex = (bytes) => [...new Uint8Array(bytes)].map((b) => b.toString(16).padStart(2, "0")).join("");
async function hmac(key, text) {
  const k = await crypto.subtle.importKey("raw", key, { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
  return new Uint8Array(await crypto.subtle.sign("HMAC", k, enc.encode(text)));
}

/** The server's proof that it knows the secret, then the credential for its boot: {state,
 *  cred}, state "ok" (cred made), "bad" (another program answered, or nothing it could prove
 *  with), "unknown" (terminumd does not know this sign-in: signed out), or "down" (nothing answered). */
async function proveNow(s) {
  if (!crypto.subtle) return { state: "bad" };
  const n = hex(crypto.getRandomValues(new Uint8Array(16)));
  const id = hex(await crypto.subtle.digest("SHA-256", enc.encode(s))).slice(0, 16);
  let answer;
  try {
    const r = await realFetch(`/api/hello?n=${n}&id=${id}`, { cache: "no-store" });
    answer = await r.json().catch(() => null);
    if (r.status === 404 && answer?.error === "unknown_signin") return { state: "unknown" };
  } catch { return { state: "down" }; }
  const key = await hmac(enc.encode(s), "tm-proof-key");
  const want = hex(await hmac(key, `tm-hello-v1:${n}`));
  if (typeof answer?.proof !== "string" || answer.proof.toLowerCase() !== want || !/^[0-9a-f]{64}$/.test(answer.boot ?? "")) return { state: "bad" };
  return { state: "ok", cred: `${id}.${hex(await hmac(key, `tm-cred-v1:${answer.boot}`))}` };
}

// One proof per connection: asked before the secret first goes out, and again after the
// connection dropped (invalidate), as another program may hold the port now.
let proof = null;
/** The proof for the current connection (asked when there is none): proveNow's answer. */
function proving() {
  const s = secret;
  if (!s) return Promise.resolve({ state: "ok" });
  if (!proof) {
    const mine = proof = proveNow(s).catch(() => ({ state: "bad" }));
    mine.then((r) => {
      if (proof !== mine) return;
      if (r.state === "ok") token = r.cred;
      else proof = null;                             // asked again next time
    });
  }
  return proof;
}
/** The proof's state for the current connection: "ok", "bad", "unknown" or "down". */
export const prove = () => proving().then((r) => r.state);
/** The connection dropped: prove the server again, and make a new credential, before a request goes out. */
export function invalidate() { proof = null; token = null; }

const realFetch = window.fetch.bind(window);
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
  try { return [await realFetch(input, { ...init, headers }), mine]; }
  catch (e) { if (proof === mine) invalidate(); throw e; }   // the server may be gone: prove the next one again
}

// Every same-origin request carries the credential; a refused one is reported. A 401
// bad_session may be a new terminumd, of another boot (an upgrade, a restart): it is proved
// again and the request, which no handler ran, sent once more; only a second 401 is reported.
// Requests elsewhere (none: CSP connect-src 'self') get nothing.
window.fetch = async (input, init = {}) => {
  const url = new URL(input instanceof Request ? input.url : String(input), location.href);
  if (url.origin !== location.origin) return realFetch(input, init);
  if (!secret) return realFetch(input, init);
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
