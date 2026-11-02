// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// Virtual file tree. Folders are paged from the backend (500 rows per page), so a
// folder with 500,000 entries costs one scrollbar and ~40 DOM rows. Expanded folders
// are spliced into their parent by index (row arithmetic in tree-rows.ts, row HTML in
// tree-render.ts, "expand all" in tree-expand.ts, paging in tree-load.ts, clicks and keys
// in tree-input.ts, which use the members not marked private here).

import { basename, dirname, isDirKind, isUnder, join, rebase, type Sort } from "./api";
import { rowHtml, type RowView } from "./tree-render";
import { PAGE, indexOf, rowAt, rowsOf, type DirNode, type RowDesc, type RowsHost } from "./tree-rows";
import { expandAll, summary } from "./tree-expand";
import { InlineEdit, type EditMode } from "./tree-edit";
import { Pager } from "./tree-load";
import { TreeInput } from "./tree-input";

export type { EditMode } from "./tree-edit";

const OVERSCAN = 12;

export interface TreeCallbacks {
  open(path: string): void;                              // file activated
  openWindow(path: string): void;                        // ⌘-click: file in the viewer window
  changed(): void;                                       // expanded/selected changed → persist
  context(path: string | null, isDir: boolean, ev: MouseEvent): void;
  commit(mode: EditMode, dir: string, name: string, path?: string): Promise<string | null>;
  notify(message: string): void;                         // e.g. what "expand all" did
}

/** The members without `private` that are not the Panel's API (cursor, anchor, epoch,
 *  selectionRun, rows, pager, el, cb, load, expand, collapse, descAt, scrollIntoView,
 *  expandAllFrom) are @internal: shared with TreeInput (tree-input.ts) and Pager (tree-load.ts)
 *  only. Moving the selection state into TreeInput would close them again (Noticed, not fixed). */
export class Tree {
  root: DirNode | null = null;
  private nodes = new Map<string, DirNode>();
  expanded = new Set<string>();
  selected = new Set<string>();
  showHidden = true;
  sort: Sort = "name";
  cursor = -1;               // row index of the keyboard cursor
  anchor = -1;               // row index where a ⇧-range starts
  private filter = "";
  rowH = 22;
  private version = 0;               // bumps on every structural change; invalidates memos
  private renderQueued = false;
  epoch = 0;                 // bumps on re-root; stale async results are dropped
  private expandRun = 0;             // bumps on expand all and any collapse; cancels a running one
  selectionRun = 0;          // cancels a paged range on another interaction
  private spacer = document.createElement("div");
  private layer = document.createElement("div");
  private edit: InlineEdit;
  pager: Pager;
  /** The tree's state as the row arithmetic reads it. */
  rows: RowsHost = ((t: Tree) => ({
    get version() { return t.version; },
    get filter() { return t.filter; },
    get root() { return t.root; },
    nodes: t.nodes,
    creatingIn: (p: string) => t.edit.creatingIn(p),
    load: (n: DirNode, page: number) => void t.load(n, page),
  }))(this);

  constructor(readonly el: HTMLElement, readonly cb: TreeCallbacks) {
    this.spacer.className = "spacer";
    this.layer.className = "rows";
    const t = this;
    this.pager = new Pager({
      get epoch() { return t.epoch; },
      query: (extra) => this.query(extra),
      collapse: (p) => this.collapse(p, false, false),
      touch: () => this.touch(),
    });
    this.edit = new InlineEdit({ commit: cb.commit, rerender: () => this.touch(), done: () => el.focus() });
    el.append(this.spacer, this.layer, this.edit.el);
    el.addEventListener("scroll", () => this.queue());
    new ResizeObserver(() => this.themeChanged()).observe(el);
    const input = new TreeInput(this);
    el.addEventListener("click", (e) => input.onClick(e));
    el.addEventListener("contextmenu", (e) => input.onContext(e));
    el.addEventListener("keydown", (e) => input.onKey(e));
  }

  // ── public API ──────────────────────────────────────────────────────────────

  /** Show `root`, restoring `expanded`, `selected` and the scroll position. */
  async setRoot(root: string, expanded: string[], selected: string[], scroll = 0) {
    this.epoch++;
    this.edit.cancel();
    this.nodes.clear();
    this.expanded = new Set();
    this.selected = new Set(selected);
    this.cursor = this.anchor = -1;
    this.el.scrollTop = 0;
    this.root = this.node(root);
    const epoch = this.epoch;
    await this.load(this.root, 0);
    // restore expanded folders level by level: each is located in its (already open) parent
    const levels = new Map<number, string[]>();
    for (const p of expanded) {
      if (!isUnder(p, root) || p === root) continue;                    // also for root "/"
      const depth = p.split("/").length;
      levels.set(depth, [...(levels.get(depth) ?? []), p]);
    }
    for (const depth of [...levels.keys()].sort((a, b) => a - b)) {
      if (epoch !== this.epoch) return;
      await Promise.all(levels.get(depth)!.map((p) => this.expand(p, false)));
    }
    if (epoch !== this.epoch) return;
    this.render();
    if (scroll > 0) this.el.scrollTop = scroll;
    else if (selected[0]) await this.reveal(selected[0]);
    this.queue();
  }

  setFilter(filter: string) { this.filter = filter.trim(); this.reload(); }

  setShowHidden(show: boolean) {
    this.showHidden = show;
    if (!show) for (const p of this.selected) if (this.isHiddenPath(p)) this.selected.delete(p);
    this.reload();
  }

  setSort(sort: Sort) { this.sort = sort; this.reload(); }

  /** Re-read everything (keeps expanded, selection and scroll). */
  reload() {
    if (this.root) void this.setRoot(this.root.path, [...this.expanded], [...this.selected], this.el.scrollTop);
  }

  /** Folders changed on disk or through a panel: re-read the ones we show. The next
   *  response replaces their pages (gen -1) and re-locates expanded children. */
  async refreshDirs(dirs: string[]) {
    this.selectionRun++;
    await Promise.all(dirs.map((d) => this.nodes.get(d)).filter((n) => n?.loaded).map((n) => {
      n!.gen = -1;
      return this.load(n!, 0, true);
    }));
  }

  /** After a rename, carry expanded folders, selection and cached nodes to the new path. */
  renamed(from: string, to: string) {
    this.selectionRun++;
    this.expanded = new Set([...this.expanded].map((p) => rebase(p, from, to)));
    this.selected = new Set([...this.selected].map((p) => rebase(p, from, to)));
    const parent = this.nodes.get(dirname(from));
    const c = parent?.children.get(basename(from));
    if (parent && c) {
      parent.children.delete(basename(from));
      for (const k of [...this.nodes.keys()]) if (isUnder(k, from)) this.nodes.delete(k); // reloads lazily
      const node = this.node(to);
      parent.children.set(basename(to), { idx: c.idx, node });
      void this.load(node, 0);
    }
    this.touch();
  }

  /** Expand every folder below `start` (the root by default), bounded (AC-31). Returns
   *  what was done, or null when cancelled by a re-root, a collapse or another expand. */
  async expandAll(start = this.root?.path): Promise<string | null> {
    if (!this.root || !start) return null;
    const run = ++this.expandRun, epoch = this.epoch, root = this.root.path;
    const alive = () => run === this.expandRun && epoch === this.epoch;
    const wasOpen = new Set(this.expanded);           // the user's open folders stay open
    const r = await expandAll(start, root, {
      alive,
      expand: async (p) => {
        if (p !== root && !this.expanded.has(p)) await this.expand(p, false);
        const n = this.nodes.get(p);
        return alive() && n?.loaded && !n.error && (p === root || this.expanded.has(p)) ? { total: n.total, rows: n.pages.get(0) ?? [] } : null;
      },
      collapse: (p) => { if (!wasOpen.has(p)) this.collapse(p, false, false); },
    });
    if (r.cancelled) return null;
    this.cb.changed();
    return summary(r);
  }

  expandAllFrom(p: string) { void this.expandAll(p).then((m) => { if (m) this.cb.notify(m); }); }

  collapseAll() {
    this.selectionRun++;
    this.expandRun++;
    this.expanded.clear();
    // every cached folder forgets its open children, or re-opening one shows them again
    for (const n of this.nodes.values()) { n.children.clear(); n.sorted = undefined; }
    if (this.edit.edit) this.edit.cancel();
    this.cb.changed();
    this.touch();
  }

  focus() { this.el.focus(); }
  get scroll() { return this.el.scrollTop; }
  get cursorPath(): string | null { return this.descAt(this.cursor)?.p ?? [...this.selected][0] ?? null; }
  /** May the panel write in folder `dir` (as last reported by the backend). */
  isWritable(dir: string) { return this.nodes.get(dir)?.writable ?? false; }
  /** Folder `dir` was read and is not writable (a folder not read yet: terminumd decides). */
  readOnly(dir: string) { const n = this.nodes.get(dir); return !!n?.loaded && !n.writable; }

  /** The folder files dragged over the tree would upload to (AC-95), its rows marked; null: none. */
  dropAt: string | null = null;
  setDrop(dir: string | null) { if (dir !== this.dropAt) { this.dropAt = dir; this.touch(); } }

  /** Row height follows the theme's font size; read it only when layout may have changed. */
  themeChanged() {
    this.rowH = parseFloat(getComputedStyle(this.el).getPropertyValue("--row")) || 22;
    this.queue();
  }

  /** Target folder for "new file": the selected folder, or the folder of the selected file. */
  targetDir(): string | null {
    if (!this.root) return null;
    const p = this.cursorPath;
    if (!p) return this.root.path;
    const d = this.descAt(this.cursor);
    return (d ? d.isDir : this.nodes.has(p)) ? p : dirname(p);
  }

  async startCreate(dir: string, mode: "file" | "folder") {
    if (!this.root) return;
    if (dir !== this.root.path && !this.expanded.has(dir)) await this.expand(dir);
    // a folder that did not open here (a selection kept from another folder, or under a closed one) has no
    // row for the name's field: it goes in the root, never nowhere
    if (!this.root) return;
    if (dir !== this.root.path && !this.expanded.has(dir)) dir = this.root.path;
    this.edit.start({ mode, dir });
  }

  startRename(path: string) { this.edit.start({ mode: "rename", dir: dirname(path), path }); }

  // ── data ────────────────────────────────────────────────────────────────────

  private touch() { this.version++; this.queue(); }

  private node(path: string): DirNode {
    let n = this.nodes.get(path);
    if (!n) {
      n = { path, loaded: false, writable: false, total: 0, gen: 0, pages: new Map(), pending: new Set(), children: new Map() };
      this.nodes.set(path, n);
    }
    return n;
  }

  private query(extra: Record<string, string | number | boolean | undefined>) {
    return { hidden: this.showHidden, filter: this.filter || undefined, sort: this.sort === "name" ? undefined : this.sort, limit: PAGE, ...extra };
  }

  load(n: DirNode, page: number, force = false) { return this.pager.load(n, page, force); }
  private locate(n: DirNode, name: string) { return this.pager.locate(n, name); }

  async expand(path: string, persist = true) {
    this.selectionRun++;
    if (!this.root || !path.startsWith(this.root.path)) return;
    const parent = this.nodes.get(dirname(path));
    if (!parent || (!this.expanded.has(parent.path) && parent !== this.root)) return;
    const epoch = this.epoch, run = this.expandRun;
    const idx = await this.locate(parent, basename(path));
    if (idx == null || epoch !== this.epoch || run !== this.expandRun || (parent !== this.root && !this.expanded.has(parent.path))) return;
    const node = this.node(path);
    parent.children.set(basename(path), { idx, node });
    parent.sorted = undefined;
    this.expanded.add(path);
    this.touch();
    await this.load(node, 0);
    if (persist) this.cb.changed();
  }

  collapse(path: string, persist = true, cancelExpand = true) {
    this.selectionRun++;
    if (cancelExpand) this.expandRun++;
    for (const p of [...this.expanded]) {
      if (!isUnder(p, path)) continue;
      this.expanded.delete(p);
      const parent = this.nodes.get(dirname(p));
      if (parent) { parent.children.delete(basename(p)); parent.sorted = undefined; }
    }
    if (this.edit.edit && isUnder(this.edit.edit.dir, path)) this.edit.cancel();
    if (persist) this.cb.changed();
    this.touch();
  }

  toggle(path: string) {
    if (this.expanded.has(path)) this.collapse(path); else void this.expand(path);
  }

  /** The cursor onto `path`, scrolled into sight; `open`: its folders expand first (Reveal in tree).
   *  False when it is not under the root, not found or the tree moved meanwhile. */
  async reveal(path: string, open = false): Promise<boolean> {
    const epoch = this.epoch, root = this.root?.path;
    if (open && root && isUnder(path, root) && path !== root) {
      const chain: string[] = [];
      for (let d = dirname(path); d !== root && isUnder(d, root); d = dirname(d)) chain.unshift(d);
      for (const d of chain) if (!this.expanded.has(d) && epoch === this.epoch) await this.expand(d);
    }
    const parent = this.nodes.get(dirname(path));
    const idx = parent ? await this.locate(parent, basename(path)) : null;
    if (epoch !== this.epoch) return false;
    const at = parent && idx != null ? indexOf(parent, idx, this.rows) : -1;
    if (at < 0) return false;
    this.cursor = this.anchor = at;
    this.scrollIntoView(at);
    this.queue();
    return true;
  }

  scrollIntoView(i: number) {
    const top = i * this.rowH, h = this.el.clientHeight;
    if (top < this.el.scrollTop) this.el.scrollTop = top;
    else if (top + this.rowH > this.el.scrollTop + h) this.el.scrollTop = top + this.rowH - h;
  }

  // ── rendering ───────────────────────────────────────────────────────────────

  queue() {
    if (this.renderQueued) return;
    this.renderQueued = true;
    requestAnimationFrame(() => { this.renderQueued = false; this.render(); });
  }

  private render() {
    if (!this.root) { this.layer.innerHTML = ""; return; }
    const total = rowsOf(this.root, this.rows);
    this.spacer.style.height = `${total * this.rowH}px`;
    const first = Math.max(0, Math.floor(this.el.scrollTop / this.rowH) - OVERSCAN);
    const last = Math.min(total, Math.ceil((this.el.scrollTop + this.el.clientHeight) / this.rowH) + OVERSCAN);
    const html: string[] = [];
    let editRow = -1, editDepth = 0;
    for (let i = first; i < last; i++) {
      const d = rowAt(this.root, i, 0, this.rows);
      if (d.kind === "edit" || (d.kind === "entry" && d.row && this.edit.renaming(join(d.dir.path, d.row.n)))) {
        editRow = i; editDepth = d.depth;
      }
      html.push(this.rowHtml(d, i));
    }
    this.layer.style.transform = `translateY(${first * this.rowH}px)`;
    this.layer.innerHTML = html.join("");
    this.edit.place(editRow, editDepth, this.rowH);
  }

  private rowHtml(d: RowDesc, i: number): string {
    const view: RowView = d.kind === "entry" ? { kind: "entry", dir: d.dir.path, depth: d.depth, row: d.row }
      : d.kind === "note" ? d
      : { kind: "edit", depth: d.depth, folder: this.edit.edit?.mode === "folder", name: this.edit.value };
    return rowHtml(view, i, { expanded: this.expanded, selected: this.selected, cursor: this.cursor, filter: this.filter, renaming: (p) => this.edit.renaming(p), drop: this.dropAt });
  }

  private isHiddenPath(p: string) {
    return this.root ? p.slice(this.root.path.length).split("/").some((s) => s.startsWith(".")) : false;
  }

  descAt(i: number) {
    if (!this.root || i < 0) return null;
    const d = rowAt(this.root, i, 0, this.rows);
    return d.kind === "entry" && d.row ? { d, p: join(d.dir.path, d.row.n), isDir: isDirKind(d.row.k) } : null;
  }

}
