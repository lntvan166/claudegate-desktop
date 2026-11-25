import { describe, expect, it } from "vitest";
import type { ProjectView, ScopeView } from "./api";
import { filterBulk, filterGroups, filterLabel, matchesFilter, matchPrompt, normalizeFilter, pendingFilterPath, stillShown } from "./filter";
import type { Group, Row } from "./rows";

const scope = (root: string, isWorktree = false): ScopeView => ({ root, isWorktree, shapeUnknown: [], error: null });
const row = (scopeRoot: string, rel: string): Row => ({
  key: `${scopeRoot}/${rel}`,
  path: `${scopeRoot}/${rel}`,
  rel,
  scope: scopeRoot,
  added: 1,
  removed: 0,
  newFile: false,
  decidedAt: null,
  waiting: false,
  rejectOutcome: "restore",
});

describe("normalizeFilter", () => {
  it("collapses empty input to a single null state", () => {
    expect(normalizeFilter(undefined)).toBeNull();
    expect(normalizeFilter(null)).toBeNull();
    expect(normalizeFilter("")).toBeNull();
    expect(normalizeFilter("   ")).toBeNull();
    expect(normalizeFilter("  auth  ")).toBe("auth");
  });
});

describe("matchesFilter", () => {
  it("matches everything for a null filter", () => {
    expect(matchesFilter("/repo/pkg/ws/auth.go", null)).toBe(true);
  });
  it("is a case-insensitive substring over the whole path", () => {
    const p = "/repo/wt-feature/pkg/ws/auth.go";
    expect(matchesFilter(p, "auth")).toBe(true);
    expect(matchesFilter(p, "AUTH")).toBe(true);
    expect(matchesFilter(p, "pkg/ws")).toBe(true);
    expect(matchesFilter(p, "wt-feature")).toBe(true);
    expect(matchesFilter(p, "handler")).toBe(false);
  });
  it("normalises path separators both ways", () => {
    expect(matchesFilter("C:\\repo\\pkg\\ws\\auth.go", "pkg/ws")).toBe(true);
    expect(matchesFilter("/repo/pkg/ws/auth.go", "pkg\\ws")).toBe(true);
  });
});

describe("filterLabel", () => {
  it("is silent unfiltered and reports shown-of-total otherwise", () => {
    expect(filterLabel(null, 16, 16)).toBeUndefined();
    expect(filterLabel("auth", 4, 16)).toBe("auth — 4 of 16");
    expect(filterLabel("zzz", 0, 16)).toBe("zzz — 0 of 16");
  });
});

const ROOT = "/ws/a";
const WT = "/ws/a/.wt/x";
const view = { root: ROOT, scopes: [scope(ROOT), scope(WT, true)] } as unknown as ProjectView;

describe("pendingFilterPath", () => {
  it("is relative to the project root for the primary scope", () => {
    expect(pendingFilterPath(row(ROOT, "src/a.ts"), view)).toBe("src/a.ts");
  });
  it("is prefixed with the scope label for a worktree", () => {
    expect(pendingFilterPath(row(WT, "z.ts"), view)).toBe(".wt/x/z.ts");
  });
  it("does not match the project root's own name", () => {
    expect(matchesFilter(pendingFilterPath(row(ROOT, "src/a.ts"), view), "ws")).toBe(false);
  });
});

describe("filterGroups", () => {
  const groups: Group[] = [
    { scope: scope(ROOT), label: "a", rows: [row(ROOT, "src/auth.ts"), row(ROOT, "README.md")] },
    { scope: scope(WT, true), label: ".wt/x", rows: [row(WT, "z.ts")] },
  ];
  it("returns the groups untouched with no filter", () => {
    expect(filterGroups(groups, view, null)).toBe(groups);
  });
  it("keeps matching rows and drops emptied groups", () => {
    const out = filterGroups(groups, view, "auth");
    expect(out).toHaveLength(1);
    expect(out[0].rows.map((r) => r.rel)).toEqual(["src/auth.ts"]);
  });
  it("finds a worktree's rows by its name", () => {
    const out = filterGroups(groups, view, "wt/x");
    expect(out).toHaveLength(1);
    expect(out[0].rows.map((r) => r.rel)).toEqual(["z.ts"]);
  });
});

describe("matchPrompt", () => {
  it("uses the extension's copy", () => {
    expect(matchPrompt("accept", 3, "auth")).toEqual({
      text: 'Accept 3 file(s) matching "auth"? Only files matching the filter are accepted. Anything the filter hides stays pending.',
      go: "Accept 3",
      danger: false,
    });
    expect(matchPrompt("reject", 2, "auth")).toEqual({
      text: 'Reject 2 file(s) matching "auth"? This restores their original content on disk. Files the filter hides are untouched.',
      go: "Reject 2",
      danger: true,
    });
  });
});

describe("stillShown", () => {
  const g: Group[] = [{ scope: scope(ROOT), label: "a", rows: [row(ROOT, "src/auth.ts")] }];
  it("is true only for a row in the shown groups", () => {
    expect(stillShown({ scope: ROOT, key: `${ROOT}/src/auth.ts` }, g)).toBe(true);
    expect(stillShown({ scope: ROOT, key: `${ROOT}/README.md` }, g)).toBe(false);
    expect(stillShown({ scope: ROOT, key: `${ROOT}/src/auth.ts` }, [])).toBe(false);
    // The same key in another scope is another row.
    expect(stillShown({ scope: `${ROOT}/nested`, key: `${ROOT}/src/auth.ts` }, g)).toBe(false);
  });
});

describe("filterBulk", () => {
  it("keeps only the matching set, in order, and never adds a key", () => {
    const shown = new Set(["/a", "/c", "/zzz"]);
    expect(filterBulk(["/a", "/b", "/c"], shown)).toEqual(["/a", "/c"]);
    expect(filterBulk(["/b"], shown)).toEqual([]);
    expect(filterBulk([], shown)).toEqual([]);
  });
});
