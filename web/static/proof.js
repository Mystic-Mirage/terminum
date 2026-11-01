// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The sign-in's proof and credential (AC-09, terminumd/src/auth/signin.rs), one copy for the
// page (signin.js) and the Files panel (ui/src/api.ts): the server proves it knows the secret,
// and the page makes the credential for that terminumd's boot; the secret never goes out.
const enc = new TextEncoder();
const hex = (bytes) => [...new Uint8Array(bytes)].map((b) => b.toString(16).padStart(2, "0")).join("");
async function hmac(key, text) {
  const k = await crypto.subtle.importKey("raw", key, { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
  return new Uint8Array(await crypto.subtle.sign("HMAC", k, enc.encode(text)));
}

/** Asks `/api/hello` with `fetchFn` (one that adds no sign-in): {state: "ok", cred, apps} when
 *  the answer proves terminumd knows `secret` (cred = "<id>.<HMAC(proof key, "tm-cred-v1:" +
 *  boot)>", apps: Finder and default apps exist there); else state "unknown" (terminumd does not
 *  know this sign-in: signed out), "down" (nothing answered) or "bad" (another program answered,
 *  or nothing to prove with: no crypto.subtle outside a secure context). */
export async function hello(secret, fetchFn) {
  if (!globalThis.crypto?.subtle) return { state: "bad" };
  const n = hex(crypto.getRandomValues(new Uint8Array(16)));
  const id = hex(await crypto.subtle.digest("SHA-256", enc.encode(secret))).slice(0, 16);
  let answer;
  try {
    const r = await fetchFn(`/api/hello?n=${n}&id=${id}`, { cache: "no-store" });
    answer = await r.json().catch(() => null);
    if (r.status === 404 && answer?.error === "unknown_signin") return { state: "unknown" };
  } catch { return { state: "down" }; }
  const key = await hmac(enc.encode(secret), "tm-proof-key");
  const want = hex(await hmac(key, `tm-hello-v1:${n}`));
  if (typeof answer?.proof !== "string" || answer.proof.toLowerCase() !== want || !/^[0-9a-f]{64}$/.test(answer.boot ?? "")) return { state: "bad" };
  return { state: "ok", cred: `${id}.${hex(await hmac(key, `tm-cred-v1:${answer.boot}`))}`, apps: answer.apps !== false };
}
