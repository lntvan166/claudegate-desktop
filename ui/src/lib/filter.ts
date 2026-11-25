// Free-text filter for the Pending tab. Port of the extension's panelFilter.ts: the tab is a
// worklist, and a filter that cleared itself on every refresh would be useless exactly when used.
import type { ProjectView } from "./api";
import { isSubScope, relTo, sameRow, scopeLabel, type Group, type Row, type RowRef } from "./rows";

/** Raw input to a filter. Empty or whitespace means "no filter" (null) everywhere downstream. */
export function normalizeFilter(raw: string | undefined | null): string | null {
  const trimmed = (raw ?? "").trim();
  return trimmed === "" ? null : trimmed;
}

/**
 * Case-insensitive substring over the whole path, so one box narrows by file, directory or
 * worktree. Separators are normalised: nobody types the native one on purpose. A null filter
 * matches everything.
 */
export function matchesFilter(filePath: string, filter: string | null): boolean {
  if (filter === null) return true;
  const norm = (s: string) => s.replace(/\\/g, "/").toLowerCase();
  return norm(filePath).includes(norm(filter));
}

/**
 * The count label, e.g. `auth — 4 of 16`; undefined when nothing is filtered. The total is what
 * stops a filtered list from reading as a finished review.
 */
export function filterLabel(filter: string | null, shown: number, total: number): string | undefined {
  if (filter === null) return undefined;
  return `${filter} — ${shown} of ${total}`;
}

/**
 * What a row is matched on: its path relative to the project root. In a worktree that is the
 * worktree's label then the path inside it, so typing a worktree name finds its rows.
 */
export function pendingFilterPath(row: Row, view: Pick<ProjectView, "root" | "scopes">): string {
  const scope = view.scopes.find((s) => s.root.replace(/\/+$/, "") === row.scope.replace(/\/+$/, ""));
  if (scope && isSubScope(scope)) return `${scopeLabel(view as ProjectView, scope)}/${row.rel}`;
  return scope ? row.rel : relTo(view.root, row.path);
}

/** The groups with only the matching rows; groups left empty are dropped. */
export function filterGroups(groups: Group[], view: Pick<ProjectView, "root" | "scopes">, filter: string | null): Group[] {
  if (filter === null) return groups;
  return groups
    .map((g) => ({ ...g, rows: g.rows.filter((r) => matchesFilter(pendingFilterPath(r, view), filter)) }))
    .filter((g) => g.rows.length > 0);
}

/** The question before Accept / Reject on the matching rows (the extension's wording). */
export function matchPrompt(
  action: "accept" | "reject",
  count: number,
  filter: string,
): { text: string; go: string; danger: boolean } {
  if (action === "accept") {
    return {
      text: `Accept ${count} file(s) matching "${filter}"? Only files matching the filter are accepted. Anything the filter hides stays pending.`,
      go: `Accept ${count}`,
      danger: false,
    };
  }
  return {
    text: `Reject ${count} file(s) matching "${filter}"? This restores their original content on disk. Files the filter hides are untouched.`,
    go: `Reject ${count}`,
    danger: true,
  };
}

/** Whether the row `key` is among the rows the filter shows (a hidden row must not stay selected). */
export function stillShown(row: RowRef, shown: Group[]): boolean {
  return shown.some((g) => g.rows.some((r) => sameRow(r, row)));
}

/** The bulk keys a filtered Accept/Reject may act on: only those the filter shows, nothing else. */
export function filterBulk(keys: string[], shownPaths: Set<string>): string[] {
  return keys.filter((k) => shownPaths.has(k));
}
