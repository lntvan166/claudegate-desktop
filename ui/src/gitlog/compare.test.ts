import { describe, expect, it } from "vitest";
import { highlight, inReposText, noMatchText, pickerGroups, pushRecent, tabTitle, type BranchName, type Pair } from "./compare";

const pair = (left: string, right: string, leftRemote = false, rightRemote = false): Pair => ({ left, leftRemote, right, rightRemote });
const p1 = pair("origin/release-1.4", "origin/main", true, true);
const p2 = pair("origin/release-1.3", "origin/prod", true, true);

const names: BranchName[] = [
  { name: "origin/main", count: 68, remote: true },
  { name: "main", count: 68, remote: false },
  { name: "origin/release-1.4", count: 56, remote: true },
  { name: "feat/billing", count: 3, remote: false },
];

describe("compare helpers", () => {
  it("groups and filters branch names", () => {
    const fav = [{ name: "origin/main", remote: true }];
    const groups = pickerGroups(names, fav, [p1], "");
    expect(groups.map((g) => [g.title, g.items.length])).toEqual([["Recent pairs", 1], ["Favorites", 1], ["Local", 2], ["Remote", 2]]);
    const searched = pickerGroups(names, fav, [p1], "REL");
    expect(searched.map((g) => g.title)).toEqual(["Remote"]);
    expect(pickerGroups(names, [], [], "zzz")).toEqual([]);
    // By count, then name.
    const local = groups[2].items.map((i) => (i.kind === "name" ? i.name : ""));
    expect(local).toEqual(["main", "feat/billing"]);
  });

  it("keys a branch on its name and kind", () => {
    const both: BranchName[] = [
      { name: "origin/x", count: 1, remote: false },
      { name: "origin/x", count: 2, remote: true },
    ];
    // Only the remote one is a favourite, and only it is marked.
    const g = pickerGroups(both, [{ name: "origin/x", remote: true }], [], "");
    expect(g.map((x) => x.title)).toEqual(["Favorites", "Local", "Remote"]);
    expect(g[0].items).toEqual([{ kind: "name", name: "origin/x", remote: true, count: 2, favorite: true }]);
    expect(g[1].items).toEqual([{ kind: "name", name: "origin/x", remote: false, count: 1, favorite: false }]);
  });

  it("marks the matched part", () => {
    expect(highlight("origin/release-1.4", "REL")).toEqual({ pre: "origin/", hit: "rel", post: "ease-1.4" });
    expect(highlight("main", "")).toEqual({ pre: "main", hit: "", post: "" });
    expect(highlight("main", "zz")).toEqual({ pre: "main", hit: "", post: "" });
  });

  it("counts repositories", () => {
    expect(inReposText(1)).toBe("in 1 repository");
    expect(inReposText(3)).toBe("in 3 repositories");
  });

  it("says when no branch matches", () => {
    expect(noMatchText("zzz")).toBe('No branch named "zzz" in any repository.');
  });

  it("titles the pair without origin/", () => {
    expect(tabTitle(p1)).toBe("release-1.4 ↔ main");
    expect(tabTitle(null)).toBe("");
    expect(tabTitle(pair("origin/x", "main"))).toBe("origin/x ↔ main");
    expect(tabTitle(pair("main", ""))).toBe("");
  });

  it("keeps recent pairs newest first, once each, at most five", () => {
    let recent = pushRecent([], p1);
    recent = pushRecent(recent, p2);
    recent = pushRecent(recent, p1);
    expect(recent).toEqual([p1, p2]);
    // The same names with another kind is another pair.
    expect(pushRecent(recent, pair("origin/release-1.4", "origin/main", false, true))).toHaveLength(3);
    for (let i = 0; i < 9; i++) recent = pushRecent(recent, pair(`rel-${i}`, "main"));
    expect(recent).toHaveLength(5);
  });
});
