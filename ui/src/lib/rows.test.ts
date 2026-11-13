import { describe, expect, it } from "vitest";
import type { ProjectSummary, ProjectView } from "./api";
import { groupsFor, sortProjects } from "./rows";

const view: ProjectView = {
  root: "/ws/shop",
  missing: false,
  scopes: [
    { root: "/ws/shop", isWorktree: false, shapeUnknown: [], error: null },
    { root: "/ws/shop/.worktrees/feat", isWorktree: true, shapeUnknown: [], error: null },
  ],
  pending: [
    { path: "/ws/shop/src/a.ts", rel: "src/a.ts", scope: "/ws/shop", newFile: false, capturedAt: null, added: 3, removed: 1 },
    {
      path: "/ws/shop/.worktrees/feat/src/b.ts",
      rel: ".worktrees/feat/src/b.ts",
      scope: "/ws/shop/.worktrees/feat",
      newFile: true,
      capturedAt: null,
      added: 9,
      removed: 0,
    },
  ],
  accepted: [
    { id: "t1::/ws/shop/x.md", path: "/ws/shop/x.md", rel: "x.md", scope: "/ws/shop", decidedAt: "t1", added: 1, removed: 0 },
  ],
  rejected: [
    { id: "t2::/ws/shop/y.md", path: "/ws/shop/y.md", rel: "y.md", scope: "/ws/shop", decidedAt: "t2", added: 0, removed: 2 },
  ],
};

describe("groupsFor", () => {
  it("groups by scope, with rels relative to the scope root", () => {
    const groups = groupsFor(view, "pending");
    expect(groups.map((g) => g.label)).toEqual(["shop", ".worktrees/feat"]);
    expect(groups[1].rows[0].rel).toBe("src/b.ts");
    expect(groups[1].rows[0].newFile).toBe(true);
    expect(groups[0].rows[0].key).toBe("/ws/shop/src/a.ts");
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
    const p = (name: string, missing = false): ProjectSummary => ({ root: `/${name}`, name, missing, pending: 0 });
    const sorted = sortProjects([p("b"), p("gone", true), p("a")]);
    expect(sorted.map((x) => x.name)).toEqual(["b", "a", "gone"]);
  });
});
