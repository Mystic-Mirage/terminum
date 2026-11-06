// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// AC-61 checks for e2e_panel.mjs: many panels at once each load, because events come over a
// WebSocket (a browser keeps 6 HTTP connections per host), and terminumd refuses a socket from
// another origin or without the sign-in (its subprotocols ["tm", secret]).

import http from "node:http";
import { check, panelUrl, PORT, within } from "./e2e_harness.mjs";

const PANELS = 12;

/** The status terminumd answers a WebSocket upgrade of the panel's socket with. */
const upgrade = (headers) => new Promise((resolve) => {
  const req = http.get({ host: "127.0.0.1", port: PORT, path: "/fb/api/ws", headers: {
    Connection: "Upgrade", Upgrade: "websocket", "Sec-WebSocket-Version": "13", "Sec-WebSocket-Key": "dGhlIHNhbXBsZSBub25jZQ==", ...headers,
  } });
  req.on("upgrade", (res, socket) => { socket.destroy(); resolve(res.statusCode); });
  req.on("response", (res) => { res.resume(); resolve(res.statusCode); });
  req.on("error", () => resolve(0));
});

export async function manyPanels(browser, row, secret) {
  const origin = `http://127.0.0.1:${PORT}`, signed = { "Sec-WebSocket-Protocol": `tm, ${secret}` };
  check("AC-61 the panel's origin with the sign-in opens the socket", await upgrade({ Origin: origin, ...signed }) === 101);
  check("AC-61 another origin is refused", await upgrade({ Origin: "http://evil.example", ...signed }) === 403);
  check("AC-61 no origin is refused", await upgrade(signed) === 403);
  check("AC-61 no sign-in is refused", await upgrade({ Origin: origin }) === 401);
  check("AC-61 a wrong sign-in is refused", await upgrade({ Origin: origin, "Sec-WebSocket-Protocol": `tm, ${"0".repeat(64)}` }) === 401);

  const ctx = await browser.newContext({ viewport: { width: 400, height: 500 } });
  await ctx.addInitScript((s) => localStorage.setItem("tm.session", s), secret);
  const pages = await Promise.all(Array.from({ length: PANELS }, () => ctx.newPage()));
  const sockets = [];
  for (const p of pages) p.on("websocket", (ws) => sockets.push(ws.url()));
  await within(`AC-61 ${PANELS} panels at once all load their tree`, async (ms) => {
    await Promise.all(pages.map(async (p) => {
      await p.goto(panelUrl(), { timeout: ms });
      await p.waitForSelector(row("src"), { timeout: ms });
    }));
  }, 15000);
  check(`AC-61 … each over its own event socket, no secret in its URL`, sockets.filter((u) => u.endsWith("/fb/api/ws")).length === PANELS && sockets.every((u) => !u.includes("t=")), sockets.join(" "));
  await ctx.close();
}
