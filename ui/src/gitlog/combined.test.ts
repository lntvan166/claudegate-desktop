import { describe, expect, it } from "vitest";
import {
  CombinedLoad,
  combinedFirst,
  combinedNotice,
  combinedPlan,
  combinedTitle,
  enterStep,
  reconcileOpen,
  staleStep,
  isCombined,
  itemsOf,
  noNetChangeText,
  orderRepos,
  shaList,
  warnText,
  type CombinedFile,
  type CombinedRepo,
  type CombinedState,
  type CombinedView,
} from "./combined";
import { QueryGate, type RowView } from "./log";

const sha = (c: string) => c.repeat(40);
const file = (path: string, extra: Partial<CombinedFile> = {}): CombinedFile => ({
  path,
  from: null,
  status: "modified",
  shas: [sha("a")],
  last: sha("a"),
  beforeRef: sha("0"),
  beforePath: path,
  included: [],
  mode: "fast",
  reason: null,
  ...extra,
});
const repo = (id: string, files: CombinedFile[] = []): CombinedRepo => ({ repo: id, commits: [], files, noNetChange: [], subjects: {}, error: null });
const row = (r: string, s: string): RowView => ({ repo: r, sha: s, time: 0, author: "Rin", email: "rin@acme.test", subject: "x", parents: [], source: "HEAD" });

describe("combined text", () => {
  it("titles the combined tree", () => {
    expect(combinedTitle(4, 2)).toBe("4 commits · 2 repositories");
    expect(combinedTitle(2, 1)).toBe("2 commits · 1 repository");
  });
  it("lists the shas that touched a file, oldest first", () => {
    expect(shaList([sha("a"), sha("c")])).toBe("aaaaaaa · ccccccc");
  });
  it("explains a fallback", () => {
    const subjects = { [sha("9")]: "header: wider title", [sha("1")]: "fix: x" };
    const conflict = (included: string[]) => file("a.ts", { mode: "fallback", reason: "conflict", included });
    expect(warnText(conflict([sha("9")]), subjects)).toBe("Includes changes from 1 unselected commit: 9999999 header: wider title");
    expect(warnText(conflict([sha("9"), sha("1")]), subjects)).toBe(
      "Includes changes from 2 unselected commits: 9999999 header: wider title; 1111111 fix: x",
    );
    expect(warnText(conflict([]), subjects)).toBe("The selected changes can't be shown apart here");
    expect(warnText(file("a.png", { mode: "fallback", reason: "binary" }), subjects)).toBe("Binary file: shown as one diff across the selection");
    expect(warnText(file("a.sql", { mode: "fallback", reason: "tooLarge" }), subjects)).toBe(
      "Too large to take apart: shown as one diff across the selection",
    );
    expect(warnText(file("a.ts", { mode: "merge" }), subjects)).toBeNull();
    expect(warnText(file("a.ts"), subjects)).toBeNull();
  });
  it("names the files with no net change", () => {
    expect(noNetChangeText(["src/debug.ts"])).toBe("1 file with no net change: src/debug.ts");
    expect(noNetChangeText(["src/debug.ts", "src/nav.ts"])).toBe("2 files with no net change: src/debug.ts, src/nav.ts");
    expect(noNetChangeText([])).toBeNull();
  });
});

describe("selection helpers", () => {
  it("is combined from two commits", () => {
    expect(isCombined([])).toBe(false);
    expect(isCombined([row("r", "a")])).toBe(false);
    expect(isCombined([row("r", "a"), row("r", "b")])).toBe(true);
    expect(itemsOf([row("r", "a"), row("q", "b")])).toEqual([
      { repo: "r", sha: "a" },
      { repo: "q", sha: "b" },
    ]);
  });
  it("plans each side's read", () => {
    expect(combinedPlan(file("a"))).toEqual({ left: "rev", right: "rev" });
    expect(combinedPlan(file("a", { status: "added" }))).toEqual({ left: "missing", right: "rev" });
    expect(combinedPlan(file("a", { beforeRef: null }))).toEqual({ left: "missing", right: "rev" });
    expect(combinedPlan(file("a", { status: "deleted" }))).toEqual({ left: "rev", right: "missing" });
    expect(combinedPlan(file("a", { mode: "merge" }))).toEqual({ left: "merge", right: "rev" });
    expect(combinedPlan(file("a", { mode: "fallback", reason: "conflict" }))).toEqual({ left: "rev", right: "rev" });
  });
  it("orders repositories as the Repo List does", () => {
    const out = orderRepos([repo("z"), repo("b"), repo("unknown"), repo("a")], ["a", "b", "z"]);
    expect(out.map((r) => r.repo)).toEqual(["a", "b", "z", "unknown"]);
    expect(orderRepos([repo("x"), repo("y")], []).map((r) => r.repo)).toEqual(["x", "y"]);
  });
  it("opens the first file of the first repository with files", () => {
    const repos = [repo("empty"), repo("r", [file("src/z.ts"), file("README.md"), file("src/a.ts")])];
    // Folders come first in the tree, then files: src/a.ts, src/z.ts, README.md.
    expect(combinedFirst(repos)).toEqual({ repo: "r", file: expect.objectContaining({ path: "src/a.ts" }) });
    expect(combinedFirst([repo("empty")])).toBeNull();
    expect(combinedFirst([])).toBeNull();
  });
});

describe("CombinedLoad", () => {
  const view = (queryId: number, tag: string): CombinedView => ({ queryId, repos: [repo(tag)] });
  const deferred = <T>() => {
    let resolve!: (v: T) => void;
    let reject!: (e: unknown) => void;
    const promise = new Promise<T>((a, b) => ((resolve = a), (reject = b)));
    return { promise, resolve, reject };
  };
  const items = (s: string) => [{ repo: "r", sha: s }];

  it("a superseded combined answer is ignored", async () => {
    const calls: { id: number; d: ReturnType<typeof deferred<CombinedView>> }[] = [];
    const states: CombinedState[] = [];
    const load = new CombinedLoad(
      (id) => {
        const d = deferred<CombinedView>();
        calls.push({ id, d });
        return d.promise;
      },
      (s) => states.push(s),
      new QueryGate(1000),
    );
    const a = load.start(items("a"));
    const b = load.start(items("b"));
    expect(calls[1].id).toBeGreaterThan(calls[0].id);
    expect(load.id).toBe(calls[1].id);
    calls[1].d.resolve(view(calls[1].id, "B"));
    await b;
    calls[0].d.resolve(view(calls[0].id, "A"));
    await a;
    expect(states.at(-1)?.view?.repos[0].repo).toBe("B");
    expect(states.at(-1)?.loading).toBe(false);
  });

  it("a refused older request leaves no error", async () => {
    const calls: ReturnType<typeof deferred<CombinedView>>[] = [];
    const ids: number[] = [];
    const states: CombinedState[] = [];
    const load = new CombinedLoad(
      (id) => {
        const d = deferred<CombinedView>();
        calls.push(d);
        ids.push(id);
        return d.promise;
      },
      (s) => states.push(s),
      new QueryGate(1000),
    );
    const a = load.start(items("a"));
    const b = load.start(items("b"));
    calls[0].reject("stale query");
    await a;
    expect(states.at(-1)?.error).toBeNull();
    expect(states.at(-1)?.loading).toBe(true);
    calls[1].resolve(view(ids[1], "B"));
    await b;
    expect(states.at(-1)?.view?.repos[0].repo).toBe("B");
  });

  it("drops an answer whose query id is not the one asked", async () => {
    const states: CombinedState[] = [];
    const load = new CombinedLoad(async (id) => view(id - 1, "old"), (s) => states.push(s), new QueryGate(1000));
    await load.start(items("a"));
    expect(states.at(-1)).toEqual({ loading: true, view: null, error: null });
  });

  it("the current query failing ends loading with the error", async () => {
    const states: CombinedState[] = [];
    const load = new CombinedLoad(() => Promise.reject("Combined Changes shows up to 100 commits"), (s) => states.push(s), new QueryGate(1000));
    await load.start(items("a"));
    expect(states.at(-1)).toEqual({ loading: false, view: null, error: "Combined Changes shows up to 100 commits" });
  });

  it("clear ignores what is in flight", async () => {
    const d = deferred<CombinedView>();
    const states: CombinedState[] = [];
    let asked = 0;
    const load = new CombinedLoad(
      (id) => ((asked = id), d.promise),
      (s) => states.push(s),
      new QueryGate(1000),
    );
    const p = load.start(items("a"));
    load.clear();
    d.resolve(view(asked, "A"));
    await p;
    expect(states.at(-1)).toEqual({ loading: false, view: null, error: null });
  });
});

describe("races of the open file", () => {
  const a = file("a.ts");
  it("re-reads a file whose read was cut short even when its entry is identical (I1)", () => {
    expect(reconcileOpen(a, { ...a }, false)).toBe("keep");
    expect(reconcileOpen(a, { ...a }, true)).toBe("reread");
    expect(reconcileOpen(a, file("a.ts", { last: sha("b") }), false)).toBe("reread");
  });
  it("closes a file the new answer no longer has, whatever else is pending (I2)", () => {
    expect(reconcileOpen(a, undefined, false)).toBe("close");
    expect(reconcileOpen(a, undefined, true)).toBe("close");
  });
  it("does not open the old tree while a fetch waits or is in flight (M1)", () => {
    expect(enterStep({ hasView: true, loading: true, pending: true })).toBe("flush");
    expect(enterStep({ hasView: true, loading: false, pending: true })).toBe("flush");
    expect(enterStep({ hasView: true, loading: true, pending: false })).toBe("wait");
    expect(enterStep({ hasView: true, loading: false, pending: false })).toBe("open");
    expect(enterStep({ hasView: false, loading: false, pending: false })).toBe("start");
  });
  it("retries a stale left read once, after the answer (M7)", () => {
    expect(staleStep({ superseded: true, loading: true, retried: false })).toBe("wait");
    // The newer answer landed while the read ran: nothing will re-read the file, so read it now.
    expect(staleStep({ superseded: true, loading: false, retried: false })).toBe("retry");
    expect(staleStep({ superseded: false, loading: true, retried: false })).toBe("wait");
    expect(staleStep({ superseded: false, loading: false, retried: false })).toBe("retry");
    expect(staleStep({ superseded: false, loading: false, retried: true })).toBe("fail");
  });
});

describe("combinedNotice", () => {
  const a = sha("a");
  const c = sha("c");
  const x = sha("9");
  const subjects = { [x]: "header: wider title", [sha("1")]: "fix: x" };
  const f = (extra: Partial<CombinedFile>) => file("a.ts", { shas: [a, c], last: c, ...extra });
  const range = "aaaaaaa^ ↔ ccccccc";
  it("says nothing for one commit on the fast path", () => {
    expect(combinedNotice(file("a.ts"), {}, null)).toBeNull();
  });
  it("names the selected commits on the fast path", () => {
    expect(combinedNotice(f({}), {}, null)).toEqual({ tone: "info", text: "2 selected commits: aaaaaaa · ccccccc" });
  });
  it("says unselected edits are context after a clean merge", () => {
    expect(combinedNotice(f({ mode: "merge", included: [x] }), subjects, null)?.text).toBe(
      "2 selected commits: aaaaaaa · ccccccc · edits from 1 unselected commit shown as unchanged context",
    );
    expect(combinedNotice(f({ mode: "merge" }), subjects, null)?.text).toBe("2 selected commits: aaaaaaa · ccccccc");
    expect(combinedNotice(file("a.ts", { mode: "merge", included: [x] }), subjects, null)?.text).toMatch(/^1 selected commit: aaaaaaa · edits/);
  });
  it("warns when the changes can't be shown apart", () => {
    const conflict = (included: string[]) => f({ mode: "fallback", reason: "conflict", included });
    expect(combinedNotice(conflict([x]), subjects, null)).toEqual({
      tone: "warn",
      text: `${range} · includes 1 unselected commit: 9999999 header: wider title`,
    });
    expect(combinedNotice(conflict([x, sha("1")]), subjects, null)?.text).toBe(
      `${range} · includes 2 unselected commits: 9999999 header: wider title; 1111111 fix: x`,
    );
    expect(combinedNotice(conflict([]), subjects, null)?.text).toBe(`${range} · the selected changes can't be shown apart`);
  });
  it("labels binary and too-large fallbacks and still names included commits", () => {
    expect(combinedNotice(f({ mode: "fallback", reason: "binary" }), subjects, null)?.text).toBe(`${range} · binary`);
    expect(combinedNotice(f({ mode: "fallback", reason: "tooLarge" }), subjects, null)?.text).toBe(`${range} · too large`);
    expect(combinedNotice(f({ mode: "fallback", reason: "binary", included: [x] }), subjects, null)?.text).toBe(
      `${range} · binary · includes 1 unselected commit: 9999999 header: wider title`,
    );
  });
  it("a fallback from the left-side read overrides a merge", () => {
    expect(combinedNotice(f({ mode: "merge", included: [x] }), subjects, { kind: "fallback", value: "conflict" })).toEqual({
      tone: "warn",
      text: `${range} · includes 1 unselected commit: 9999999 header: wider title`,
    });
    expect(combinedNotice(f({ mode: "merge" }), subjects, { kind: "text", value: "" })?.tone).toBe("info");
  });
});
