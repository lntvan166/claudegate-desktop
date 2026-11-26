// The workspace: the projects the user added to the rail (`workspace` in config.json). Every other
// listed project is a "known project", offered in the Add to workspace picker. Pending work in
// known projects is never swallowed: the rail's elsewhere line and the header count it.
import type { ProjectSummary } from "./api";
import { ownerOf, type Owner } from "./launch";

const norm = (p: string) => p.replace(/\\/g, "/").replace(/\/+$/, "");

type Listed = Pick<ProjectSummary, "root" | "nested">;

/** Where `p` sits in the workspace (its own root, or a nested root folded into it); -1 when outside. */
function indexIn(p: Listed, workspace: string[]): number {
  const mine = new Set([p.root, ...(p.nested ?? [])].map(norm));
  return workspace.findIndex((r) => mine.has(norm(r)));
}

/** Whether `p` is in the workspace, by its own root or a nested root folded into it. */
export function inWorkspace(p: Listed, workspace: string[] | undefined): boolean {
  return !workspace || indexIn(p, workspace) >= 0;
}

/**
 * The listed projects split into the workspace (in the user's order, a missing folder last) and
 * the known projects outside it (in the listed order). With no workspace yet (`undefined`, before
 * the config loads or when it cannot be read) every project is shown, as before the workspace.
 */
export function splitWorkspace<P extends Listed & { missing?: boolean }>(
  projects: P[],
  workspace: string[] | undefined,
): { workspace: P[]; known: P[] } {
  if (!workspace) return { workspace: [...projects], known: [] };
  const placed = projects.map((p) => ({ p, i: indexIn(p, workspace) }));
  const shown = placed
    .filter((x) => x.i >= 0)
    .sort((a, b) => Number(!!a.p.missing) - Number(!!b.p.missing) || a.i - b.i)
    .map((x) => x.p);
  return { workspace: shown, known: placed.filter((x) => x.i < 0).map((x) => x.p) };
}

/** A known project in the picker: `pending` is what has landed (rows still waiting do not count). */
export type PickerRow = { root: string; name: string; pending: number };

/** The rows that count as pending outside the workspace: settled no-ops are already gone; waiting ones are left out. */
const landed = (p: Pick<ProjectSummary, "pending"> & { ready?: number }) => p.ready ?? p.pending;

/** The picker's known projects matching `query` (name or path, any case): with pending first, then the rest, each by name. */
export function pickerRows(known: (Pick<ProjectSummary, "root" | "name" | "pending"> & { ready?: number })[], query: string): PickerRow[] {
  const q = query.trim().toLowerCase();
  const byName = (a: PickerRow, b: PickerRow) => a.name.localeCompare(b.name) || a.root.localeCompare(b.root);
  const rows = known
    .filter((p) => !q || p.name.toLowerCase().includes(q) || p.root.toLowerCase().includes(q))
    .map((p) => ({ root: p.root, name: p.name, pending: landed(p) }));
  return [...rows.filter((r) => r.pending > 0).sort(byName), ...rows.filter((r) => r.pending === 0).sort(byName)];
}

/** `/home/me/src/x` -> `~/src/x`, then cut from the left to about `max` characters at a folder boundary. */
export function shortPath(path: string, max = 32): string {
  const tidy = path.replace(/^\/(?:home|Users)\/[^/]+(?=\/|$)/, "~").replace(/^[A-Za-z]:\\Users\\[^\\]+/, "~");
  if (tidy.length <= max) return tidy;
  const parts = tidy.split(/[\\/]/);
  let tail = parts.pop() ?? tidy;
  while (parts.length > 0 && tail.length + parts[parts.length - 1].length + 3 <= max) tail = `${parts.pop()}/${tail}`;
  return `…/${tail}`;
}

/** Known projects with landed pending work, and how much. */
export function elsewhereOf(known: (Pick<ProjectSummary, "pending"> & { ready?: number })[]): { projects: number; pending: number } {
  const withPending = known.map(landed).filter((n) => n > 0);
  return { projects: withPending.length, pending: withPending.reduce((s, n) => s + n, 0) };
}

/** The rail's line above "+ Add to workspace": "2 other projects have 7 pending", or "" when none. */
export function elsewhereLine(e: { projects: number; pending: number }): string {
  if (e.pending <= 0) return "";
  return e.projects === 1 ? `1 other project has ${e.pending} pending` : `${e.projects} other projects have ${e.pending} pending`;
}

/** What the header adds after the workspace total: " · +7 elsewhere", or "". */
export function elsewhereSuffix(pending: number): string {
  return pending > 0 ? ` · +${pending} elsewhere` : "";
}

export type Ownership = { kind: "workspace"; root: string } | { kind: "known"; root: string } | { kind: "none" };

/**
 * Who owns a launch path (`claudegate PATH`): a workspace project (open it), a known project
 * outside the workspace (offer to add it), or nothing (offer to add the folder as a project).
 */
export function launchOwnership(path: string, owners: Owner[], inWs: (root: string) => boolean, canonical?: string | null): Ownership {
  const root = ownerOf(path, owners, canonical);
  if (!root) return { kind: "none" };
  return inWs(root) ? { kind: "workspace", root } : { kind: "known", root };
}
