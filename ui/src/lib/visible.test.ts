import { describe, expect, it } from "vitest";
import type { ScopeView } from "./api";
import type { Group, Row } from "./rows";
import { collapseKeysToReveal, fileOrder, visibleItems } from "./visible";

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

const A = "/ws/a";
const W = "/ws/a/.wt/x";
const groups: Group[] = [
  { scope: scope(A), label: "a", rows: [row(A, "src/api/b.ts"), row(A, "src/a.ts"), row(A, "README.md")] },
  { scope: scope(W, true), label: ".wt/x", rows: [row(W, "z.ts")] },
];

const none = { has: () => false };

describe("visibleItems", () => {
  it("lists groups, folders and files in display order", () => {
    const items = visibleItems(groups, { mode: "tree", showGroups: true, tab: "pending", collapsed: none });
    expect(items.map((i) => i.id)).toEqual([
      `g:${A}`,
      `d:${A}|src`,
      `d:${A}|src/api`,
      `f:${A}/src/api/b.ts`,
      `f:${A}/src/a.ts`,
      `f:${A}/README.md`,
      `g:${W}`,
      `f:${W}/z.ts`,
    ]);
    expect(items[3].parent).toBe(`d:${A}|src/api`);
    expect(items[0].parent).toBeNull();
  });

  it("skips the children of collapsed folders and groups", () => {
    const collapsed = new Set([`pending|${A}|src`, `pending|group|${W}`]);
    const items = visibleItems(groups, { mode: "tree", showGroups: true, tab: "pending", collapsed });
    expect(items.map((i) => i.id)).toEqual([`g:${A}`, `d:${A}|src`, `f:${A}/README.md`, `g:${W}`]);
    expect(items.find((i) => i.id === `d:${A}|src`)?.open).toBe(false);
  });

  it("is flat in list mode without groups", () => {
    const items = visibleItems(groups, { mode: "list", showGroups: false, tab: "pending", collapsed: none });
    expect(items.map((i) => i.kind)).toEqual(["file", "file", "file", "file"]);
    expect(items.map((i) => i.row!.rel)).toEqual(["README.md", "src/a.ts", "src/api/b.ts", "z.ts"]);
  });
});

describe("fileOrder", () => {
  it("is every file in display order, collapsed or not", () => {
    expect(fileOrder(groups, "tree").map((r) => r.rel)).toEqual(["src/api/b.ts", "src/a.ts", "README.md", "z.ts"]);
  });
});

describe("collapseKeysToReveal", () => {
  it("finds the collapsed group and ancestor folders hiding a row", () => {
    const collapsed = [`pending|group|${A}`, `pending|${A}|src`, `pending|${A}|src/api`, `pending|${A}|srcx`, `accepted|${A}|src`];
    expect(collapseKeysToReveal(row(A, "src/api/b.ts"), "pending", collapsed).sort()).toEqual(
      [`pending|${A}|src`, `pending|${A}|src/api`, `pending|group|${A}`].sort(),
    );
  });
});
