// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The Files panel's documents (e2e_panel.mjs): Markdown and HTML render inert (raw HTML as text,
// no script), their relative images come from blob: addresses without the sign-in, local links
// open other documents in tabs, nothing loads from outside terminumd (AC-54, AC-67, AC-09).
import { slack } from "../../web/test/check.mjs";
import { check, within } from "./e2e_harness.mjs";

/** row(p): a tree row's selector; SECRET: the sign-in; outside, blocked: the page's requests
 *  that left terminumd and those the CSP stopped, as e2e_panel.mjs records them. */
export async function panelDocs(page, { row, SECRET, outside, blocked }) {
  // AC-54 / AC-67 Markdown
  await page.click(row("README.md"));
  await page.waitForSelector(".md-body a");
  const inert = await page.$eval(".md-body", (el) => !el.querySelector("img[src='x']") && el.textContent.includes("<img src=x"));
  check("AC-54 raw HTML shown as text", inert);
  // fetched with the sign-in's header and shown from a blob: (AC 09): no address on the page carries it
  await page.waitForFunction(() => document.querySelector(".md-body img")?.src.startsWith("blob:"), null, { timeout: 5000 }).catch(() => {});
  const width = await page.$eval(".md-body img", (i) => new Promise((r) => (i.complete ? r(i.naturalWidth) : ((i.onload = () => r(i.naturalWidth)), (i.onerror = () => r(-1))))));
  const mdSrc = await page.$eval(".md-body img", (i) => i.getAttribute("src"));
  check("AC-54 relative image loads, from a blob: address without the sign-in", width === 1 && mdSrc.startsWith("blob:"), `width ${width} ${mdSrc}`);
  const url = page.url();
  await page.click('.md-body a:text("guide")');
  await within("AC-67 local .md link opens a tab", (ms) => page.waitForSelector('.tab.active .tname:text("guide.md")', { timeout: ms }));
  await page.click('.md-body a:text("readme")');
  await within("AC-67 relative link back", (ms) => page.waitForSelector('.tab.active .tname:text("README.md")', { timeout: ms }));
  check("AC-67 panel never navigated", page.url() === url);

  // AC-67 HTML documents render; links inside Markdown and HTML open other documents
  await page.click('.tab .tname:text("guide.md")');
  await page.click('.md-body a:text("the page")');
  await within("AC-67 md link opens an HTML document rendered", (ms) => page.waitForSelector(".tab.active .tname:text('page.html')", { timeout: ms }).then(() => page.waitForSelector("iframe.html-doc", { timeout: ms })));
  const frame = page.frameLocator("iframe.html-doc");
  await within("AC-67 HTML content shown", (ms) => frame.locator("h1:text('HTML page')").waitFor({ timeout: ms }));
  await frame.locator('img[src^="blob:"]').first().waitFor({ timeout: 5000 }).catch(() => {});
  const imgW = await frame.locator("img").first().evaluate((i) => new Promise((r) => (i.complete ? r(i.naturalWidth) : ((i.onload = () => r(i.naturalWidth)), (i.onerror = () => r(-1))))));
  check("AC-67 relative image inside HTML loads (blob:, no sign-in in the document)", imgW === 1 && !(await frame.locator("html").evaluate((h, s) => h.outerHTML.includes(s), SECRET)), `width ${imgW}`);
  check("AC-67 scripts in HTML do not run", (await page.title()) !== "PWNED");
  const planted = await frame.locator("html").evaluate((h) => ({ img: h.querySelector("#planted")?.getAttribute("src") ?? null, css: [...h.querySelectorAll("style")].some((s) => s.textContent.includes("localhost")) }));
  check("AC-67 an HTML file's own data-raw reads nothing with the sign-in (only its local src and href are fetched)", planted.img === null && !planted.css, JSON.stringify(planted));
  await page.waitForTimeout(500);
  check("AC-67 HTML loads nothing from outside terminumd", outside.length === 0, outside.join(" "));
  check("AC-67 CSP blocked the external image and CSS url()", blocked.length >= 2, `${blocked.length} blocked`);
  check("AC-67 panel URL keeps no sign-in", !/[?&]t=/.test(page.url()) && !page.url().includes(SECRET), page.url());
  const sec2Top = await frame.locator("#sec2").evaluate((el) => el.getBoundingClientRect().top);
  check("AC-67 '#sec2' anchor scrolled into view", sec2Top < 800, `top ${Math.round(sec2Top)}`);
  check("AC-67 HTML has a Source toggle", !!(await page.$('#tools button[data-act="source"]')));
  // a blob: has the panel's origin: opened as a page (Open Video in New Tab) an HTML or SVG one would
  // run there with the sign-in in reach, so only a picture, a video or a sound keeps its type (AC-09);
  // the panel's policy fetches no blob:, so each is opened as a page, as the menu would
  await frame.locator('#clip[src^="blob:"]').waitFor({ timeout: 5000 }).catch(() => {});
  const shown = await frame.locator("html").evaluate((h) => ({ clip: h.querySelector("#clip")?.getAttribute("src") ?? "", vec: h.querySelector("#vec")?.getAttribute("src") ?? "" }));
  const opened = async (u) => {
    const tab = await page.context().newPage();
    const how = await tab.goto(u).then(() => "page", (e) => (/Download is starting/.test(e.message) ? "download" : /ERR_FILE_NOT_FOUND/.test(e.message) ? "gone" : e.message.split("\n")[0]));
    await tab.waitForTimeout(300);
    await tab.close();
    return how;
  };
  const asPages = { clip: await opened(shown.clip), vec: await opened(shown.vec) };
  check("AC-09 an HTML document's video named after an HTML file is bytes (opened, it downloads), its SVG picture a data: address",
    shown.clip.startsWith("blob:") && asPages.clip === "download" && shown.vec.startsWith("data:image/svg+xml"), JSON.stringify({ asPages, vec: shown.vec.slice(0, 30) }));
  check("AC-09 opened as pages, they ran nothing in the panel's origin", !(await page.evaluate(() => localStorage.getItem("pwned"))));
  await frame.locator("a:text('to readme')").click();
  await within("AC-67 link inside HTML opens the Markdown document", (ms) => page.waitForSelector('.tab.active .tname:text("README.md")', { timeout: ms }));

  // the video's address is let go once its document closed (it lived as long as the panel did)
  await page.locator(".tab", { has: page.locator('.tname:text("page.html")') }).locator(".close").click();
  // let go 5 s after the close: asked until then, later on a busy machine
  let revoked = false;
  for (const until = Date.now() + slack(8000); !revoked && Date.now() < until;) {
    await page.waitForTimeout(1000);
    revoked = (await opened(shown.clip)) === "gone";
  }
  check("AC-67 a video's blob: address is revoked once its HTML document's tab closed", revoked, shown.clip);
}
