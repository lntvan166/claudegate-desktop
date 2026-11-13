import type { DiffKind, ProjectSummary, ProjectView, ScopeView } from "./api";

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
};

export type Group = { scope: ScopeView; label: string; rows: Row[] };

const norm = (p: string) => p.replace(/\\/g, "/").replace(/\/+$/, "");

export function relTo(root: string, path: string): string {
  const r = norm(root);
  const p = norm(path);
  return p.startsWith(`${r}/`) ? p.slice(r.length + 1) : p;
}

export function basename(path: string): string {
  return norm(path).split("/").pop() || path;
}

export function scopeLabel(view: ProjectView, scope: ScopeView): string {
  return scope.isWorktree ? relTo(view.root, scope.root) : basename(view.root);
}

function rowsOf(view: ProjectView, kind: DiffKind): Row[] {
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
  }));
}

/** The tab's rows grouped by scope (primary first, then worktrees), empty scopes dropped. */
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
