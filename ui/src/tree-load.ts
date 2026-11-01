// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// Paging of the virtual tree from the backend: reading one page of a folder (polling while
// the backend still reads a huge one), finding an entry's index, and re-finding the expanded
// children of a folder that was re-read. Stale answers (a re-root, a forced refresh) are dropped.

import { api, join, type Page } from "./api";
import { PAGE, type DirNode } from "./tree-rows";

/** What paging reads from, and tells, the tree. */
export interface PagerHost {
  readonly epoch: number;                               // bumps on re-root
  query(extra: Record<string, string | number | boolean | undefined>): Record<string, string | number | boolean | undefined>;
  collapse(path: string): void;                         // a folder is gone (not the user: expand all goes on)
  touch(): void;                                        // structure changed: render again
}

export class Pager {
  constructor(private host: PagerHost) {}

  async load(n: DirNode, page: number, force = false): Promise<void> {
    if (n.pending.has(page) && !force) return;
    if (force) { n.read = (n.read ?? 0) + 1; n.pending.clear(); }
    n.pending.add(page);
    const epoch = this.host.epoch, read = n.read;
    try {
      let r: Page;
      for (;;) {
        r = await api<Page>("GET", "/api/ls", { query: this.host.query({ path: n.path, offset: page * PAGE }) });
        if (r.status !== "loading") break;           // backend still reading a huge folder
        await new Promise((res) => setTimeout(res, 300));
        if (epoch !== this.host.epoch || read !== n.read) return;
      }
      if (epoch !== this.host.epoch || read !== n.read) return;
      const reread = n.loaded && r.gen !== n.gen;
      if (r.gen !== n.gen) n.pages.clear();          // folder was re-read: old pages are stale
      Object.assign(n, { loaded: true, error: r.status === "error" ? r.error : undefined, total: r.total, gen: r.gen, writable: r.writable });
      if (r.status !== "error") n.pages.set(page, r.entries);
      if (reread && n.children.size) void this.relocate(n); // entries may have shifted
    } catch (e: any) {
      if (epoch === this.host.epoch && read === n.read) { n.error = e.message; n.loaded = true; }
    } finally {
      if (read === n.read) n.pending.delete(page);
      this.host.touch();
    }
  }

  /** Re-find expanded children after their folder was re-read; drop the ones that are gone. */
  private async relocate(n: DirNode) {
    const epoch = this.host.epoch;
    for (const [name, c] of [...n.children]) {
      const idx = await this.locate(n, name).catch(() => null);
      if (epoch !== this.host.epoch) return;
      if (idx == null || idx >= n.total) this.host.collapse(join(n.path, name));
      else c.idx = idx;
    }
    n.sorted = undefined;
    this.host.touch();
  }

  /** Index of `name` inside folder `n`, asking the backend if the page is not loaded. */
  async locate(n: DirNode, name: string): Promise<number | null> {
    for (const [p, rows] of n.pages) {
      const i = rows.findIndex((r) => r.n === name);
      if (i >= 0) return p * PAGE + i;
    }
    // a failed lookup (an unreachable host, AC-37) counts as not found: the folder's error row says why
    const r = await api<Page>("GET", "/api/ls", { query: this.host.query({ path: n.path, offset: 0, limit: 1, locate: name }) }).catch(() => null);
    return r?.located ?? null;
  }
}
