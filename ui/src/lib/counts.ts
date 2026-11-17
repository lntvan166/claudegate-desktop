// One meaning per count. The header totals pending files across the shown projects; the project's
// title line leads with its own pending count; the tabs count rows; the bulk bar counts what its
// buttons would act on, and says "to review" when that is not the whole tab.
import { relTime } from "./time";

const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;
const norm = (p: string) => p.replace(/\\/g, "/").replace(/\/+$/, "");

/** The toolbar: "15 pending in 2 projects" over the shown (not hidden) projects. */
export function headerTotal(projects: { pending: number }[]): string {
  const withPending = projects.filter((p) => p.pending > 0);
  const n = withPending.reduce((s, p) => s + p.pending, 0);
  if (n === 0) return "Nothing pending";
  return `${n} pending in ${plural(withPending.length, "project", "projects")}`;
}

/** The bulk bar's count: the files (records) it acts on, or "N to review" when not the whole tab. */
export function bulkLabel(count: number, tabCount: number, tab: "pending" | "accepted" | "rejected"): string {
  if (tab === "pending" && count !== tabCount) return `${count} to review`;
  return tab === "pending" ? plural(count, "file", "files") : plural(count, "record", "records");
}

/**
 * The project's title line, short enough for two lines: "13 pending · 1 not written yet · oldest
 * 30m ago", with "in 3 worktrees" when the files span several. `detail` is the per-worktree
 * breakdown ("2 in shop, 1 in .wt/a"), for a tooltip; null for a single scope.
 */
export function projectLine(
  pending: { scope: string; waiting: boolean; capturedAt: string | null }[],
  scopes: { root: string; label: string }[],
  now: number = Date.now(),
): { text: string; detail: string | null } {
  const n = pending.length;
  if (n === 0) return { text: "Nothing pending", detail: null };
  const per = scopes
    .map((s) => ({ label: s.label, n: pending.filter((p) => norm(p.scope) === norm(s.root)).length }))
    .filter((s) => s.n > 0);
  const where = per.length > 1 ? ` in ${per.length} worktrees` : "";
  const parts = [`${n} pending${where}`];
  const w = pending.filter((p) => p.waiting).length;
  if (w > 0) parts.push(`${w} not written yet`);
  const oldest = pending
    .map((p) => p.capturedAt)
    .filter((t): t is string => !!t)
    .sort()[0];
  if (oldest) parts.push(`oldest ${relTime(oldest, now)}`);
  return { text: parts.join(" · "), detail: per.length > 1 ? per.map((s) => `${s.n} in ${s.label}`).join(", ") : null };
}
