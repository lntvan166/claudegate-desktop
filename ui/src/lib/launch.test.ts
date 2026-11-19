import { describe, expect, it } from "vitest";
import { addFolderFor, findPendingRow, ownerOf } from "./launch";

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

describe("findPendingRow", () => {
  const rows = [{ path: "/w/app/a.ts" }, { path: "/w/app-wt/feat/b.ts" }];
  it("finds a file's row, only on an exact path", () => {
    expect(findPendingRow("/w/app/a.ts", rows)).toBe(rows[0]);
    expect(findPendingRow("/w/app-wt/feat/b.ts", rows)).toBe(rows[1]);
    expect(findPendingRow("/w/app", rows)).toBeNull();
  });
});

describe("addFolderFor", () => {
  it("is the folder itself, or the file's folder", () => {
    expect(addFolderFor({ path: "/w/new/", isDir: true })).toBe("/w/new");
    expect(addFolderFor({ path: "/w/new/src/a.ts", isDir: false })).toBe("/w/new/src");
  });
});
