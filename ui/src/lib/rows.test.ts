import { describe, expect, it } from "vitest";
import type { ProjectSummary, ProjectView } from "./api";
import { groupsFor, initials, nodeKey, restoreSelection, scopeKind, scopeLabel, selectionAfterList, sortProjects } from "./rows";
import { buildTree } from "./tree";

const view: ProjectView = {
  root: "/ws/shop",
  missing: false,
  scopes: [
    { root: "/ws/shop", isWorktree: false, shapeUnknown: [], error: null },
    { root: "/ws/shop/.worktrees/feat", isWorktree: true, shapeUnknown: [], error: null },
  ],
  pending: [
    { path: "/ws/shop/src/a.ts", rel: "src/a.ts", scope: "/ws/shop", newFile: false, capturedAt: null, added: 3, removed: 1, waiting: false, rejectOutcome: "restore" },
    {
      path: "/ws/shop/.worktrees/feat/src/b.ts",
      rel: ".worktrees/feat/src/b.ts",
      scope: "/ws/shop/.worktrees/feat",
      newFile: true,
      capturedAt: null,
      added: 9,
      removed: 0,
      waiting: true,
      rejectOutcome: "delete",
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

describe("nested repo scopes", () => {
  const nested: ProjectView = {
    ...view,
    root: "/ws/umbrella",
    scopes: [
      { root: "/ws/umbrella", isWorktree: false, shapeUnknown: [], error: null },
      { root: "/ws/umbrella/umbrella-orders", isWorktree: false, nested: true, shapeUnknown: [], error: null },
    ],
    pending: [
      { ...view.pending[0], path: "/ws/umbrella/README.md", rel: "README.md", scope: "/ws/umbrella" },
      { ...view.pending[0], path: "/ws/umbrella/umbrella-orders/a.go", rel: "umbrella-orders/a.go", scope: "/ws/umbrella/umbrella-orders" },
    ],
  };
  it("labels the scope by its path in the parent and calls it a repo, not a worktree", () => {
    expect(scopeLabel(nested, nested.scopes[1])).toBe("umbrella-orders");
    expect(scopeKind(nested.scopes[1])).toBe("repo");
    expect(scopeKind(nested.scopes[0])).toBe("primary");
    expect(scopeKind(view.scopes[1])).toBe("worktree");
  });
  it("groups its rows apart, relative to the nested root", () => {
    const g = groupsFor(nested, "pending");
    expect(g.map((x) => x.label)).toEqual(["umbrella", "umbrella-orders"]);
    expect(g[1].rows[0].rel).toBe("a.go");
  });
});

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
    expect(keys).toEqual(expect.arrayContaining(["f:/ws/shop|t3::/ws/shop/x.md", "f:/ws/shop|t1::/ws/shop/x.md"]));
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

describe("initials", () => {
  it("takes the first letters of the first two words of a project name", () => {
    expect(initials("shop-api")).toBe("SA");
    expect(initials("herdr-pokemon")).toBe("HP");
    expect(initials("legacy_billing v2")).toBe("LB");
    expect(initials("claudeGate")).toBe("CG");
    expect(initials("my.site.io")).toBe("MS");
  });

  it("is one letter for a one-word name, and never empty", () => {
    expect(initials("bifrost")).toBe("B");
    expect(initials("-x-")).toBe("X");
    expect(initials("")).toBe("?");
    expect(initials("ổn-định")).toBe("ỔĐ");
  });
});

describe("restoreSelection", () => {
  it("finds the same row by scope and key, never the same key in another scope", () => {
    const groups = groupsFor(
      {
        ...view,
        root: "/ws/umbrella",
        scopes: [
          { root: "/ws/umbrella", isWorktree: false, shapeUnknown: [], error: null },
          { root: "/ws/umbrella/orders", isWorktree: false, nested: true, shapeUnknown: [], error: null },
        ],
        accepted: [],
        rejected: [
          { id: "1", path: "/ws/umbrella/orders/a.go", rel: "", scope: "/ws/umbrella", decidedAt: "t1", added: 1, removed: 0 },
          { id: "2", path: "/ws/umbrella/orders/a.go", rel: "", scope: "/ws/umbrella/orders", decidedAt: "t2", added: 1, removed: 0 },
        ],
      },
      "rejected",
    );
    const got = restoreSelection(groups, { scope: "/ws/umbrella/orders", key: "/ws/umbrella/orders/a.go" });
    expect(got?.scope).toBe("/ws/umbrella/orders");
    expect(got?.decidedAt).toBe("t2");
    expect(restoreSelection(groups, { scope: "/ws/umbrella/other", key: "/ws/umbrella/orders/a.go" })).toBeNull();
    expect(restoreSelection(groups, null)).toBeNull();
  });
});
