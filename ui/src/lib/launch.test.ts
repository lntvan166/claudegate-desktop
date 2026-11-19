import { describe, expect, it } from "vitest";
import { addFolderFor, addLabel, launchNotice, findPendingRow, ownerOf } from "./launch";

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
  const file = { path: "/w/n/a.ts", exists: true, isDir: false, canonical: null };
  it("says what is wrong and what Add would add", () => {
    expect(launchNotice(file)).toBe("/w/n/a.ts isn't a ClaudeGate project.");
    expect(launchNotice({ ...file, exists: false })).toBe("/w/n/a.ts doesn't exist.");
    expect(addLabel(file)).toBe("Add /w/n");
    expect(addLabel({ ...file, isDir: true })).toBe("Add project");
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
    expect(addFolderFor({ path: "/w/new/", exists: true, isDir: true, canonical: null })).toBe("/w/new");
    expect(addFolderFor({ path: "/w/new/src/a.ts", exists: true, isDir: false, canonical: null })).toBe("/w/new/src");
  });
});
