// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The files a pane's output names (AC-35, AC-52, AC-56): where a printed path is (a relative one from
// the folder its coding agent works in, as its hooks said, then from the pane's; "~" from its home;
// on its host) and whether a file is there.
// Only a file that is there gets an underline or a click. A file is looked up by
// listing its folder through the proxy (nothing is read): one page answers every name of a
// folder of up to 500 entries; a larger one is asked by name.
import { absolutePath } from "./links.js";

const PANE_FOR = 5000;       // ms the pane's folder is taken as known (a "cd" moves it)
const AGAIN = [10000, 30000, 60000];   // ms before a file not there is looked for again, then again…
const KEEP_FOUND = 60000;    // ms a file found is taken as there (it may be deleted)
const AT_ONCE = 4;           // listings asked for at once
const PAGE = 500;            // entries of a folder's page (fbd's most)
const QUEUED = 200;          // folders waiting, at most: the newest asked first, the oldest dropped
const KEPT = 3000;           // answers kept, the oldest go first
const SAY_EVERY = 250;       // ms between two "files turned up" (the page marks links again)

/** paneInfo(): the shown pane's {cwd, home, host, agent} or {error}, null when the Mac does not answer.
 *  fetchJson(url): the answer's JSON, null when it failed (tests give their own). */
/** A listing's page once read: a large folder still being read is asked again, three times at most. */
export async function settled(ask) {
  let page = await ask();
  for (let tries = 0; page?.status === "loading" && tries < 3; tries++) {
    await new Promise((r) => setTimeout(r, 400));
    page = await ask();
  }
  return page;
}

export function paneFiles(paneInfo, fetchJson = (url) => fetch(url).then((r) => (r.ok ? r.json() : null), () => null)) {
  let pane = null, paneAt = -Infinity, asking = null;
  let found = new Map();     // host\npath -> {ok, at, tries, pending}
  const queue = [];          // folders to list, {host, dir, names: Set}
  const listeners = new Set();
  let busy = 0, gen = 0, saying = 0, soon = 0;
  let requesting = 0;
  const waiting = [];

  async function fetchPage(url, mine) {
    if (requesting < AT_ONCE) requesting++;
    else await new Promise((resolve) => waiting.push(resolve));
    try { return mine === gen ? await fetchJson(url) : null; }
    finally {
      const next = waiting.shift();
      if (next) next(); else requesting--;
    }
  }

  const say = () => { if (!saying) saying = setTimeout(() => { saying = 0; for (const fn of listeners) fn(); }, SAY_EVERY); };
  // the pane's folder, asked again at most every PANE_FOR ms; a new one means new places
  function refresh() {
    if (asking || performance.now() - paneAt < PANE_FOR) return asking;
    const mine = gen;
    asking = paneInfo().then((info) => {
      asking = null;
      if (mine !== gen) return;
      paneAt = performance.now();
      const moved = JSON.stringify([pane?.cwd, pane?.agent, pane?.home, pane?.host, pane?.error]) !== JSON.stringify([info?.cwd, info?.agent, info?.home, info?.host, info?.error]);
      pane = info;
      if (moved) say();
    }, () => { asking = null; });
    return asking;
  }
  const query = (params, host) => { const q = new URLSearchParams(params); if (host) q.set("host", host); return q; };
  // where a printed path may be, the likelier first: from the agent's folder, then from the pane's
  const placesOf = (value, p = pane) => {
    if (!p || p.error) return [];
    const paths = [...new Set([p.agent, p.cwd].filter(Boolean).map((base) => absolutePath(value, base, p.home)).filter(Boolean))];
    return paths.map((path) => ({ host: p.host ?? null, path, dir: path.slice(0, path.lastIndexOf("/")) || "/", name: path.slice(path.lastIndexOf("/") + 1) }));
  };
  const keyOf = (host, path) => `${host ?? ""}\n${path}`;
  const known = (place) => found.get(keyOf(place.host, place.path));
  /** The first of `value`'s places whose file is there, as far as known now. */
  const foundAt = (value) => placesOf(value).find((place) => known(place)?.ok) ?? null;

  function remember(host, dir, name, ok) {
    const k = keyOf(host, dir === "/" ? `/${name}` : `${dir}/${name}`), was = found.get(k);
    found.delete(k);
    found.set(k, { ok, at: performance.now(), tries: ok ? 0 : (was?.tries ?? 0) + 1 });
    if (found.size > KEPT) found.delete(found.keys().next().value);
  }
  async function list(job) {
    const mine = gen;
    const isFile = (e) => e.k === "f" || e.k === "l";
    const ask = (params) => fetchPage(`/fb/api/ls?${query({ path: job.dir, limit: String(PAGE), ...params }, job.host)}`, mine);
    const page = await settled(() => ask({}));
    if (!page || page.status !== "ready") return [...job.names].map((n) => [n, false]);
    if (page.total <= page.entries.length) {
      const files = new Set(page.entries.filter(isFile).map((e) => e.n));
      return [...job.names].map((n) => [n, files.has(n)]);
    }
    return Promise.all([...job.names].map(async (n) => {                            // more than a page: by name
      const one = await ask({ filter: n });
      return [n, !!one?.entries?.some((e) => e.n === n && isFile(e))];
    }));
  }
  function next() {
    while (busy < AT_ONCE) {
      const job = queue.pop();
      if (!job) return;
      const mine = gen;
      busy++;
      list(job).catch(() => [...job.names].map((n) => [n, false])).then((answers) => {
        busy--;
        if (mine === gen) {                                          // not another pane's meanwhile
          for (const [n, ok] of answers) remember(job.host, job.dir, n, ok);
          if (answers.some(([, ok]) => ok)) say();
        }
        next();
      });
    }
  }
  function enqueue(place) {
    const k = keyOf(place.host, place.path);
    found.set(k, { ...(found.get(k) ?? { ok: false, tries: 0 }), pending: true });           // asked: not again meanwhile
    let job = queue.find((j) => j.dir === place.dir && j.host === place.host);
    if (job) queue.splice(queue.indexOf(job), 1);
    else job = { host: place.host, dir: place.dir, names: new Set() };
    job.names.add(place.name);
    queue.push(job);                                                 // the newest first
    if (queue.length > QUEUED) {                                     // the oldest dropped: asked anew when marked again
      const old = queue.shift();
      for (const n of old.names) found.delete(keyOf(old.host, old.dir === "/" ? `/${n}` : `${old.dir}/${n}`));
    }
    soon ||= setTimeout(() => { soon = 0; next(); }, 0);             // once the marking pass named all it sees
  }

  /** "yes": its file is there; "no": not, or not known yet; or {error}: the pane's folder is not
   *  known (a click says why). Asked in the background when not known (listeners hear when one
   *  turns out to be there). */
  function state(value) {
    refresh();
    if (!pane) return "no";
    if (pane.error) return { error: pane.error };
    const places = placesOf(value);
    if (!places.length) return { error: `Cannot tell where ${value} is: this session's folder is not known.` };
    for (const place of places) {
      const k = known(place);
      const stale = !k || (!k.pending && performance.now() - k.at > (k.ok ? KEEP_FOUND : AGAIN[Math.min(AGAIN.length, k.tries) - 1]));
      if (stale) enqueue(place);
    }
    return foundAt(value) ? "yes" : "no";
  }
  return {
    state,
    /** Whether the file a printed path names is there, as far as known now. */
    there: (value) => state(value) === "yes",
    /** The absolute path of the file a printed path names, where it is there; null when none is known. */
    pathOf: (value) => foundAt(value)?.path ?? null,
    /** Another pane is shown: its folder and files are asked again. */
    forget() { gen++; pane = null; paneAt = -Infinity; asking = null; found = new Map(); queue.length = 0; },
    /** fn() when files turned out to be there or the pane's folder changed (at most every 250 ms). */
    listen(fn) { listeners.add(fn); },
  };
}
