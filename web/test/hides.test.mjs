// One rule of what hides what a line runs, in the page and in terminumd: the page's list
// (seltext.js HIDE) holds the very code points of terminumd's (termtext.rs hides).
import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import { HIDE } from "../static/seltext.js";

test("seltext.js HIDE and termtext.rs hides match every code point", () => {
  const rs = fs.readFileSync(new URL("../../terminumd/src/termtext.rs", import.meta.url), "utf8");
  const body = rs.slice(rs.indexOf("pub fn hides")).match(/matches!\(c as u32, ([^)]*)\)/)[1];
  const ranges = body.split("|").map((t) => t.trim().split("..=").map((h) => parseInt(h, 16)));
  assert.ok(ranges.length > 5 && ranges.every((r) => r.every(Number.isFinite)), body);
  const inRust = (cp) => ranges.some(([a, b = a]) => cp >= a && cp <= b);
  const odd = [];
  for (let cp = 0; cp <= 0x10ffff; cp++) {
    if (cp >= 0xd800 && cp <= 0xdfff) continue;
    if (HIDE.test(String.fromCodePoint(cp)) !== inRust(cp)) odd.push(cp.toString(16));
  }
  assert.deepEqual(odd, []);
});
