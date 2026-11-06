// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// AC-31 a table a real program printed, copied as a PNG on a real terminumd (e2e_live.mjs), at a
// secure address, so with the browser's own ClipboardItem. WebKit's write is a spy, as its clipboard
// may be the user's; Chromium's headless shell (this suite's) has a clipboard of its own, read back.
import fs from "node:fs";
import path from "node:path";
import { check, shownPane } from "./check.mjs";

const TABLE = ["┌────────┬──────┐", "│ Server │ Apps │", "├────────┼──────┤", "│ alpha  │   50 │", "├────────┼──────┤", "│ beta   │    3 │", "└────────┴──────┘"];

export async function liveWidgetPng(page, { tmux, work }) {
  const pane = await shownPane(page, { tmux, work }), file = path.join(work, "png-table.txt");
  fs.writeFileSync(file, TABLE.join("\n") + "\n");
  tmux("send-keys", "-t", pane, `clear; cat ${file}; echo done-png`, "Enter");
  const png = '.ln.wg-host .wg[data-kind="table"] [data-act="png"]';
  await page.waitForSelector(png, { timeout: 8000 });
  const real = page.context().browser().browserType().name() === "chromium";
  if (real) await page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
  else await page.evaluate(() => {
    navigator.clipboard.write = async ([item]) => { window.pngItem = { types: [...item.types], blob: await item.getType("image/png") }; };
  });
  await page.click(png);
  await page.waitForFunction(() => /Copied the table as PNG/.test(document.getElementById("toast")?.textContent ?? ""), null, { timeout: 5000 }).catch(() => {});
  const got = await page.evaluate(async (real) => {
    let types, blob;
    if (real) {
      const [item] = await navigator.clipboard.read();
      types = item.types;
      blob = await item.getType("image/png");
    } else ({ types, blob } = window.pngItem ?? {});
    if (!blob) return { types };
    const bitmap = await createImageBitmap(blob);
    return { types, type: blob.type, w: bitmap.width, h: bitmap.height };
  }, real).catch((e) => ({ error: e.message }));
  check(`AC-31 live: PNG on a table a program printed: a picture on the ${real ? "clipboard, read back" : "clipboard item"}`,
    got.type === "image/png" && got.types?.join() === "image/png" && got.w > 100 && got.h > 100, JSON.stringify(got));
  if (real) await page.context().clearPermissions();
  else await page.evaluate(() => { delete navigator.clipboard.write; });   // the spy, on this object only
  tmux("send-keys", "-t", pane, "clear", "Enter");
  fs.rmSync(file, { force: true });
}
