import { describe, expect, it } from "vitest";
import type { ProjectSummary, ProjectView } from "./api";
import { groupsFor, nodeKey, selectionAfterList, sortProjects } from "./rows";
import { buildTree } from "./tree";

const view: ProjectView = {
  root: "/ws/shop",
  missing: false,
  scopes: [
    { root: "/ws/shop", isWorktree: false, shapeUnknown: [], error: null },
    { root: "/ws/shop/.worktrees/feat", isWorktree: true, shapeUnknown: [], error: null },
  ],
  pending: [
    { path: "/ws/shop/src/a.ts", rel: "src/a.ts", scope: "/ws/shop", newFile: false, capturedAt: null, added: 3, removed: 1, waiting: false },
    {
      path: "/ws/shop/.worktrees/feat/src/b.ts",
      rel: ".worktrees/feat/src/b.ts",
      scope: "/ws/shop/.worktrees/feat",
      newFile: true,
      capturedAt: null,
      added: 9,
      removed: 0,
      waiting: true,
    },
  ],
  accepted: [
    { id: "t1::/ws/shop/x.md", path: "/ws/shop/x.md", rel: "x.md", scope: "/ws/shop", decidedAt: "t1", added: 1, removed: 0 },
  ],
  rejected: [
    { id: "t2::/ws/shop/y.md", path: "/ws/shop/y.md", rel: "y.md", scope: "/ws/shop", decidedAt: "t2", added: 0, removed: 2 },
  ],
  nextRecheckAt: null,
};

describe("groupsFor", () => {
  it("groups by scope, with rels relative to the scope root", () => {
    const groups = groupsFor(view, "pending");
    expect(groups.map((g) => g.label)).toEqual(["shop", ".worktrees/feat"]);
    expect(groups[1].rows[0].rel).toBe("src/b.ts");
    expect(groups[1].rows[0].newFile).toBe(true);
    expect(groups[0].rows[0].key).toBe("/ws/shop/src/a.ts");
  });

  it("carries a pending row's waiting flag; records never wait", () => {
    const [main, wt] = groupsFor(view, "pending");
    expect(main.rows[0].waiting).toBe(false);
    expect(wt.rows[0].waiting).toBe(true);
    expect(groupsFor(view, "accepted")[0].rows[0].waiting).toBe(false);
  });

  it("drops scopes with nothing on the tab", () => {
    expect(groupsFor(view, "accepted").map((g) => g.label)).toEqual(["shop"]);
  });

  it("keys accepted rows by id and rejected rows by path", () => {
    expect(groupsFor(view, "accepted")[0].rows[0].key).toBe("t1::/ws/shop/x.md");
    expect(groupsFor(view, "rejected")[0].rows[0].key).toBe("/ws/shop/y.md");
  });
});

describe("sortProjects", () => {
  it("keeps the roots order but sends missing folders last", () => {
    const p = (name: string, missing = false): ProjectSummary => ({ root: `/${name}`, name, missing, pending: 0, nextRecheckAt: null });
    const sorted = sortProjects([p("b"), p("gone", true), p("a")]);
    expect(sorted.map((x) => x.name)).toEqual(["b", "a", "gone"]);
  });
});

describe("duplicate accepted records", () => {
  it("keeps both rows for one path, with distinct node keys", () => {
    const twice: ProjectView = {
      ...view,
      accepted: [
        { id: "t3::/ws/shop/x.md", path: "/ws/shop/x.md", rel: "x.md", scope: "/ws/shop", decidedAt: "t3", added: 2, removed: 0 },
        { id: "t1::/ws/shop/x.md", path: "/ws/shop/x.md", rel: "x.md", scope: "/ws/shop", decidedAt: "t1", added: 1, removed: 0 },
      ],
    };
    const rows = groupsFor(twice, "accepted")[0].rows;
    const leaves = buildTree(rows);
    expect(leaves).toHaveLength(2);
    const keys = leaves.map(nodeKey);
    expect(new Set(keys).size).toBe(2);
    expect(keys).toEqual(expect.arrayContaining(["f:t3::/ws/shop/x.md", "f:t1::/ws/shop/x.md"]));
  });

  it("keys folders apart from files of the same rel", () => {
    expect(nodeKey({ name: "a", rel: "a", kind: "folder", children: [] })).toBe("d:a");
  });
});

describe("selectionAfterList", () => {
  const p = (root: string, pending = 0, missing = false): ProjectSummary => ({ root, name: root, missing, pending, nextRecheckAt: null });
  const list = [p("/a"), p("/b", 2), p("/c")];

  it("keeps a current selection that is still listed", () => {
    expect(selectionAfterList(list, "/c", "/a")).toEqual({ kind: "keep", root: "/c" });
  });

  it("selects the remembered project, else the first with pending, else the first", () => {
    expect(selectionAfterList(list, null, "/c")).toEqual({ kind: "select", root: "/c" });
    expect(selectionAfterList(list, null, "/gone")).toEqual({ kind: "select", root: "/b" });
    expect(selectionAfterList([p("/a"), p("/c")], "/gone", null)).toEqual({ kind: "select", root: "/a" });
  });

  it("clears only when there is nothing to select", () => {
    expect(selectionAfterList([], "/a", "/a")).toEqual({ kind: "clear" });
  });
});
