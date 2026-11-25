import type { DiffKind, ProjectSummary, ProjectView, ScopeView } from "./api";
import type { RejectOutcome } from "./decide";
import type { TreeNode } from "./tree";

/** One file row on a review tab, whatever tab it came from. */
export type Row = {
  /** The `key` get_diff expects: pending and rejected by path, accepted by record id. */
  key: string;
  path: string;
  /** Relative to the row's own scope root, so a worktree group reads like its own repo. */
  rel: string;
  scope: string;
  added: number;
  removed: number;
  newFile: boolean;
  decidedAt: string | null;
  /** A pending capture whose write has not landed yet. */
  waiting: boolean;
  /** Pending rows: what a reject does on disk. Null on the Accepted and Rejected tabs. */
  rejectOutcome: RejectOutcome | null;
  /** Pending rows: when the hook captured it (the age label). */
  capturedAt?: string | null;
  /** The Claude session behind it; null when the hook had none. */
  sessionId?: string | null;
  /** The key of the group it is listed under, when that is not its scope (grouped by session). */
  group?: string;
};

/** What identifies a row: its scope and its key. The same path can be a row in two scopes. */
export type RowRef = { scope: string; key: string };

/** Whether `a` and `b` are the same row (scope and key). */
export function sameRow(a: RowRef | null | undefined, b: RowRef | null | undefined): boolean {
  return !!a && !!b && a.key === b.key && norm(a.scope) === norm(b.scope);
}

/**
 * The row in `groups` that is the same row as `was` (scope and key), so a reload keeps the
 * selection on its fresh object; null when it is gone. Never a same-key row of another scope.
 */
export function restoreSelection(groups: Group[], was: RowRef | null): Row | null {
  if (!was) return null;
  return groups.flatMap((g) => g.rows).find((r) => sameRow(r, was)) ?? null;
}

/** `key` identifies the group when it is not simply its scope (one scope can hold several). */
export type Group = { scope: ScopeView; label: string; rows: Row[]; key?: string };

/** The group's identity in ids and collapse keys. */
export const groupKey = (g: Group) => g.key ?? g.scope.root;

const norm = (p: string) => p.replace(/\\/g, "/").replace(/\/+$/, "");

export function relTo(root: string, path: string): string {
  const r = norm(root);
  const p = norm(path);
  return p.startsWith(`${r}/`) ? p.slice(r.length + 1) : p;
}

export function basename(path: string): string {
  return norm(path).split("/").pop() || path;
}

/** What a scope is: the project's own root, a git worktree in it, or a registered repo nested in it. */
export type ScopeKind = "primary" | "worktree" | "repo";

export function scopeKind(scope: Pick<ScopeView, "isWorktree" | "nested">): ScopeKind {
  return scope.nested ? "repo" : scope.isWorktree ? "worktree" : "primary";
}

/** A scope other than the project's own root, labelled by its path inside the project. */
export const isSubScope = (scope: Pick<ScopeView, "isWorktree" | "nested">) => scopeKind(scope) !== "primary";

export function scopeLabel(view: ProjectView, scope: ScopeView): string {
  return isSubScope(scope) ? relTo(view.root, scope.root) : basename(view.root);
}

export function rowsOf(view: ProjectView, kind: DiffKind): Row[] {
  if (kind === "pending") {
    return view.pending.map((p) => ({
      key: p.path,
      path: p.path,
      rel: relTo(p.scope, p.path),
      scope: p.scope,
      added: p.added,
      removed: p.removed,
      newFile: p.newFile,
      decidedAt: null,
      waiting: p.waiting,
      rejectOutcome: p.rejectOutcome,
      capturedAt: p.capturedAt,
      sessionId: p.sessionId ?? null,
    }));
  }
  return view[kind].map((r) => ({
    key: kind === "accepted" ? r.id : r.path,
    path: r.path,
    rel: relTo(r.scope, r.path),
    scope: r.scope,
    added: r.added,
    removed: r.removed,
    newFile: false,
    decidedAt: r.decidedAt,
    waiting: false,
    rejectOutcome: null,
    sessionId: r.sessionId ?? null,
  }));
}

/** The tab's rows grouped by scope (primary first, then worktrees and nested repos), empty scopes dropped. */
export function groupsFor(view: ProjectView, kind: DiffKind): Group[] {
  const rows = rowsOf(view, kind);
  return view.scopes
    .map((scope) => ({
      scope,
      label: scopeLabel(view, scope),
      rows: rows.filter((r) => norm(r.scope) === norm(scope.root)),
    }))
    .filter((g) => g.rows.length > 0);
}

/** Projects in roots order, with folders that no longer exist moved to the end. */
export function sortProjects(list: ProjectSummary[]): ProjectSummary[] {
  return [...list.filter((p) => !p.missing), ...list.filter((p) => p.missing)];
}

/**
 * A unique `{#each}` key for a tree node. Files key by their row (the accepted log is append-only,
 * so one path can appear twice with different record ids); folders by their rel.
 */
export function nodeKey(n: TreeNode<Row>): string {
  return n.kind === "file" && n.item ? `f:${n.item.key}` : `d:${n.rel}`;
}

export type ListSelection = { kind: "keep"; root: string } | { kind: "select"; root: string } | { kind: "clear" };

/**
 * What to select after the project list loads: keep a selection still listed, else the remembered
 * project, the first with pending changes, or the first; clear only when the list is empty.
 */
export function selectionAfterList(
  projects: ProjectSummary[],
  selected: string | null,
  remembered: string | null,
): ListSelection {
  if (selected && projects.some((p) => p.root === selected)) return { kind: "keep", root: selected };
  const pick =
    projects.find((p) => p.root === remembered) ?? projects.find((p) => p.pending > 0) ?? projects[0];
  return pick ? { kind: "select", root: pick.root } : { kind: "clear" };
}

/** A project's rail label: the first letters of its first two words, or one letter for one word. */
export function initials(name: string): string {
  const words = name
    .replace(/([\p{Ll}\p{N}])(\p{Lu})/gu, "$1 $2")
    .split(/[^\p{L}\p{N}]+/u)
    .filter(Boolean);
  if (words.length === 0) return "?";
  return words
    .slice(0, 2)
    .map((w) => [...w][0].toLocaleUpperCase())
    .join("");
}

/**
 * The hidden roots that come back on their own: a pending capture newer than the moment they were
 * hidden. A root with no stamp (or one that does not parse) stays hidden: the app stamps it when
 * it first sees it, so only captures after that count.
 */
export function resurfacing(
  projects: Pick<ProjectSummary, "root" | "newestCaptureMs">[],
  hiddenRoots: string[] | undefined,
  hiddenAt: Record<string, string> | undefined,
): string[] {
  const out: string[] = [];
  for (const root of hiddenRoots ?? []) {
    const stamp = hiddenAt?.[root];
    const at = stamp ? Date.parse(stamp) : NaN;
    if (Number.isNaN(at)) continue;
    const p = projects.find((x) => norm(x.root) === norm(root));
    if (p?.newestCaptureMs != null && p.newestCaptureMs > at) out.push(root);
  }
  return out;
}

/**
 * The project list split by the config's `hiddenProjects`: hidden projects leave the rail and count
 * toward nothing; they wait in the "Hidden projects" disclosure. Both keep the list's order.
 */
export function splitHidden(
  projects: ProjectSummary[],
  hiddenRoots: string[] | undefined,
): { visible: ProjectSummary[]; hidden: ProjectSummary[] } {
  const hidden = new Set((hiddenRoots ?? []).map(norm));
  return {
    visible: projects.filter((p) => !hidden.has(norm(p.root))),
    hidden: projects.filter((p) => hidden.has(norm(p.root))),
  };
}
