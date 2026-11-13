/** A row in the review tree. Folders carry `children`, files carry the `item` they came from. */
export type TreeNode<T = unknown> = {
  name: string;
  rel: string;
  kind: "folder" | "file";
  children?: TreeNode<T>[];
  item?: T;
};

const byName = (a: { name: string }, b: { name: string }) => {
  const x = a.name.toLowerCase();
  const y = b.name.toLowerCase();
  return x < y ? -1 : x > y ? 1 : a.name < b.name ? -1 : a.name > b.name ? 1 : 0;
};

function sortLevel<T>(nodes: TreeNode<T>[]): TreeNode<T>[] {
  const folders = nodes.filter((n) => n.kind === "folder").sort(byName);
  const files = nodes.filter((n) => n.kind === "file").sort(byName);
  for (const f of folders) {
    f.children = sortLevel(f.children ?? []);
  }
  return [...folders, ...files];
}

/** Merges `a` -> `b` -> ... while a folder's only child is itself a folder (`src/api`). */
function compact<T>(nodes: TreeNode<T>[]): TreeNode<T>[] {
  return nodes.map((n) => {
    if (n.kind !== "folder") return n;
    let cur = n;
    while (cur.children?.length === 1 && cur.children[0].kind === "folder") {
      const only = cur.children[0];
      cur = { ...only, name: `${cur.name}/${only.name}` };
    }
    return { ...cur, children: compact(cur.children ?? []) };
  });
}

/**
 * Builds a folder tree from slash-separated relative paths: folders first, then files, each
 * alphabetical ignoring case; single-child folder chains are compacted into one row.
 */
export function buildTree<T extends { rel: string }>(items: T[]): TreeNode<T>[] {
  const root: TreeNode<T>[] = [];
  const folders = new Map<string, TreeNode<T>>();

  for (const item of items) {
    const parts = item.rel.replace(/\\/g, "/").split("/").filter((p) => p !== "");
    if (parts.length === 0) continue;
    let level = root;
    let prefix = "";
    for (const dir of parts.slice(0, -1)) {
      prefix = prefix ? `${prefix}/${dir}` : dir;
      let folder = folders.get(prefix);
      if (!folder) {
        folder = { name: dir, rel: prefix, kind: "folder", children: [] };
        folders.set(prefix, folder);
        level.push(folder);
      }
      level = folder.children!;
    }
    const name = parts[parts.length - 1];
    level.push({ name, rel: prefix ? `${prefix}/${name}` : name, kind: "file", item });
  }

  return compact(sortLevel(root));
}
