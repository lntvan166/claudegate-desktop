import { describe, expect, it } from "vitest";
import { addFolderFor, addLabel, knownNotice, launchNotice, findPendingRow, ownerOf, reposForLaunch } from "./launch";

const owners = [
  { root: "/w/app", scopes: ["/w/app", "/w/app-wt/feat"] },
  { root: "/w/app/packages/lib", scopes: ["/w/app/packages/lib"] },
  { root: "/w/other", scopes: ["/w/other"] },
];

describe("ownerOf", () => {
  it("picks the longest root prefix", () => {
    expect(ownerOf("/w/app/packages/lib/src/a.ts", owners)).toBe("/w/app/packages/lib");
    expect(ownerOf("/w/app/src/a.ts", owners)).toBe("/w/app");
  });
  it("matches the root itself, with or without a trailing slash", () => {
    expect(ownerOf("/w/app", owners)).toBe("/w/app");
    expect(ownerOf("/w/app/", owners)).toBe("/w/app");
    expect(ownerOf("/w/other///", owners)).toBe("/w/other");
  });
  it("does not match a sibling that shares a name prefix", () => {
    expect(ownerOf("/w/app2/a.ts", owners)).toBeNull();
    expect(ownerOf("/w/app-wt/a.ts", owners)).toBeNull();
  });
  it("a worktree scope belongs to its project", () => {
    expect(ownerOf("/w/app-wt/feat/src/a.ts", owners)).toBe("/w/app");
    expect(ownerOf("/w/app-wt/feat", owners)).toBe("/w/app");
  });
  it("is null when nothing owns it, and tolerates backslashes", () => {
    expect(ownerOf("/elsewhere/x", owners)).toBeNull();
    expect(ownerOf("C:\\w\\app\\a.ts", [{ root: "C:/w/app", scopes: [] }])).toBe("C:/w/app");
  });
});

describe("ownerOf, a nested registered root folded into its parent", () => {
  it("a path in the nested repo opens the parent project", () => {
    const o = [{ root: "/w/umbrella", scopes: ["/w/umbrella", "/w/umbrella/umbrella-orders"] }, { root: "/w/umbrellax", scopes: ["/w/umbrellax"] }];
    expect(ownerOf("/w/umbrella/umbrella-orders/a.go", o)).toBe("/w/umbrella");
    expect(ownerOf("/w/umbrellax/a.go", o)).toBe("/w/umbrellax");
  });
});

describe("ownerOf, nested and symlinked", () => {
  it("a worktree of B inside A's root belongs to B", () => {
    const o = [
      { root: "/w/a", scopes: ["/w/a"] },
      { root: "/elsewhere/b", scopes: ["/elsewhere/b", "/w/a/.wt/b-feat"] },
    ];
    expect(ownerOf("/w/a/.wt/b-feat/src/x.ts", o)).toBe("/elsewhere/b");
    expect(ownerOf("/w/a/src/x.ts", o)).toBe("/w/a");
  });
  it("tries the canonical path too", () => {
    expect(ownerOf("/link/x.ts", owners, "/w/other/x.ts")).toBe("/w/other");
    expect(ownerOf("/link/x.ts", owners, null)).toBeNull();
  });
});

describe("launch notice", () => {
  const file = { kind: "review" as const, path: "/w/n/a.ts", exists: true, isDir: false, canonical: null };
  it("says what is wrong and what Add would add", () => {
    expect(launchNotice(file)).toBe("/w/n/a.ts isn't a ClaudeGate project.");
    expect(launchNotice({ ...file, exists: false })).toBe("/w/n/a.ts doesn't exist.");
    expect(addLabel(file)).toBe("Add /w/n");
    expect(addLabel({ ...file, isDir: true })).toBe("Add project");
  });
  it("names the known project a path is in when it is outside the workspace", () => {
    expect(knownNotice({ ...file, path: "/w/umbrella/go.mod" }, "umbrella")).toBe("/w/umbrella/go.mod is in umbrella, which isn't in your workspace.");
  });
});

describe("findPendingRow", () => {
  const rows = [{ path: "/w/app/a.ts" }, { path: "/w/app-wt/feat/b.ts" }];
  it("finds a file's row, only on an exact path", () => {
    expect(findPendingRow("/w/app/a.ts", rows)).toBe(rows[0]);
    expect(findPendingRow("/w/app-wt/feat/b.ts", rows)).toBe(rows[1]);
    expect(findPendingRow("/w/app", rows)).toBeNull();
  });
  it("picks the most specific scope when two list the path", () => {
    const two = [{ path: "/w/umbrella/orders/a.go", scope: "/w/umbrella" }, { path: "/w/umbrella/orders/a.go", scope: "/w/umbrella/orders" }];
    expect(findPendingRow("/w/umbrella/orders/a.go", two)).toBe(two[1]);
  });
});

describe("addFolderFor", () => {
  it("is the folder itself, or the file's folder", () => {
    expect(addFolderFor({ kind: "review" as const, path: "/w/new/", exists: true, isDir: true, canonical: null })).toBe("/w/new");
    expect(addFolderFor({ kind: "review" as const, path: "/w/new/src/a.ts", exists: true, isDir: false, canonical: null })).toBe("/w/new/src");
  });
});

describe("reposForLaunch (claudegate log PATH)", () => {
  const co = (...p: string[]) => p.map((path, i) => ({ path, head: "", branch: null, isMain: i === 0 }));
  const repos = [
    { id: "/work/acme/acme-api/.git", checkouts: co("/work/acme/acme-api", "/work/acme-wt/api-feat") },
    { id: "/work/acme/acme-web/.git", checkouts: co("/work/acme/acme-web") },
    { id: "/work/acme/umbrella/.git", checkouts: co("/work/acme/umbrella") },
    { id: "/work/acme/umbrella/lib/.git", checkouts: co("/work/acme/umbrella/lib") },
    { id: "/work/other/.git", checkouts: co("/work/other") },
  ];
  it("ticks every repo whose checkout is at or under the path", () => {
    expect(reposForLaunch(repos, "/work/acme")).toEqual([
      "/work/acme/acme-api/.git",
      "/work/acme/acme-web/.git",
      "/work/acme/umbrella/.git",
      "/work/acme/umbrella/lib/.git",
    ]);
    expect(reposForLaunch(repos, "/work/acme/acme-web/")).toEqual(["/work/acme/acme-web/.git"]);
  });
  it("a worktree checkout under the path ticks its repo", () => {
    expect(reposForLaunch(repos, "/work/acme-wt")).toEqual(["/work/acme/acme-api/.git"]);
  });
  it("a path inside a repo ticks that repo, the deepest one", () => {
    expect(reposForLaunch(repos, "/work/acme/acme-api/src/deep")).toEqual(["/work/acme/acme-api/.git"]);
    expect(reposForLaunch(repos, "/work/acme/umbrella/lib/src")).toEqual(["/work/acme/umbrella/lib/.git"]);
    expect(reposForLaunch(repos, "/work/acme/umbrella/docs")).toEqual(["/work/acme/umbrella/.git"]);
  });
  it("matches the symlink-free form, and does not match a sibling prefix", () => {
    expect(reposForLaunch(repos, "/link/web", "/work/acme/acme-web")).toEqual(["/work/acme/acme-web/.git"]);
    expect(reposForLaunch(repos, "/work/acme/acme")).toEqual([]);
    expect(reposForLaunch(repos, "/elsewhere")).toEqual([]);
  });
});
