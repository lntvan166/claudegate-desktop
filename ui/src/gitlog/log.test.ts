import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  DEBOUNCE_MS,
  DEFAULT_FILTER,
  HISTORY_KEY,
  LogQuery,
  QueryGate,
  ROW_H,
  MAX_IDLE_PAGES,
  STALE,
  autoMoreAllowed,
  slashTarget,
  addAuthor,
  badgeLabel,
  commitAge,
  debounce,
  footerText,
  historyBanner,
  historyRequest,
  keepSelection,
  navigate,
  nearEnd,
  parseFilter,
  rowKey,
  scrollToReveal,
  toFilterView,
  visibleRange,
  type PageView,
  type QueryState,
  type RowView,
} from "./log";
import { pickFile, type ChangedFile } from "./changes";

const SHA = "a".repeat(40);
function row(n: number, extra: Partial<RowView> = {}): RowView {
  return { repo: "/w/acme-api/.git", sha: n.toString(16).padStart(40, "0"), time: 1_700_000_000 - n, author: "Rin", email: "rin@acme.test", subject: `commit ${n}`, parents: [], source: "HEAD", ...extra };
}
const rows = (from: number, to: number) => Array.from({ length: to - from }, (_, i) => row(from + i));
function page(queryId: number, r: RowView[], done = false): PageView {
  return { queryId, rows: r, failures: [], branchUse: null, done };
}
const deferred = <T,>() => {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((a, b) => ((resolve = a), (reject = b)));
  return { promise, resolve, reject };
};

describe("query ids", () => {
  it("are seeded from the clock and never go backwards", () => {
    const g = new QueryGate(1_000_000);
    const a = g.begin(500); // a clock that went back
    const b = g.begin(500);
    expect(a).toBeGreaterThanOrEqual(1_000_000);
    expect(b).toBe(a + 1);
    expect(g.isCurrent(a)).toBe(false);
    expect(g.isCurrent(b)).toBe(true);
  });

  it("ignore a response with a stale id", async () => {
    const first = deferred<PageView>();
    const second = deferred<PageView>();
    const calls = [first, second];
    let n = 0;
    const seen: QueryState[] = [];
    const q = new LogQuery(() => calls[n++].promise, (s) => seen.push(s), new QueryGate(10));
    const p1 = q.start({ repos: ["a"], filter: {}, history: null });
    const p2 = q.start({ repos: ["a"], filter: { search: "x" }, history: null });
    second.resolve(page(0, rows(0, 2), true));
    await p2;
    first.resolve(page(0, rows(100, 105)));
    await p1;
    expect(q.state.rows.map((r) => r.subject)).toEqual(["commit 0", "commit 1"]);
    expect(q.state.loading).toBe(false);
  });

  it("send rising ids, and the same id for Load more", async () => {
    const ids: [number, boolean][] = [];
    const q = new LogQuery(async (id, _r, more) => (ids.push([id, more]), page(id, rows(0, 1))), () => {}, new QueryGate(50));
    await q.start({ repos: [], filter: {}, history: null });
    await q.more();
    await q.start({ repos: [], filter: {}, history: null });
    expect(ids[0][0]).toBe(ids[1][0]);
    expect(ids[1][1]).toBe(true);
    expect(ids[2][0]).toBeGreaterThan(ids[0][0]);
  });

  it("a stale error for the current query is surfaced, not left loading", async () => {
    const q = new LogQuery(async () => Promise.reject(STALE), () => {}, new QueryGate(1));
    await q.start({ repos: [], filter: {}, history: null });
    expect(q.state.loading).toBe(false);
    expect(q.state.error).toContain(STALE);
  });

  it("a stale error for a superseded query changes nothing", async () => {
    const first = deferred<PageView>();
    let n = 0;
    const q = new LogQuery(() => (n++ === 0 ? first.promise : Promise.resolve(page(0, rows(0, 1), true))), () => {}, new QueryGate(1));
    const p1 = q.start({ repos: [], filter: {}, history: null });
    await q.start({ repos: [], filter: {}, history: null });
    first.reject(STALE);
    await p1;
    expect(q.state.error).toBeNull();
    expect(q.state.rows).toHaveLength(1);
  });

  it("clear() forgets the old rows for good", async () => {
    const q = new LogQuery(async (id) => page(id, rows(0, 3)), () => {}, new QueryGate(1));
    await q.start({ repos: [], filter: {}, history: null });
    q.clear();
    expect(q.state.rows).toEqual([]);
    expect(q.state.loading).toBe(false);
  });
});

describe("Load more", () => {
  it("keeps one in flight: a second press is ignored", async () => {
    const d = deferred<PageView>();
    let calls = 0;
    const q = new LogQuery(
      async (id, _r, more) => {
        calls++;
        return more ? d.promise : page(id, rows(0, 3));
      },
      () => {},
      new QueryGate(1),
    );
    await q.start({ repos: [], filter: {}, history: null });
    const a = q.more();
    const b = q.more();
    await b;
    expect(calls).toBe(2); // the first page and one more
    d.resolve(page(0, rows(3, 6)));
    await a;
    expect(q.state.rows).toHaveLength(6);
  });

  it("a new query supersedes a pending one", async () => {
    const d = deferred<PageView>();
    let phase = 0;
    const q = new LogQuery(
      async (id, _r, more) => {
        if (more) return d.promise;
        return page(id, phase++ === 0 ? rows(0, 3) : rows(50, 52));
      },
      () => {},
      new QueryGate(1),
    );
    await q.start({ repos: [], filter: {}, history: null });
    const pending = q.more();
    await q.start({ repos: [], filter: { search: "y" }, history: null });
    d.resolve(page(0, rows(3, 6)));
    await pending;
    expect(q.state.rows.map((r) => r.subject)).toEqual(["commit 50", "commit 51"]);
    // and Load more works again for the new query
    expect(q.state.loadingMore).toBe(false);
  });

  it("appends without losing the selection, and stops when done", async () => {
    let n = 0;
    const q = new LogQuery(async (id) => (n++ === 0 ? page(id, rows(0, 3)) : page(id, rows(3, 5), true)), () => {}, new QueryGate(1));
    await q.start({ repos: [], filter: {}, history: null });
    const selected = rowKey(q.state.rows[1]);
    await q.more();
    expect(q.state.rows).toHaveLength(5);
    expect(keepSelection(q.state.rows, selected)).toBe(selected);
    expect(q.state.done).toBe(true);
    await q.more();
    expect(n).toBe(2);
  });

  it("does not add a commit it already holds", async () => {
    let n = 0;
    const q = new LogQuery(async (id) => (n++ === 0 ? page(id, rows(0, 3)) : page(id, rows(2, 5))), () => {}, new QueryGate(1));
    await q.start({ repos: [], filter: {}, history: null });
    await q.more();
    expect(q.state.rows).toHaveLength(5);
  });
});

describe("auto Load more", () => {
  it("stops after a few pages with no scrolling", () => {
    expect(MAX_IDLE_PAGES).toBe(3);
    expect(autoMoreAllowed(0)).toBe(true);
    expect(autoMoreAllowed(MAX_IDLE_PAGES - 1)).toBe(true);
    expect(autoMoreAllowed(MAX_IDLE_PAGES)).toBe(false);
  });
});

describe("the / key", () => {
  const el = (tagName: string, o: { role?: string; list?: boolean } = {}) => ({
    tagName,
    closest: (sel: string) => (o.role === "listbox" && sel.includes("listbox") ? {} : null),
    hasAttribute: (n: string) => !!o.list && n === "data-loglist",
  });
  it("acts on the page and on the commit list", () => {
    expect(slashTarget(null)).toBe(true);
    expect(slashTarget(el("BUTTON"))).toBe(true);
    expect(slashTarget(el("DIV", { role: "listbox", list: true }))).toBe(true);
  });
  it("does not act while typing, or in another listbox", () => {
    expect(slashTarget(el("INPUT"))).toBe(false);
    expect(slashTarget(el("DIV", { role: "listbox" }))).toBe(false);
  });
});

describe("debounce", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("waits 250 ms after the last call", () => {
    expect(DEBOUNCE_MS).toBe(250);
    const fn = vi.fn();
    const d = debounce(fn);
    d();
    vi.advanceTimersByTime(200);
    d();
    vi.advanceTimersByTime(249);
    expect(fn).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(fn).toHaveBeenCalledTimes(1);
  });

  it("can be cancelled or flushed", () => {
    const fn = vi.fn();
    const d = debounce(fn);
    d();
    d.cancel();
    vi.advanceTimersByTime(1000);
    expect(fn).not.toHaveBeenCalled();
    d();
    d.flush();
    expect(fn).toHaveBeenCalledTimes(1);
  });
});

describe("commitAge", () => {
  const now = Date.parse("2026-11-26T12:00:00Z");
  const at = (iso: string) => Date.parse(iso) / 1000;
  it("reads like the review's ages", () => {
    expect(commitAge(at("2026-11-26T11:59:50Z"), now)).toBe("just now");
    expect(commitAge(at("2026-11-26T11:20:00Z"), now)).toBe("40m ago");
    expect(commitAge(at("2026-11-26T07:00:00Z"), now)).toBe("5h ago");
    expect(commitAge(at("2026-11-23T12:00:00Z"), now)).toBe("3d ago");
  });
  it("is empty for garbage", () => {
    expect(commitAge(NaN, now)).toBe("");
  });
});

describe("footerText", () => {
  it("counts the loaded commits and marks a partial list", () => {
    expect(footerText(1, true, null)).toBe("1 commit");
    expect(footerText(40, true, null)).toBe("40 commits");
    expect(footerText(40, false, null)).toBe("40+ commits");
    expect(footerText(0, false, null, true)).toBe("Loading…");
  });
  it("adds where the branch was found and where the current branch stood in", () => {
    expect(footerText(12, true, { branch: "release", found: 3, fallback: 2 })).toBe("12 commits · release found in 3 · current branch in 2");
    expect(footerText(12, true, { branch: "release", found: 5, fallback: 0 })).toBe("12 commits · release found in 5");
  });
});

describe("badgeLabel", () => {
  it("is empty for the main checkout's HEAD", () => {
    expect(badgeLabel("HEAD", "")).toBeNull();
  });
  it("strips refs/heads/ and shortens a sha to 7", () => {
    expect(badgeLabel("refs/heads/checkout-v2", "")).toBe("checkout-v2");
    expect(badgeLabel(SHA, "")).toBe("aaaaaaa");
    expect(badgeLabel("b".repeat(64), "")).toBe("bbbbbbb");
    expect(badgeLabel("b".repeat(50), "")).toBe("b".repeat(50));
  });
  it("is suppressed when the source is the active branch filter", () => {
    expect(badgeLabel("release", "release")).toBeNull();
    expect(badgeLabel("refs/heads/release", "release")).toBeNull();
    expect(badgeLabel("refs/heads/dark-mode", "release")).toBe("dark-mode");
  });
});

describe("filter", () => {
  it("sends the 24h default as a window the backend resolves", () => {
    expect(toFilterView(DEFAULT_FILTER)).toEqual({ within: 86_400 });
    expect(toFilterView({ ...DEFAULT_FILTER, range: "all" })).toEqual({});
    expect(toFilterView({ ...DEFAULT_FILTER, range: "30d" })).toEqual({ within: 30 * 86_400 });
  });
  it("sends a custom range as whole local days", () => {
    const v = toFilterView({ ...DEFAULT_FILTER, range: "custom", from: "2026-11-01", to: "2026-11-02" });
    expect(v.within).toBeUndefined();
    expect(v.until! - v.since!).toBe(2 * 86_400 - 1);
    expect(toFilterView({ ...DEFAULT_FILTER, range: "custom", from: "", to: "" })).toEqual({});
  });
  it("leaves blank fields out and trims the rest", () => {
    expect(toFilterView({ ...DEFAULT_FILTER, range: "all", search: "  fix ", path: " ", branch: " main ", mine: true, authors: ["Rin"] })).toEqual({
      search: "fix",
      branch: "main",
      mine: true,
      authors: ["Rin"],
    });
  });
  it("repairs a stored filter", () => {
    expect(parseFilter(null)).toEqual(DEFAULT_FILTER);
    expect(parseFilter({ range: "yesterday", authors: ["Rin", 4, " "], from: "bad", search: 3, mine: "yes" })).toEqual({ ...DEFAULT_FILTER, authors: ["Rin"] });
    expect(parseFilter({ range: "7d", mine: true, from: "2026-01-02" })).toMatchObject({ range: "7d", mine: true, from: "2026-01-02" });
  });
  it("adds an author once, ignoring case", () => {
    expect(addAuthor(["Rin"], " rin ")).toEqual(["Rin"]);
    expect(addAuthor(["Rin"], "Sam")).toEqual(["Rin", "Sam"]);
    expect(addAuthor([], "  ")).toEqual([]);
  });
});

describe("keyboard selection", () => {
  it("moves one row, clamped to the list", () => {
    expect(navigate(10, 3, "ArrowDown", 5)).toBe(4);
    expect(navigate(10, 9, "ArrowDown", 5)).toBe(9);
    expect(navigate(10, 0, "ArrowUp", 5)).toBe(0);
  });
  it("starts at the top with nothing selected", () => {
    expect(navigate(10, -1, "ArrowDown", 5)).toBe(0);
    expect(navigate(10, -1, "ArrowUp", 5)).toBe(0);
    expect(navigate(0, -1, "ArrowDown", 5)).toBe(-1);
  });
  it("jumps by Home, End and a page", () => {
    expect(navigate(100, 50, "Home", 8)).toBe(0);
    expect(navigate(100, 50, "End", 8)).toBe(99);
    expect(navigate(100, 50, "PageDown", 8)).toBe(58);
    expect(navigate(100, 50, "PageUp", 8)).toBe(42);
    expect(navigate(100, 96, "PageDown", 8)).toBe(99);
    expect(navigate(100, 3, "PageUp", 8)).toBe(0);
  });
  it("keeps the selected commit while it is listed, drops it when it is gone", () => {
    const list = rows(0, 5);
    const k = rowKey(list[2]);
    expect(keepSelection(list, k)).toBe(k);
    expect(keepSelection(list.slice(3), k)).toBeNull();
    expect(keepSelection(list, null)).toBeNull();
  });
});

describe("windowing", () => {
  it("draws only the visible rows and a margin", () => {
    const r = visibleRange(3000, 600, ROW_H, 10_000, 8);
    expect(r.start).toBe(100 - 8);
    expect(r.end).toBe(120 + 8);
    expect(visibleRange(0, 600, ROW_H, 5)).toEqual({ start: 0, end: 5 });
    expect(visibleRange(0, 600, ROW_H, 0)).toEqual({ start: 0, end: 0 });
  });
  it("scrolls the least that reveals a row", () => {
    expect(scrollToReveal(5, 0, 300, 30)).toBe(0);
    expect(scrollToReveal(20, 0, 300, 30)).toBe(330);
    expect(scrollToReveal(2, 300, 300, 30)).toBe(60);
  });
  it("asks for the next page near the end", () => {
    expect(nearEnd(90, 100)).toBe(true);
    expect(nearEnd(50, 100)).toBe(false);
    expect(nearEnd(0, 0)).toBe(false);
  });
});

describe("File History", () => {
  const R = "/w/acme-api/.git";
  it("history sets search, authors, Me, path and dates aside and keeps the branch", () => {
    const f = { ...DEFAULT_FILTER, search: "fix", authors: ["dana"], mine: true, path: "src", range: "7d" as const, branch: "release-1.4" };
    expect(historyRequest(f, { repo: R, path: "a.ts" })).toEqual({ repos: [R], filter: { branch: "release-1.4" }, history: { repo: R, path: "a.ts" } });
    expect(historyRequest({ ...f, branch: " " }, { repo: R, path: "a.ts" }).filter).toEqual({});
  });
  it("names the file in the banner", () => {
    expect(historyBanner("src/new.ts")).toBe("History of src/new.ts");
    expect(HISTORY_KEY).toBe("h");
  });
  it("the diff follows the file's path at each commit", () => {
    const file = (path: string): ChangedFile => ({ path, status: "modified", from: null, added: 1, removed: 0 });
    const files = [file("new.ts"), file("old.ts")];
    const r = row(1, { file: { path: "old.ts", from: null, status: "modified" } });
    expect(pickFile(files, r.file?.path ?? null)?.path).toBe("old.ts");
    expect(pickFile(files, null)?.path).toBe("new.ts");
  });
  it("sends the history request through to the fetcher", async () => {
    const seen: unknown[] = [];
    const q = new LogQuery(async (id, req) => (seen.push(req.history), page(id, [], true)), () => {}, new QueryGate(1));
    await q.start(historyRequest(DEFAULT_FILTER, { repo: R, path: "a.ts" }));
    expect(seen).toEqual([{ repo: R, path: "a.ts" }]);
  });
});
