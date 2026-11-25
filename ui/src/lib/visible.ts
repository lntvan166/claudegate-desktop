// The review tree's rows in display order. FileTree renders from the same functions, so keyboard
// navigation and what is on screen cannot disagree.
import type { DiffKind } from "./api";
import { groupKey, type Group, type Row } from "./rows";
import { buildTree, type TreeNode } from "./tree";

export type Mode = "tree" | "list";

export type Item = {
  /** Stable DOM/focus id: `g:<scope>`, `d:<scope>|<rel>`, `f:<scope>|<row key>`. */
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
/** A file row's id: its scope and key, as the same key can be a row in two scopes. */
export const fileId = (row: { scope: string; key: string }) => `f:${row.scope}|${row.key}`;
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
    const gk = groupKey(g);
    if (o.showGroups) {
      const collapseKey = groupCollapseKey(o.tab, gk);
      const open = !o.collapsed.has(collapseKey);
      const id = groupId(gk);
      out.push({ id, kind: "group", depth: 0, parent: null, collapseKey, open });
      if (open) walk(nodes, 1, gk, id);
    } else {
      walk(nodes, 0, gk, null);
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
  const g = row.group ?? row.scope;
  const group = groupCollapseKey(tab, g);
  const prefix = `${tab}|${g}|`;
  const out: string[] = [];
  for (const k of collapsed) {
    if (k === group) out.push(k);
    else if (k.startsWith(prefix) && row.rel.startsWith(`${k.slice(prefix.length)}/`)) out.push(k);
  }
  return out;
}

/**
 * The row a tab switch selects: the first file on screen, in display order, under the tab's own
 * collapse state (and the filter, which the caller has applied to `groups`). Null when no file
 * shows: an empty tab keeps its empty state, and a fully collapsed one selects nothing hidden.
 */
export function firstVisibleRow(groups: Group[], o: Opts): Row | null {
  return visibleItems(groups, o).find((i) => i.kind === "file")?.row ?? null;
}
