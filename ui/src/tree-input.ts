// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// Pointer and keyboard input of the virtual tree: click, context menu, selection (single,
// toggle, paged ⇧-range) and the cursor keys. It acts on the Tree's state through the
// members the Tree marks as shared with it.

import { join } from "./api";
import { PAGE, rowAt, rowsOf, type RowsHost } from "./tree-rows";
import { selectRange } from "./tree-select";
import type { Tree } from "./tree";

export class TreeInput {
  constructor(private t: Tree) {}

  rowEl(e: Event) { return (e.target as HTMLElement).closest<HTMLElement>(".row[data-p]"); }

  select(p: string, i: number, mode: "one" | "toggle" | "range" = "one") {
    this.t.selectionRun++;
    if (mode === "range" && this.t.anchor >= 0) {
      void this.rangeTo(i);
      return;
    } else if (mode === "toggle") {
      if (!this.t.selected.delete(p)) this.t.selected.add(p);
      this.t.anchor = i;
    } else {
      this.t.selected = new Set([p]);
      this.t.anchor = i;
    }
    this.t.cursor = i;
    this.t.cb.changed();
    this.t.queue();
  }

  async rangeTo(i: number, range = true) {
    if (!this.t.root) return;
    const root = this.t.root, epoch = this.t.epoch, run = ++this.t.selectionRun;
    const alive = () => epoch === this.t.epoch && run === this.t.selectionRun;
    const rows = Object.create(this.t.rows) as RowsHost;
    rows.load = () => {}; // selection controls page concurrency; rendering still loads its viewport
    const [a, b] = range ? (this.t.anchor < i ? [this.t.anchor, i] : [i, this.t.anchor]) : [i, i];
    if (!range) this.t.anchor = i;
    this.t.cursor = i;
    this.t.selected.clear();
    const selected = new Set<string>();
    const done = await selectRange(a, b, async (index) => {
      let d = rowAt(root, index, 0, rows);
      if (d.kind !== "entry") return null;
      const page = Math.floor(d.idx / PAGE);
      while (d.dir.pending.has(page) && alive()) await new Promise((r) => setTimeout(r, 16));
      if (!alive()) return null;
      if (!d.row) await this.t.load(d.dir, page);
      if (!alive()) return null;
      d = rowAt(root, index, 0, rows);
      return d.kind === "entry" && d.row ? join(d.dir.path, d.row.n) : null;
    }, alive, (path) => selected.add(path));
    if (done) { this.t.selected = selected; this.t.cb.changed(); this.t.queue(); }
  }

  onClick(e: MouseEvent) {
    if ((e.target as HTMLElement).closest(".inline-edit")) return;
    const row = this.rowEl(e);
    if (!row) return;
    const p = row.dataset.p!, i = Number(row.dataset.i), isDir = row.dataset.d === "1";
    if (e.shiftKey) return this.select(p, i, "range");
    if (e.altKey && isDir && (e.target as HTMLElement).closest(".chev")) { // macOS: ⌥-click the disclosure arrow
      this.select(p, i);
      return this.t.expanded.has(p) ? this.t.collapse(p) : this.t.expandAllFrom(p);
    }
    if (e.metaKey && !isDir) { this.select(p, i); return this.t.cb.openWindow(p); }
    if (e.metaKey || e.altKey) return this.select(p, i, "toggle");
    this.select(p, i);
    if (isDir) this.t.toggle(p); else this.t.cb.open(p);
  }

  onContext(e: MouseEvent) {
    e.preventDefault();
    const row = this.rowEl(e);
    if (!row) return this.t.cb.context(null, true, e);
    const p = row.dataset.p!;
    if (!this.t.selected.has(p)) this.select(p, Number(row.dataset.i));
    this.t.cb.context(p, row.dataset.d === "1", e);
  }

  onKey(e: KeyboardEvent) {
    if (!this.t.root || e.metaKey || e.ctrlKey) return;
    if (e.altKey) {                                       // macOS: ⌥→ / ⌥← on a folder, recursively
      const cur = this.t.descAt(this.t.cursor);
      if (!cur?.isDir || (e.key !== "ArrowRight" && e.key !== "ArrowLeft")) return;
      e.preventDefault();
      return e.key === "ArrowRight" ? this.t.expandAllFrom(cur.p) : this.t.collapse(cur.p);
    }
    const total = rowsOf(this.t.root, this.t.rows);
    const move = (to: number) => {
      to = Math.max(0, Math.min(total - 1, to));
      void this.rangeTo(to, e.shiftKey && this.t.anchor >= 0);
      this.t.cursor = to;
      this.t.scrollIntoView(to);
      this.t.queue();
    };
    const page = Math.max(1, Math.floor(this.t.el.clientHeight / this.t.rowH) - 1);
    const cur = this.t.descAt(this.t.cursor);
    switch (e.key) {
      case "ArrowDown": move(this.t.cursor + 1); break;
      case "ArrowUp": move(this.t.cursor < 0 ? 0 : this.t.cursor - 1); break;
      case "PageDown": move(this.t.cursor + page); break;
      case "PageUp": move(this.t.cursor - page); break;
      case "Home": move(0); break;
      case "End": move(total - 1); break;
      case "ArrowRight":
        if (cur?.isDir && !this.t.expanded.has(cur.p)) void this.t.expand(cur.p);
        else if (cur?.isDir) move(this.t.cursor + 1);
        break;
      case "ArrowLeft":
        if (cur?.isDir && this.t.expanded.has(cur.p)) this.t.collapse(cur.p);
        else if (cur && cur.d.dir !== this.t.root) {
          const parent = cur.d.dir.path;
          void this.t.reveal(parent).then(() => { this.t.selected = new Set([parent]); this.t.cb.changed(); this.t.queue(); });
        }
        break;
      case "Enter": if (cur) cur.isDir ? this.t.toggle(cur.p) : this.t.cb.open(cur.p); break;
      case "F2": if (cur) this.t.startRename(cur.p); break;
      default: return;
    }
    e.preventDefault();
  }
}
