// The Git log list's pure parts: the filter state and what the backend is sent, the query-id
// gate, the paging state machine, the keyboard selection model, the row badge and the windowing.
// gitlog.rs `PageView`/`RowView` are the wire shapes.
import { relTime } from "../lib/time";

/** gitlog.rs `RowView`. `time` is the committer time in epoch seconds. */
export type RowView = {
  repo: string;
  sha: string;
  time: number;
  author: string;
  email: string;
  subject: string;
  parents: string[];
  source: string;
};
export type Failure = { repo: string; reason: string };
export type BranchUse = { branch: string; found: number; fallback: number };
export type PageView = { queryId: number; rows: RowView[]; failures: Failure[]; branchUse: BranchUse | null; done: boolean };

/** gitlog.rs `FilterView`, as the backend takes it. */
export type FilterView = {
  search?: string;
  authors?: string[];
  mine?: boolean;
  path?: string;
  branch?: string;
  since?: number;
  until?: number;
  within?: number;
};

// ---- filter state ------------------------------------------------------------------------------

export type Range = "24h" | "7d" | "30d" | "all" | "custom";
export const RANGES: { id: Range; label: string }[] = [
  { id: "24h", label: "24h" },
  { id: "7d", label: "7d" },
  { id: "30d", label: "30d" },
  { id: "all", label: "All" },
  { id: "custom", label: "Custom" },
];
const WITHIN: Record<string, number> = { "24h": 86_400, "7d": 7 * 86_400, "30d": 30 * 86_400 };

/** What the filter bar edits, and what `gitLog.lastFilter` stores (the backend keeps it opaque). */
export type FilterState = {
  search: string;
  authors: string[];
  /** The "Me" chip: the repository's own `user.email`. */
  mine: boolean;
  path: string;
  branch: string;
  range: Range;
  /** `YYYY-MM-DD`, for the custom range. */
  from: string;
  to: string;
};

export const DEFAULT_FILTER: FilterState = { search: "", authors: [], mine: false, path: "", branch: "", range: "24h", from: "", to: "" };

/** The stored filter, repaired field by field; anything unusable falls back to its default. */
export function parseFilter(stored: unknown): FilterState {
  const o = stored && typeof stored === "object" ? (stored as Record<string, unknown>) : {};
  const str = (k: string) => (typeof o[k] === "string" ? (o[k] as string) : "");
  const range = RANGES.some((r) => r.id === o.range) ? (o.range as Range) : DEFAULT_FILTER.range;
  const authors = Array.isArray(o.authors) ? o.authors.filter((a): a is string => typeof a === "string" && a.trim() !== "") : [];
  const day = (s: string) => (/^\d{4}-\d{2}-\d{2}$/.test(s) ? s : "");
  return {
    search: str("search"),
    authors,
    mine: o.mine === true,
    path: str("path"),
    branch: str("branch"),
    range,
    from: day(str("from")),
    to: day(str("to")),
  };
}

/** Local midnight at the start of `day` (`YYYY-MM-DD`), epoch seconds; null when it is not a date. */
export function dayStart(day: string): number | null {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(day);
  if (!m) return null;
  const t = new Date(+m[1], +m[2] - 1, +m[3]).getTime();
  return Number.isNaN(t) ? null : Math.floor(t / 1000);
}

/** The filter as the backend takes it. A blank field is left out; the custom range is whole local days. */
export function toFilterView(f: FilterState): FilterView {
  const v: FilterView = {};
  if (f.search.trim()) v.search = f.search.trim();
  if (f.authors.length) v.authors = f.authors.slice();
  if (f.mine) v.mine = true;
  if (f.path.trim()) v.path = f.path.trim();
  if (f.branch.trim()) v.branch = f.branch.trim();
  if (f.range === "custom") {
    const since = dayStart(f.from);
    const to = dayStart(f.to);
    if (since !== null) v.since = since;
    if (to !== null) v.until = to + 86_400 - 1; // through the end of that day
  } else if (f.range in WITHIN) {
    v.within = WITHIN[f.range];
  }
  return v;
}

/** Whether anything narrows the list beyond the default 24h window. */
export function isFiltered(f: FilterState): boolean {
  return !!(f.search.trim() || f.authors.length || f.mine || f.path.trim() || f.branch.trim() || f.range !== DEFAULT_FILTER.range);
}

/** Adds an author chip: trimmed, no duplicates (case-insensitive), never empty. */
export function addAuthor(list: string[], raw: string): string[] {
  const a = raw.trim();
  if (!a || list.some((x) => x.toLowerCase() === a.toLowerCase())) return list;
  return [...list, a];
}

// ---- debounce ----------------------------------------------------------------------------------

/** How long a filter edit waits before it queries: typing is one query, not one per key. */
export const DEBOUNCE_MS = 250;

export type Debounced = { (): void; cancel(): void; flush(): void };

/** Calls `fn` `ms` after the last call; each call restarts the wait. */
export function debounce(fn: () => void, ms: number = DEBOUNCE_MS): Debounced {
  let t: ReturnType<typeof setTimeout> | null = null;
  const run = () => {
    t = null;
    fn();
  };
  const d = (() => {
    if (t) clearTimeout(t);
    t = setTimeout(run, ms);
  }) as Debounced;
  d.cancel = () => {
    if (t) clearTimeout(t);
    t = null;
  };
  d.flush = () => {
    if (!t) return;
    clearTimeout(t);
    run();
  };
  return d;
}

// ---- query ids and the paging state machine ----------------------------------------------------

/**
 * Query ids for this webview. The backend refuses an id older than the newest it has seen from the
 * window, so they are seeded from the clock (a reload never restarts at 0) and always rise.
 */
export class QueryGate {
  private last: number;
  constructor(seed: number = Date.now()) {
    this.last = seed - 1;
  }
  begin(now: number = Date.now()): number {
    this.last = Math.max(this.last + 1, now);
    return this.last;
  }
  get current(): number {
    return this.last;
  }
  isCurrent(id: number): boolean {
    return id === this.last;
  }
}

/** The one gate of this webview, shared by every list instance. */
export const queryGate = new QueryGate();

export const STALE = "stale query";
export const isStale = (e: unknown) => String(e).includes(STALE);

export type QueryState = {
  rows: RowView[];
  failures: Failure[];
  branchUse: BranchUse | null;
  done: boolean;
  /** A first page is being fetched; the previous rows stay until it lands. */
  loading: boolean;
  loadingMore: boolean;
  /** The first page's failure, or Load more's; null when fine. */
  error: string | null;
};

export const EMPTY_QUERY: QueryState = { rows: [], failures: [], branchUse: null, done: false, loading: false, loadingMore: false, error: null };

export type Request = { repos: string[]; filter: FilterView };
export type PageFetcher = (id: number, req: Request, more: boolean) => Promise<PageView>;

export const rowKey = (r: Pick<RowView, "repo" | "sha">) => `${r.repo}\u0000${r.sha}`;

/**
 * One query and its pages. A new `start` supersedes the one before it, whatever is still in flight;
 * Load more holds one request at a time. Every response is checked against the id it was asked for.
 */
export class LogQuery {
  private id = 0;
  private req: Request = { repos: [], filter: {} };
  private moreBusy = false;
  state: QueryState = EMPTY_QUERY;

  constructor(
    private readonly fetch: PageFetcher,
    private readonly emit: (s: QueryState) => void,
    private readonly gate: QueryGate = queryGate,
  ) {}

  private set(patch: Partial<QueryState>) {
    this.state = { ...this.state, ...patch };
    this.emit(this.state);
  }

  async start(req: Request): Promise<void> {
    const id = this.gate.begin();
    this.id = id;
    this.req = req;
    this.moreBusy = false;
    this.set({ loading: true, loadingMore: false, error: null });
    try {
      const page = await this.fetch(id, req, false);
      if (id !== this.id) return;
      this.state = { ...EMPTY_QUERY, rows: dedupe([], page.rows), failures: page.failures, branchUse: page.branchUse, done: page.done };
      this.emit(this.state);
    } catch (e) {
      if (id !== this.id || isStale(e)) return;
      this.set({ rows: [], failures: [], branchUse: null, done: true, loading: false, error: String(e) });
    }
  }

  /** Appends the next page. Ignored while the first page or another Load more is pending, or when done. */
  async more(): Promise<void> {
    if (this.id === 0 || this.state.done || this.state.loading || this.moreBusy) return;
    const id = this.id;
    this.moreBusy = true;
    this.set({ loadingMore: true, error: null });
    try {
      const page = await this.fetch(id, this.req, true);
      if (id !== this.id) return;
      this.set({ rows: dedupe(this.state.rows, page.rows), failures: page.failures, branchUse: page.branchUse, done: page.done, loadingMore: false });
    } catch (e) {
      if (id !== this.id || isStale(e)) return;
      this.set({ loadingMore: false, error: String(e) });
    } finally {
      if (id === this.id) this.moreBusy = false;
    }
  }

  /** Drops whatever is in flight: its answer will be ignored. */
  cancel() {
    this.id = 0;
    this.moreBusy = false;
  }
}

/** `base` followed by the rows of `next` it does not already hold. */
export function dedupe(base: RowView[], next: RowView[]): RowView[] {
  const seen = new Set(base.map(rowKey));
  const out = base.slice();
  for (const r of next) {
    const k = rowKey(r);
    if (!seen.has(k)) {
      seen.add(k);
      out.push(r);
    }
  }
  return out;
}

// ---- row text ----------------------------------------------------------------------------------

/** "5m ago", "3d ago", a date past a week; empty for a bad time. `time` is epoch seconds. */
export function commitAge(time: number, now: number = Date.now()): string {
  if (!Number.isFinite(time)) return "";
  return relTime(new Date(time * 1000).toISOString(), now);
}

/** The full date and time, for a tooltip. */
export function commitDate(time: number): string {
  return Number.isFinite(time) ? new Date(time * 1000).toLocaleString() : "";
}

export const shortSha = (sha: string) => sha.slice(0, 7);

const SHA = /^[0-9a-f]{40}$/i;
const stripHeads = (s: string) => (s.startsWith("refs/heads/") ? s.slice("refs/heads/".length) : s);

/**
 * The badge on a row: where the commit lives when that is not the main checkout's HEAD.
 * `refs/heads/x` reads `x`, a detached worktree's sha reads as 7 characters, and nothing shows
 * for `HEAD`, nor for a source that is the branch the filter already names.
 */
export function badgeLabel(source: string, branchFilter: string): string | null {
  const s = source.trim();
  if (!s || s === "HEAD") return null;
  const label = SHA.test(s) ? shortSha(s) : stripHeads(s);
  const f = stripHeads(branchFilter.trim());
  if (f && (s === branchFilter.trim() || label === f)) return null;
  return label;
}

/** The status line under the list: how many are loaded, and how the branch filter resolved. */
export function footerText(count: number, done: boolean, branchUse: BranchUse | null, loading = false): string {
  const parts: string[] = [];
  if (loading && count === 0) parts.push("Loading…");
  else parts.push(`${count}${done ? "" : "+"} ${count === 1 && done ? "commit" : "commits"}`);
  if (branchUse) {
    parts.push(`${branchUse.branch} found in ${branchUse.found}`);
    if (branchUse.fallback > 0) parts.push(`current branch in ${branchUse.fallback}`);
  }
  return parts.join(" · ");
}

// ---- keyboard selection ------------------------------------------------------------------------

export type NavKey = "ArrowUp" | "ArrowDown" | "Home" | "End" | "PageUp" | "PageDown";
export const NAV_KEYS: readonly string[] = ["ArrowUp", "ArrowDown", "Home", "End", "PageUp", "PageDown"];

/**
 * The index a navigation key moves to. With nothing selected, Down and Page Down start at the top
 * and Up and Page Up at the top too (there is no row "before" none); the result stays in range.
 */
export function navigate(count: number, current: number, key: NavKey, page: number): number {
  if (count <= 0) return -1;
  const last = count - 1;
  const step = Math.max(1, page);
  switch (key) {
    case "Home":
      return 0;
    case "End":
      return last;
    case "ArrowDown":
      return current < 0 ? 0 : Math.min(last, current + 1);
    case "ArrowUp":
      return current < 0 ? 0 : Math.max(0, current - 1);
    case "PageDown":
      return current < 0 ? Math.min(last, step - 1) : Math.min(last, current + step);
    case "PageUp":
      return current < 0 ? 0 : Math.max(0, current - step);
  }
}

/** The index of `key` in `rows`, or -1. */
export function indexOfKey(rows: RowView[], key: string | null): number {
  return key === null ? -1 : rows.findIndex((r) => rowKey(r) === key);
}

/** The key to keep selected after the rows changed: the same commit while it is still listed. */
export function keepSelection(rows: RowView[], key: string | null): string | null {
  return indexOfKey(rows, key) >= 0 ? key : null;
}

// ---- windowing ---------------------------------------------------------------------------------

export const ROW_H = 30;

/** The rows to draw for a scroll position: the visible ones plus `overscan` either side. */
export function visibleRange(scrollTop: number, viewH: number, rowH: number, count: number, overscan = 8): { start: number; end: number } {
  if (count <= 0 || rowH <= 0) return { start: 0, end: 0 };
  const first = Math.floor(Math.max(0, scrollTop) / rowH);
  const last = Math.ceil((Math.max(0, scrollTop) + Math.max(0, viewH)) / rowH);
  return { start: Math.max(0, first - overscan), end: Math.min(count, last + overscan) };
}

/** The scrollTop that brings row `index` fully into view with the least movement. */
export function scrollToReveal(index: number, scrollTop: number, viewH: number, rowH: number): number {
  const top = index * rowH;
  if (top < scrollTop) return top;
  if (top + rowH > scrollTop + viewH) return top + rowH - viewH;
  return scrollTop;
}

/** Whether the window reaches close enough to the end that the next page should be fetched. */
export function nearEnd(end: number, count: number, margin = 20): boolean {
  return count > 0 && end >= count - margin;
}
