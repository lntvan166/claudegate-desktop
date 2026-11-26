import { describe, expect, it } from "vitest";
import { elsewhereLine, elsewhereOf, elsewhereSuffix, inWorkspace, launchOwnership, pickerRows, shortPath, splitWorkspace } from "./workspace";

const p = (root: string, pending = 0, extra: { ready?: number; nested?: string[]; missing?: boolean } = {}) => ({
  root,
  name: root.split("/").pop()!,
  missing: extra.missing ?? false,
  pending,
  ready: extra.ready ?? pending,
  nextRecheckAt: null,
  nested: extra.nested ?? [],
});

describe("splitWorkspace", () => {
  const list = [p("/w/shop-api", 3), p("/w/umbrella", 2, { nested: ["/w/umbrella/umbrella-orders"] }), p("/w/field-notes"), p("/w/tide-sim", 1)];

  it("lists only the workspace, in the user's order, and the rest as known projects", () => {
    const s = splitWorkspace(list, ["/w/tide-sim", "/w/shop-api"]);
    expect(s.workspace.map((x) => x.root)).toEqual(["/w/tide-sim", "/w/shop-api"]);
    expect(s.known.map((x) => x.root)).toEqual(["/w/umbrella", "/w/field-notes"]);
  });
  it("matches a root spelled with a trailing slash", () => {
    expect(splitWorkspace(list, ["/w/field-notes/"]).workspace.map((x) => x.root)).toEqual(["/w/field-notes"]);
  });
  it("a nested root in the workspace brings its parent's project, never one of its own", () => {
    const s = splitWorkspace(list, ["/w/umbrella/umbrella-orders"]);
    expect(s.workspace.map((x) => x.root)).toEqual(["/w/umbrella"]);
    expect(s.known.map((x) => x.root)).not.toContain("/w/umbrella/umbrella-orders");
  });
  it("ignores a workspace root nothing lists any more", () => {
    expect(splitWorkspace(list, ["/w/gone"]).workspace).toEqual([]);
  });
  it("keeps a missing folder last", () => {
    const l = [p("/w/a", 0, { missing: true }), p("/w/b")];
    expect(splitWorkspace(l, ["/w/a", "/w/b"]).workspace.map((x) => x.root)).toEqual(["/w/b", "/w/a"]);
  });
  it("before the config says (no workspace yet), shows every project", () => {
    expect(splitWorkspace(list, undefined).workspace).toHaveLength(4);
  });
  it("inWorkspace answers the same question for one root", () => {
    expect(inWorkspace(list[1], ["/w/umbrella/umbrella-orders"])).toBe(true);
    expect(inWorkspace(list[0], ["/w/umbrella"])).toBe(false);
  });
});

describe("pickerRows", () => {
  const known = [p("/w/zeta"), p("/home/me/src/shop-api", 2), p("/w/alpha"), p("/w/billing", 5), p("/w/notes", 3, { ready: 0 })];

  it("puts projects with pending first, then the rest, each by name", () => {
    expect(pickerRows(known, "").map((x) => x.name)).toEqual(["billing", "shop-api", "alpha", "notes", "zeta"]);
  });
  it("counts only what has landed: rows still waiting are not pending here", () => {
    expect(pickerRows(known, "").find((x) => x.name === "notes")?.pending).toBe(0);
  });
  it("filters by name or path, ignoring case", () => {
    expect(pickerRows(known, "SHOP").map((x) => x.name)).toEqual(["shop-api"]);
    expect(pickerRows(known, "/home/me").map((x) => x.name)).toEqual(["shop-api"]);
    expect(pickerRows(known, "  ").length).toBe(5);
    expect(pickerRows(known, "nothing-like-it")).toEqual([]);
  });
});

describe("shortPath", () => {
  it("tidies the home folder and shortens from the left", () => {
    expect(shortPath("/home/me/src/shop-api", 40)).toBe("~/src/shop-api");
    expect(shortPath("/home/me/src/clients/acme/services/shop-api", 20)).toBe("…/services/shop-api");
  });
  it("keeps at least the folder's own name", () => {
    expect(shortPath("/w/a-very-long-project-name-indeed", 10)).toBe("…/a-very-long-project-name-indeed");
  });
});

describe("the elsewhere line", () => {
  it("sums the known projects whose captures have landed", () => {
    const e = elsewhereOf([p("/w/a", 4), p("/w/b", 3), p("/w/c"), p("/w/d", 2, { ready: 0 })]);
    expect(e).toEqual({ projects: 2, pending: 7 });
    expect(elsewhereLine(e)).toBe("2 other projects have 7 pending");
  });
  it("reads in the singular, and is empty with nothing waiting", () => {
    expect(elsewhereLine({ projects: 1, pending: 1 })).toBe("1 other project has 1 pending");
    expect(elsewhereLine({ projects: 0, pending: 0 })).toBe("");
  });
  it("adds to the header total only when non-zero", () => {
    expect(elsewhereSuffix(7)).toBe(" · +7 elsewhere");
    expect(elsewhereSuffix(0)).toBe("");
  });
});

describe("launchOwnership with the workspace", () => {
  const owners = [
    { root: "/w/shop-api", scopes: ["/w/shop-api", "/w/shop-api-wt/feat"] },
    { root: "/w/umbrella", scopes: ["/w/umbrella", "/w/umbrella/umbrella-orders"] },
  ];
  const inWs = (root: string) => root === "/w/shop-api";
  it("a path in a workspace project opens it", () => {
    expect(launchOwnership("/w/shop-api-wt/feat/a.ts", owners, inWs)).toEqual({ kind: "workspace", root: "/w/shop-api" });
  });
  it("a path in a known project outside the workspace offers to add it", () => {
    expect(launchOwnership("/w/umbrella/umbrella-orders/go.mod", owners, inWs)).toEqual({ kind: "known", root: "/w/umbrella" });
  });
  it("a path nothing owns is not a project", () => {
    expect(launchOwnership("/w/elsewhere/x", owners, inWs)).toEqual({ kind: "none" });
  });
  it("tries the canonical path too", () => {
    expect(launchOwnership("/link/x", owners, inWs, "/w/umbrella/x")).toEqual({ kind: "known", root: "/w/umbrella" });
  });
});
