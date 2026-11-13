import { describe, expect, it } from "vitest";
import { buildTree, type TreeNode } from "./tree";

/** Strips a tree to names only, so expectations read like the sketch in the brief. */
function shape(nodes: TreeNode<unknown>[]): unknown[] {
  return nodes.map((n) => (n.kind === "folder" ? { [n.name]: shape(n.children ?? []) } : n.name));
}

describe("buildTree", () => {
  it("puts folders first, then files, nesting by path", () => {
    const tree = buildTree([{ rel: "src/a.ts" }, { rel: "src/api/x.ts" }, { rel: "README.md" }]);
    expect(shape(tree)).toEqual([{ src: [{ api: ["x.ts"] }, "a.ts"] }, "README.md"]);
  });

  it("compacts a single-child folder chain", () => {
    const tree = buildTree([{ rel: "a/b/c/d.ts" }]);
    expect(tree).toHaveLength(1);
    expect(tree[0].name).toBe("a/b/c");
    expect(tree[0].rel).toBe("a/b/c");
    expect(tree[0].kind).toBe("folder");
    expect(shape(tree[0].children!)).toEqual(["d.ts"]);
  });

  it("does not compact a folder whose only child is a file", () => {
    const tree = buildTree([{ rel: "src/api/x.ts" }, { rel: "src/b.ts" }]);
    expect(shape(tree)).toEqual([{ src: [{ api: ["x.ts"] }, "b.ts"] }]);
  });

  it("sorts each level case-insensitively", () => {
    const tree = buildTree([{ rel: "b.ts" }, { rel: "A.ts" }, { rel: "Zeta/x" }, { rel: "alpha/y" }]);
    expect(shape(tree)).toEqual([{ alpha: ["y"] }, { Zeta: ["x"] }, "A.ts", "b.ts"]);
  });

  it("keeps the item on its leaf and the rel of every node", () => {
    const item = { rel: "src/api/x.ts", id: 7 };
    const tree = buildTree([item, { rel: "src/y.ts", id: 8 }]);
    const api = tree[0].children![0];
    expect(api.rel).toBe("src/api");
    expect(api.children![0].item).toBe(item);
    expect(api.children![0].rel).toBe("src/api/x.ts");
  });

  it("tolerates absolute and backslashed rels", () => {
    const tree = buildTree([{ rel: "/elsewhere/f.ts" }, { rel: "win\\g.ts" }]);
    expect(shape(tree)).toEqual([{ elsewhere: ["f.ts"] }, { win: ["g.ts"] }]);
  });
});
