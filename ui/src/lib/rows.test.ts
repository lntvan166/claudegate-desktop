import { describe, expect, it } from "vitest";
import type { ProjectSummary, ProjectView } from "./api";
import { groupsFor, initials, nodeKey, restoreSelection, resurfacing, scopeKind, scopeLabel, selectionAfterList, sortProjects, splitHidden } from "./rows";
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

describe("splitHidden (UX wave 7)", () => {
  const p = (root: string, pending = 1): ProjectSummary => ({ root, name: root.split("/").pop()!, missing: false, pending, nextRecheckAt: null });
  const list = [p("/w/a"), p("/w/b", 4), p("/w/c", 0)];

  it("moves hidden roots out of the list, keeping the order of both", () => {
    const s = splitHidden(list, ["/w/c", "/w/a"]);
    expect(s.visible.map((x) => x.root)).toEqual(["/w/b"]);
    expect(s.hidden.map((x) => x.root)).toEqual(["/w/a", "/w/c"]);
  });

  it("ignores hidden roots that are not listed, and hides nothing by default", () => {
    expect(splitHidden(list, ["/gone"]).hidden).toEqual([]);
    expect(splitHidden(list, undefined).visible).toHaveLength(3);
  });

  it("matches a root written with a trailing slash", () => {
    expect(splitHidden(list, ["/w/b/"]).hidden.map((x) => x.root)).toEqual(["/w/b"]);
  });
});

describe("resurfacing", () => {
  const at = "2026-11-20T10:00:00.000Z";
  const t = Date.parse(at);
  const p = (root: string, newestCaptureMs: number | null) => ({ root, newestCaptureMs });
  it("brings back a hidden project with a capture newer than its hiding", () => {
    expect(resurfacing([p("/w/a", t + 2001), p("/w/b", t - 1)], ["/w/a", "/w/b"], { "/w/a": at, "/w/b": at })).toEqual(["/w/a"]);
  });
  it("needs the capture to be more than 2 s newer than the hide", () => {
    expect(resurfacing([p("/w/a", t + 2000)], ["/w/a"], { "/w/a": at })).toEqual([]);
    expect(resurfacing([p("/w/a", t + 1)], ["/w/a"], { "/w/a": at })).toEqual([]);
  });
  it("stays hidden when its only new capture is still waiting (the summary leaves it out: null)", () => {
    expect(resurfacing([p("/w/a", null)], ["/w/a"], { "/w/a": at })).toEqual([]);
  });
  it("keeps one hidden when the capture is not newer, or there is none", () => {
    expect(resurfacing([p("/w/a", t), p("/w/b", null)], ["/w/a", "/w/b"], { "/w/a": at, "/w/b": at })).toEqual([]);
  });
  it("never resurfaces a root without a stamp or with an unreadable one", () => {
    expect(resurfacing([p("/w/a", t + 5000)], ["/w/a"], {})).toEqual([]);
    expect(resurfacing([p("/w/a", t + 5000)], ["/w/a"], { "/w/a": "yesterday" })).toEqual([]);
    expect(resurfacing([p("/w/a", t + 5000)], ["/w/a"], undefined)).toEqual([]);
  });
  it("ignores shown projects and matches roots across trailing slashes", () => {
    expect(resurfacing([p("/w/a", t + 5000)], [], { "/w/a": at })).toEqual([]);
    expect(resurfacing([p("/w/a", t + 5000)], ["/w/a/"], { "/w/a/": at })).toEqual(["/w/a/"]);
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
