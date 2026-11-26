import { describe, expect, it } from "vitest";
import {
  PALETTE_LEN,
  aheadBehind,
  applyProfile,
  colorVar,
  fuzzyFilter,
  fuzzyScore,
  isTicked,
  profileNames,
  rangeOf,
  tickedIds,
  tickedCount,
  type RepoView,
} from "./repos";

function repo(id: string, name: string, extra: Partial<RepoView> = {}): RepoView {
  return { id, name, color: 0, checkouts: [{ path: id.replace(/\/\.git$/, ""), head: "a".repeat(40), branch: "main", isMain: true }], ahead: null, behind: null, ...extra };
}
const API = repo("/w/acme-api/.git", "acme-api");
const WEB = repo("/w/acme-web/.git", "acme-web");
const LIB = repo("/w/svc-lib/.git", "svc-lib");
const REPOS = [API, WEB, LIB];

describe("fuzzy matching", () => {
  it("matches a subsequence, case-insensitively", () => {
    expect(fuzzyScore("aw", "acme-web")).not.toBeNull();
    expect(fuzzyScore("AWB", "acme-web")).not.toBeNull();
    expect(fuzzyScore("wa", "acme-web")).toBeNull();
    expect(fuzzyScore("", "anything")).toBe(0);
  });
  it("ranks a contiguous or word-start match above a scattered one", () => {
    const contiguous = fuzzyScore("lib", "svc-lib")!;
    const scattered = fuzzyScore("lib", "lambda-ibx-b")!;
    expect(contiguous).toBeGreaterThan(scattered);
    expect(fuzzyScore("web", "acme-web")!).toBeGreaterThan(fuzzyScore("web", "awesome-bed")!);
  });
  it("filters and orders the list; an empty query keeps every repo in order", () => {
    expect(fuzzyFilter(REPOS, "").map((r) => r.name)).toEqual(["acme-api", "acme-web", "svc-lib"]);
    expect(fuzzyFilter(REPOS, "web").map((r) => r.name)).toEqual(["acme-web"]);
    expect(fuzzyFilter(REPOS, "ac").map((r) => r.name)).toEqual(["acme-api", "acme-web"]);
    expect(fuzzyFilter(REPOS, "  zz ")).toEqual([]);
  });
});

describe("ticks", () => {
  it("ticked: null means every repo", () => {
    expect(tickedIds(null, REPOS)).toEqual(REPOS.map((r) => r.id));
    expect(isTicked(null, LIB.id)).toBe(true);
    expect(tickedCount(null, REPOS)).toBe(3);
  });
  it("a list ticks only the repos it names that still exist", () => {
    const t = [WEB.id, "/gone/.git"];
    expect(tickedIds(t, REPOS)).toEqual([WEB.id]);
    expect(isTicked(t, API.id)).toBe(false);
    expect(tickedCount(t, REPOS)).toBe(1);
  });
});

describe("profiles", () => {
  it("applying a profile ticks exactly its repos and leaves the filters alone", () => {
    const filter = { search: "refund", authors: ["An"], within: 86400 };
    const state = { ticked: null as string[] | null, filter };
    const out = applyProfile(state, [LIB.id, API.id], REPOS);
    expect(out.state.ticked).toEqual([LIB.id, API.id]);
    expect(out.state.filter).toBe(filter);
    expect(out.missing).toBe(0);
    expect(tickedIds(out.state.ticked, REPOS)).toEqual([API.id, LIB.id]);
  });
  it("skips an id that is no longer discovered, silently, and counts it", () => {
    const out = applyProfile({ ticked: [WEB.id] }, [API.id, "/gone/one/.git", "/gone/two/.git"], REPOS);
    expect(out.state.ticked).toEqual([API.id]);
    expect(out.missing).toBe(2);
  });
  it("lists profile names sorted, case-insensitively", () => {
    expect(profileNames({ zeta: [], Alpha: [], beta: [] })).toEqual(["Alpha", "beta", "zeta"]);
  });
});

describe("display helpers", () => {
  it("has twelve palette slots, wrapping any index into them", () => {
    expect(PALETTE_LEN).toBe(12);
    expect(colorVar(3)).toBe("var(--repo-3)");
    expect(colorVar(15)).toBe("var(--repo-3)");
    expect(colorVar(-1)).toBe("var(--repo-11)");
  });
  it("shows ↓behind ↑ahead only when there is something to show", () => {
    expect(aheadBehind(API)).toBe("");
    expect(aheadBehind({ ...API, ahead: 0, behind: 0 })).toBe("");
    expect(aheadBehind({ ...API, ahead: 1, behind: 2 })).toBe("↓2 ↑1");
    expect(aheadBehind({ ...API, ahead: 3, behind: 0 })).toBe("↑3");
  });
  it("a shift-click range covers the rows between anchor and target, either way", () => {
    const order = ["a", "b", "c", "d"];
    expect(rangeOf(order, "b", "d")).toEqual(["b", "c", "d"]);
    expect(rangeOf(order, "d", "b")).toEqual(["b", "c", "d"]);
    expect(rangeOf(order, null, "c")).toEqual(["c"]);
    expect(rangeOf(order, "x", "c")).toEqual(["c"]);
  });
});

describe("pane widths", () => {
  it("reads stored widths, falling back per pane", async () => {
    const { panesFrom, PANE_DEFAULTS } = await import("./repos");
    expect(panesFrom({})).toEqual(PANE_DEFAULTS);
    expect(panesFrom({ repos: 300, changes: -4 })).toEqual({ repos: 300, changes: PANE_DEFAULTS.changes });
  });
  it("fits the window: Changes gives way first, the diff keeps its minimum", async () => {
    const { fitPanes, PANE_MIN, REPOS_RAIL } = await import("./repos");
    expect(fitPanes({ repos: 240, changes: 320 }, 1600, false)).toEqual({ repos: 240, changes: 320 });
    const tight = fitPanes({ repos: 240, changes: 320 }, 900, false);
    expect(tight.repos).toBe(240);
    expect(tight.changes).toBe(900 - 10 - PANE_MIN.diff - 240);
    expect(fitPanes({ repos: 400, changes: 320 }, 700, false)).toEqual({ repos: PANE_MIN.repos, changes: PANE_MIN.changes });
    expect(fitPanes({ repos: 400, changes: 320 }, 1600, true).repos).toBe(REPOS_RAIL);
  });
});
