import { describe, expect, it } from "vitest";
import { activeIndex, highlight, inReposText, noMatchText, pickerGroups, pushRecent, type BranchName, type Pair } from "./compare";

const pair = (left: string, right: string, leftRemote = false, rightRemote = false): Pair => ({ left, leftRemote, right, rightRemote });
const p1 = pair("origin/release-1.4", "origin/main", true, true);
const p2 = pair("origin/release-1.3", "origin/prod", true, true);

const names: BranchName[] = [
  { name: "origin/main", count: 68, remote: true },
  { name: "main", count: 68, remote: false },
  { name: "origin/release-1.4", count: 56, remote: true },
  { name: "feat/billing", count: 3, remote: false },
];

describe("compare helpers", () => {
  it("groups and filters branch names", () => {
    const fav = [{ name: "origin/main", remote: true }];
    const groups = pickerGroups(names, fav, [p1], "");
    expect(groups.map((g) => [g.title, g.items.length])).toEqual([["Recent pairs", 1], ["Favorites", 1], ["Local", 2], ["Remote", 2]]);
    const searched = pickerGroups(names, fav, [p1], "REL");
    expect(searched.map((g) => g.title)).toEqual(["Remote"]);
    expect(pickerGroups(names, [], [], "zzz")).toEqual([]);
    // By count, then name.
    const local = groups[2].items.map((i) => (i.kind === "name" ? i.name : ""));
    expect(local).toEqual(["main", "feat/billing"]);
  });

  it("keys a branch on its name and kind", () => {
    const both: BranchName[] = [
      { name: "origin/x", count: 1, remote: false },
      { name: "origin/x", count: 2, remote: true },
    ];
    // Only the remote one is a favourite, and only it is marked.
    const g = pickerGroups(both, [{ name: "origin/x", remote: true }], [], "");
    expect(g.map((x) => x.title)).toEqual(["Favorites", "Local", "Remote"]);
    expect(g[0].items).toEqual([{ kind: "name", name: "origin/x", remote: true, count: 2, favorite: true }]);
    expect(g[1].items).toEqual([{ kind: "name", name: "origin/x", remote: false, count: 1, favorite: false }]);
  });

  it("marks the matched part", () => {
    expect(highlight("origin/release-1.4", "REL")).toEqual({ pre: "origin/", hit: "rel", post: "ease-1.4" });
    expect(highlight("main", "")).toEqual({ pre: "main", hit: "", post: "" });
    expect(highlight("main", "zz")).toEqual({ pre: "main", hit: "", post: "" });
  });

  it("counts repositories", () => {
    expect(inReposText(1)).toBe("in 1 repository");
    expect(inReposText(3)).toBe("in 3 repositories");
  });

  it("says when no branch matches", () => {
    expect(noMatchText("zzz")).toBe('No branch named "zzz" in any repository.');
  });

  it("keeps the active row when its Favorites copy goes (D2)", () => {
    const before = ["Favorites\u0000false:main", "Branches\u0000false:dev", "Branches\u0000false:main"];
    const after = ["Branches\u0000false:dev", "Branches\u0000false:main"];
    // Nothing moved yet: the highlight is only a position, and index 0 is now another branch.
    expect(after[activeIndex(after, null, 0)]).toBe("Branches\u0000false:dev");
    // Ctrl+D pins the row by its key first (setActive(active)): it follows main to its own group.
    const pinned = before[activeIndex(before, null, 0)];
    expect(after[activeIndex(after, pinned, 0)]).toBe("Branches\u0000false:main");
  });

  it("keeps recent pairs newest first, once each, at most five", () => {
    let recent = pushRecent([], p1);
    recent = pushRecent(recent, p2);
    recent = pushRecent(recent, p1);
    expect(recent).toEqual([p1, p2]);
    // The same names with another kind is another pair.
    expect(pushRecent(recent, pair("origin/release-1.4", "origin/main", false, true))).toHaveLength(3);
    for (let i = 0; i < 9; i++) recent = pushRecent(recent, pair(`rel-${i}`, "main"));
    expect(recent).toHaveLength(5);
  });
});

// ---- Compare mode (Task 12) ------------------------------------------------------------------
import { QueryGate } from "./log";
import {
  bothStripText,
  columnHeader,
  compareBanner,
  compareMessage,
  CompareLoad,
  emptyColumnText,
  missingText,
  mirror,
  repoCounts,
  sidePage,
  summaryLabel,
  type CompareState,
  type CompareView,
  type RepoCompare,
} from "./compare";

const A = "a".repeat(40);
const B = "b".repeat(40);
const C = "c".repeat(40);
const differs = (left: number, right: number, sameLeft = 0, sameRight = sameLeft): RepoCompare => ({ kind: "differs", leftSha: A, rightSha: B, base: C, left, right, sameLeft, sameRight });

describe("compare mode", () => {
  it("summarises the comparison without commit numbers", () => {
    const d = differs(2, 1, 1);
    const results: RepoCompare[] = [d, { ...differs(5, 0) }, { kind: "identical" }, { kind: "missing" }, { kind: "missing" }, { kind: "nobase" }];
    expect(summaryLabel(results, 6)).toBe("3 repositories differ · 1 identical · 2 missing a branch · in 6 repositories");
    expect(summaryLabel([{ kind: "identical" }], 1)).toBe("0 repositories differ · 1 identical · in 1 repository");
    expect(summaryLabel([differs(5, 0)], 1)).toBe("1 repository differs · in 1 repository");
  });

  it("counts each side and the duplicates, zeros left out", () => {
    expect(repoCounts(differs(3, 1, 1))).toBe("◀3 ▶1 =1");
    expect(repoCounts(differs(3, 0))).toBe("◀3");
    expect(repoCounts(differs(0, 2))).toBe("▶2");
    expect(repoCounts(differs(0, 0, 3))).toBe("=3");
    expect(repoCounts({ kind: "nobase" })).toBe("no common history");
    expect(repoCounts({ kind: "error", reason: "fatal: x" })).toBe("git error");
  });

  it("mirrors a result on swap", () => {
    expect(mirror(differs(2, 1, 1, 3))).toEqual({ kind: "differs", leftSha: B, rightSha: A, base: C, left: 1, right: 2, sameLeft: 3, sameRight: 1 });
    expect(mirror({ kind: "missing" })).toEqual({ kind: "missing" });
    expect(mirror({ kind: "error", reason: "x" })).toEqual({ kind: "error", reason: "x" });
  });

  it("names the repositories missing a branch", () => {
    expect(missingText(p1, 12)).toBe("origin/release-1.4 or origin/main is missing in 12 repositories");
    expect(missingText(p1, 1)).toBe("origin/release-1.4 or origin/main is missing in 1 repository");
  });

  it("heads each column", () => {
    expect(columnHeader("left", "files", 3, p1)).toBe("3 files · since the split");
    expect(columnHeader("right", "files", 1, p1)).toBe("1 file · since the split");
    expect(columnHeader("left", "commits", 2, p1)).toBe("2 commits · the merge brings these in");
    expect(columnHeader("right", "commits", 2, p1)).toBe("2 commits · missing from origin/release-1.4");
    expect(columnHeader("left", "commits", 1, p1)).toBe("1 commit · the merge brings these in");
  });

  it("says when a side has nothing", () => {
    expect(emptyColumnText("left", p1)).toBe("Nothing here: origin/main already has every commit of origin/release-1.4.");
    expect(emptyColumnText("right", p1)).toBe("Nothing here: origin/release-1.4 already has every commit of origin/main.");
  });

  it("explains the On both strip", () => {
    expect(bothStripText(1)).toBe("= 1 on both — the same change committed on each side (cherry-picked): the merge does not repeat it, but history lists it twice");
  });

  it("shows the right message for each state", () => {
    const rows = (...r: RepoCompare[]) => r;
    expect(compareMessage(pair("main", "main"), 3, null)).toBe("Pick two different branches. Both sides are `main`.");
    // The same name as a local and a remote-tracking branch is two refs.
    expect(compareMessage(pair("origin/x", "origin/x", false, true), 3, rows(differs(1, 0)))).toBeNull();
    expect(compareMessage(pair("main", ""), 3, null)).toBe("Pick the branch to merge from (◀) and the branch it goes into (▶).");
    expect(compareMessage(null, 3, null)).toBe("Pick the branch to merge from (◀) and the branch it goes into (▶).");
    expect(compareMessage(p1, 3, rows({ kind: "identical" }, { kind: "identical" }, { kind: "missing" }))).toBe("origin/release-1.4 and origin/main are the same in every repository.");
    expect(compareMessage(p1, 0, null)).toBe("No repositories are ticked in the Repo List.");
    expect(compareMessage(p1, 3, rows(differs(1, 0), { kind: "identical" }))).toBeNull();
    expect(compareMessage(p1, 3, rows({ kind: "nobase" }))).toBeNull();
    expect(compareMessage(p1, 68, null)).toBe("Reading 68 repositories…");
    expect(compareMessage(p1, 1, null)).toBe("Reading 1 repository…");
  });

  it("offers more commits whenever git returned more than the page", () => {
    const sc = (sha: string, mark: "+" | "=") => ({ mark, sha, parents: [], time: 0, author: "dana", subject: sha });
    expect(sidePage([sc("a", "+"), sc("b", "="), sc("c", "+")], 2)).toEqual({ rows: [sc("a", "+")], more: true });
    expect(sidePage([sc("a", "+"), sc("c", "+")], 2)).toEqual({ rows: [sc("a", "+"), sc("c", "+")], more: false });
  });

  it("labels a compare diff", () => {
    // A remote side keeps origin/: local main and origin/main must not read the same.
    expect(compareBanner("left", p1)).toEqual({ tone: "info", text: "merge base ↔ origin/release-1.4" });
    expect(compareBanner("right", p1)).toEqual({ tone: "info", text: "merge base ↔ origin/main" });
    expect(compareBanner("right", pair("origin/main", "main", true, false))).toEqual({ tone: "info", text: "merge base ↔ main" });
    // A local branch keeps its whole name.
    expect(compareBanner("left", pair("origin/x", "main"))).toEqual({ tone: "info", text: "merge base ↔ origin/x" });
  });

  it("a superseded compare answer is ignored", async () => {
    const deferred = <T>() => {
      let resolve!: (v: T) => void;
      const promise = new Promise<T>((a) => (resolve = a));
      return { promise, resolve };
    };
    const calls: { id: number; d: ReturnType<typeof deferred<CompareView>> }[] = [];
    const states: CompareState[] = [];
    const load = new CompareLoad(
      (id) => {
        const d = deferred<CompareView>();
        calls.push({ id, d });
        return d.promise;
      },
      (s) => states.push(s),
      new QueryGate(1000),
    );
    const view = (queryId: number, repo: string): CompareView => ({ queryId, rows: [{ repo, kind: "identical" }] });
    const a = load.start(["r"], p1);
    const b = load.start(["r"], p2);
    expect(calls[1].id).toBeGreaterThan(calls[0].id);
    expect(load.id).toBe(calls[1].id);
    calls[1].d.resolve(view(calls[1].id, "B"));
    await b;
    calls[0].d.resolve(view(calls[0].id, "A"));
    await a;
    expect(states.at(-1)?.view?.rows[0].repo).toBe("B");
    expect(states.at(-1)?.loading).toBe(false);
    // Swap mirrors what is shown, without a read.
    load.mirror();
    expect(calls).toHaveLength(2);
  });
});

// ---- fix round 1 ------------------------------------------------------------------------------
import { GatedRead, repoCountsLabel, type ReadState } from "./compare";

describe("compare fix round 1", () => {
  const deferred = <T>() => {
    let resolve!: (v: T) => void;
    let reject!: (e: unknown) => void;
    const promise = new Promise<T>((a, b) => ((resolve = a), (reject = b)));
    return { promise, resolve, reject };
  };

  it("a stale column read ends, and is due again for the next query", async () => {
    const states: ReadState<string>[] = [];
    const read = new GatedRead<string>((s) => states.push(s));
    expect(read.neededFor(1)).toBe(true);
    const d = deferred<string>();
    const run = read.run(1, () => d.promise);
    expect(read.state.loading).toBe(true);
    // ↻ starts query 2 while this read runs: nothing is due yet, the read is still out.
    expect(read.neededFor(2)).toBe(false);
    // Then the backend answers it STALE (after query 2 already landed, with the same tips).
    d.reject("stale query");
    await run;
    expect(read.state).toEqual({ data: null, loading: false, error: null });
    // Due again for query 2 (no spinner forever), never again for query 1 (no re-read loop).
    expect(read.neededFor(1)).toBe(false);
    expect(read.neededFor(2)).toBe(true);
    const d2 = deferred<string>();
    const run2 = read.run(2, () => d2.promise);
    d2.resolve("files");
    await run2;
    expect(read.state).toEqual({ data: "files", loading: false, error: null });
    expect(read.neededFor(3)).toBe(false);
  });

  it("a superseded column read is dropped; a real failure is kept", async () => {
    const read = new GatedRead<string>(() => {});
    const a = deferred<string>();
    const b = deferred<string>();
    const ra = read.run(1, () => a.promise);
    const rb = read.run(1, () => b.promise);
    b.resolve("B");
    await rb;
    a.resolve("A");
    await ra;
    expect(read.state.data).toBe("B");
    const c = deferred<string>();
    const rc = read.run(1, () => c.promise);
    c.reject("fatal: bad object");
    await rc;
    expect(read.state).toEqual({ data: "B", loading: false, error: "fatal: bad object" });
    read.reset();
    expect(read.neededFor(1)).toBe(true);
  });

  it("keeps the pair its rows answer, and mirrors it on swap", async () => {
    const states: CompareState[] = [];
    const d = deferred<CompareView>();
    const load = new CompareLoad(() => d.promise, (s) => states.push(s), new QueryGate(5000));
    const run = load.start(["r"], p1);
    d.resolve({ queryId: load.id, rows: [{ repo: "r", ...differs(1, 2) }] });
    await run;
    expect(load.state.pair).toEqual(p1);
    // A newer read in flight leaves the shown rows with their own pair.
    void load.start(["r"], p2);
    expect(load.state.loading).toBe(true);
    expect(load.state.pair).toEqual(p1);
    const m = new CompareLoad(async (id) => ({ queryId: id, rows: [] }), () => {}, new QueryGate(9000));
    await m.start(["r"], p1);
    m.mirror();
    expect(m.state.pair).toEqual(pair("origin/main", "origin/release-1.4", true, true));
  });

  it("says a repo row's counts in words", () => {
    expect(repoCountsLabel(differs(3, 1, 1), p1)).toBe("3 only on origin/release-1.4, 1 only on origin/main, 1 on both");
    expect(repoCountsLabel(differs(0, 2), p1)).toBe("2 only on origin/main");
    expect(repoCountsLabel({ kind: "nobase" }, p1)).toBe("no common history");
  });
});
