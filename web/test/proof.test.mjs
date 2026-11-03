// The sign-in's proof (AC-09): a server that takes the request and never finishes its answer
// counts as down within the deadline, so the connection tries again.
import { test } from "node:test";
import assert from "node:assert/strict";
import http from "node:http";
import { DEADLINE, hello, helloWait } from "../static/proof.js";

/** A local server answering with `handler`; resolves to its base URL and a close function. */
async function serve(handler) {
  const held = new Set();
  const srv = http.createServer((req, res) => { held.add(res); handler(req, res); });
  await new Promise((r) => srv.listen(0, "127.0.0.1", r));
  const base = `http://127.0.0.1:${srv.address().port}`;
  const close = () => { for (const r of held) r.destroy(); srv.closeAllConnections(); return new Promise((r) => srv.close(r)); };
  return { fetchAt: (path, init) => fetch(base + path, init), close };
}

async function timed(fn) {
  const t = performance.now();
  const r = await fn();
  return [r, performance.now() - t];
}

test("no answer at all: down within the deadline", async () => {
  const s = await serve(() => {});
  try {
    const [r, ms] = await timed(() => hello("secret", s.fetchAt, 300));
    assert.equal(r.state, "down");
    assert.ok(ms < 2000, `took ${ms} ms`);
  } finally { await s.close(); }
});

test("headers sent, body never finished: down within the deadline", async () => {
  const s = await serve((_req, res) => { res.writeHead(200, { "Content-Type": "application/json" }); res.write('{"proof":'); });
  try {
    const [r, ms] = await timed(() => hello("secret", s.fetchAt, 300));
    assert.equal(r.state, "down");
    assert.ok(ms < 2000, `took ${ms} ms`);
  } finally { await s.close(); }
});

test("a complete wrong answer is still bad, not down", async () => {
  const s = await serve((_req, res) => { res.writeHead(200, { "Content-Type": "application/json" }); res.end('{"proof":"00"}'); });
  try { assert.equal((await hello("secret", s.fetchAt, 300)).state, "bad"); } finally { await s.close(); }
});

test("an unknown sign-in answered in time: unknown", async () => {
  const s = await serve((_req, res) => { res.writeHead(404, { "Content-Type": "application/json" }); res.end('{"error":"unknown_signin"}'); });
  try { assert.equal((await hello("secret", s.fetchAt, 300)).state, "unknown"); } finally { await s.close(); }
});

test("no answer in time, again and again: each waits longer, up to 20 s; an answer starts over", async () => {
  const hang = await serve(() => {}), fine = await serve((_req, res) => { res.writeHead(404, { "Content-Type": "application/json" }); res.end('{"error":"unknown_signin"}'); });
  try {
    const waits = [];
    for (let i = 0; i < 4; i++) {
      const r = await hello("secret", hang.fetchAt, 50);
      assert.deepEqual(r, { state: "down", waited: 50 }, "a request taken and not answered says how long it waited");
      waits.push(helloWait());
    }
    assert.deepEqual(waits, [2, 4, 4, 4].map((k) => k * DEADLINE));
    await hello("secret", fine.fetchAt, 300);
    assert.equal(helloWait(), DEADLINE);
  } finally { await hang.close(); await fine.close(); }
});
