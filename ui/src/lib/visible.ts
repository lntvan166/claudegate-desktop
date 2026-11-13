// The review tree's rows in display order. FileTree renders from the same functions, so keyboard
// navigation and what is on screen cannot disagree.
import type { DiffKind } from "./api";
import type { Group, Row } from "./rows";
import { buildTree, type TreeNode } from "./tree";

export type Mode = "tree" | "list";

export type Item = {
  /** Stable DOM/focus id: `g:<scope>`, `d:<scope>|<rel>`, `f:<row key>`. */
  id: string;
  kind: "group" | "folder" | "file";
  depth: number;
  parent: string | null;
  /** For groups and folders: the key in the collapsed set. */
  collapseKey?: string;
  open?: boolean;
  row?: Row;
};

export const groupId = (scope: string) => `g:${scope}`;
export const folderId = (scope: string, rel: string) => `d:${scope}|${rel}`;
export const fileId = (row: Row) => `f:${row.key}`;
export const groupCollapseKey = (tab: DiffKind, scope: string) => `${tab}|group|${scope}`;
export const folderCollapseKey = (tab: DiffKind, scope: string, rel: string) => `${tab}|${scope}|${rel}`;

function flat(rows: Row[]): TreeNode<Row>[] {
  return [...rows]
    .sort((a, b) => a.rel.toLowerCase().localeCompare(b.rel.toLowerCase()))
    .map((r) => ({ name: r.rel.split("/").pop() ?? r.rel, rel: r.rel, kind: "file", item: r }));
}

/** One group's nodes in the given layout. */
export function nodesFor(rows: Row[], mode: Mode): TreeNode<Row>[] {
  return mode === "tree" ? buildTree(rows) : flat(rows);
}

type Opts = { mode: Mode; showGroups: boolean; tab: DiffKind; collapsed: { has(key: string): boolean } };

export function visibleItems(groups: Group[], o: Opts): Item[] {
  const out: Item[] = [];
  const walk = (nodes: TreeNode<Row>[], depth: number, scope: string, parent: string | null) => {
    for (const n of nodes) {
      if (n.kind === "folder") {
        const collapseKey = folderCollapseKey(o.tab, scope, n.rel);
        const open = !o.collapsed.has(collapseKey);
        const id = folderId(scope, n.rel);
        out.push({ id, kind: "folder", depth, parent, collapseKey, open });
        if (open) walk(n.children ?? [], depth + 1, scope, id);
      } else if (n.item) {
        out.push({ id: fileId(n.item), kind: "file", depth, parent, row: n.item });
      }
    }
  };
  for (const g of groups) {
    const nodes = nodesFor(g.rows, o.mode);
    if (o.showGroups) {
      const collapseKey = groupCollapseKey(o.tab, g.scope.root);
      const open = !o.collapsed.has(collapseKey);
      const id = groupId(g.scope.root);
      out.push({ id, kind: "group", depth: 0, parent: null, collapseKey, open });
      if (open) walk(nodes, 1, g.scope.root, id);
    } else {
      walk(nodes, 0, g.scope.root, null);
    }
  }
  return out;
}

/** Every file across all groups in display order, ignoring collapse (for n / p). */
export function fileOrder(groups: Group[], mode: Mode): Row[] {
  return visibleItems(groups, { mode, showGroups: true, tab: "pending", collapsed: { has: () => false } })
    .filter((i) => i.kind === "file")
    .map((i) => i.row!);
}

/** Collapsed keys on `tab` that hide `row`: its group and any folder (compacted or not) above it. */
export function collapseKeysToReveal(row: Row, tab: DiffKind, collapsed: Iterable<string>): string[] {
  const group = groupCollapseKey(tab, row.scope);
  const prefix = `${tab}|${row.scope}|`;
  const out: string[] = [];
  for (const k of collapsed) {
    if (k === group) out.push(k);
    else if (k.startsWith(prefix) && row.rel.startsWith(`${k.slice(prefix.length)}/`)) out.push(k);
  }
  return out;
}
