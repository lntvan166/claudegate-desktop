// The Repositories pane's pure helpers: fuzzy search, ticks, profiles and the chip palette.
// Repository ids are git common dirs (`RepoView.id`, src-tauri/src/gitlog.rs).

/** gitlog.rs `CheckoutView`: the main checkout or one worktree of a repository. */
export type Checkout = { path: string; head: string; branch: string | null; isMain: boolean };

/** gitlog.rs `RepoView`. `color` indexes the 12-colour palette; ahead/behind are null without an upstream. */
export type RepoView = {
  id: string;
  name: string;
  color: number;
  checkouts: Checkout[];
  ahead: number | null;
  behind: number | null;
};

/** config.rs `GitLogConfig`: `gitLog` in ~/.claudegate-desktop/config.json, as the backend read it. */
export type GitLogConfig = {
  /** null: every repository is ticked. */
  ticked: string[] | null;
  profiles: Record<string, string[]>;
  lastFilter: Record<string, unknown> | null;
  scanDepth: number;
  excludeRepos: string[];
  panes: Record<string, number>;
};

export const DEFAULT_GITLOG_CONFIG: GitLogConfig = {
  ticked: null,
  profiles: {},
  lastFilter: null,
  scanDepth: 2,
  excludeRepos: [],
  panes: {},
};

/** The deepest discovery walk the backend accepts (config.rs `GITLOG_SCAN_DEPTH_MAX`). */
export const SCAN_DEPTH_MAX = 8;

/** Chip colours: `--repo-0` … `--repo-11` in theme.css, one set each for Dracula and Alucard. */
export const PALETTE_LEN = 12;

export function colorVar(i: number): string {
  const n = ((Math.trunc(i) % PALETTE_LEN) + PALETTE_LEN) % PALETTE_LEN;
  return `var(--repo-${n})`;
}

/** The main checkout's folder, else the first checkout's, else the common dir itself. */
export function repoPath(r: RepoView): string {
  return (r.checkouts.find((c) => c.isMain) ?? r.checkouts[0])?.path ?? r.id;
}

/** The checkouts besides the main one (linked worktrees). */
export function worktreesOf(r: RepoView): Checkout[] {
  return r.checkouts.filter((c) => !c.isMain);
}

const BOUNDARY = /[-_./\s]/;

/**
 * A subsequence match of `query` in `text`, case-insensitive: null when some character is
 * missing, else a score where consecutive characters and word starts count more and gaps less.
 * An empty query scores 0 (everything matches).
 */
export function fuzzyScore(query: string, text: string): number | null {
  const q = query.trim().toLowerCase();
  if (!q) return 0;
  const t = text.toLowerCase();
  let score = 0;
  let at = -1;
  for (const ch of q) {
    if (ch === " ") continue;
    const i = t.indexOf(ch, at + 1);
    if (i < 0) return null;
    score += 1;
    if (i === at + 1 && at >= 0) score += 3;
    if (i === 0 || BOUNDARY.test(t[i - 1])) score += 2;
    if (at >= 0) score -= Math.min(i - at - 1, 10) * 0.1;
    at = i;
  }
  // A shorter name with the same match is the closer one.
  return score - t.length * 0.01;
}

/** The repos whose name matches `query`, best first; with no query, all of them in their order. */
export function fuzzyFilter<T extends { name: string }>(repos: T[], query: string): T[] {
  if (!query.trim()) return repos.slice();
  return repos
    .map((r, i) => ({ r, i, s: fuzzyScore(query, r.name) }))
    .filter((x): x is { r: T; i: number; s: number } => x.s !== null)
    .sort((a, b) => b.s - a.s || a.i - b.i)
    .map((x) => x.r);
}

export function isTicked(ticked: string[] | null, id: string): boolean {
  return ticked === null || ticked.includes(id);
}

/** The ticked repos that exist, in the list's order. */
export function tickedIds(ticked: string[] | null, repos: RepoView[]): string[] {
  return repos.filter((r) => isTicked(ticked, r.id)).map((r) => r.id);
}

export function tickedCount(ticked: string[] | null, repos: RepoView[]): number {
  return ticked === null ? repos.length : repos.reduce((n, r) => n + (ticked.includes(r.id) ? 1 : 0), 0);
}

/**
 * The state after applying a profile: exactly the profile's repos ticked, everything else in
 * `state` (the filters) untouched. Ids no longer discovered are skipped and counted.
 */
export function applyProfile<S extends { ticked: string[] | null }>(
  state: S,
  profile: string[],
  repos: RepoView[],
): { state: S; missing: number } {
  const known = new Set(repos.map((r) => r.id));
  const ticked = [...new Set(profile)].filter((id) => known.has(id));
  return { state: { ...state, ticked }, missing: new Set(profile).size - ticked.length };
}

export function profileNames(profiles: Record<string, string[]>): string[] {
  return Object.keys(profiles).sort((a, b) => a.localeCompare(b, undefined, { sensitivity: "base" }));
}

/** "↓2 ↑1": behind then ahead of the upstream, as of the last fetch; empty when both are 0 or unknown. */
export function aheadBehind(r: Pick<RepoView, "ahead" | "behind">): string {
  const out: string[] = [];
  if (r.behind) out.push(`↓${r.behind}`);
  if (r.ahead) out.push(`↑${r.ahead}`);
  return out.join(" ");
}

/** The ids from `anchor` to `target` in `order`, inclusive; just `target` without a usable anchor. */
export function rangeOf(order: string[], anchor: string | null, target: string): string[] {
  const a = anchor === null ? -1 : order.indexOf(anchor);
  const b = order.indexOf(target);
  if (a < 0 || b < 0) return [target];
  return order.slice(Math.min(a, b), Math.max(a, b) + 1);
}

// ---- pane widths -------------------------------------------------------------------------------

export type GitLogPanes = { repos: number; changes: number };
export const PANE_DEFAULTS: Readonly<GitLogPanes> = { repos: 240, changes: 320 };
export const PANE_MIN = { repos: 180, changes: 220, diff: 360 } as const;
/** The collapsed Repositories column (the review's projects rail is as wide). */
export const REPOS_RAIL = 44;
/** The sheet's right inset and 1px borders: neither pane's. */
const PANE_CHROME = 10;

/** The stored widths (`gitLog.panes`), each falling back to its default. */
export function panesFrom(stored: Record<string, number>): GitLogPanes {
  const ok = (n: unknown): n is number => typeof n === "number" && Number.isFinite(n) && n > 0;
  return { repos: ok(stored.repos) ? stored.repos : PANE_DEFAULTS.repos, changes: ok(stored.changes) ? stored.changes : PANE_DEFAULTS.changes };
}

/**
 * The widths that fit a window `total` px wide: each pane at least its minimum and the diff left
 * with its own. Too narrow, the Changes column gives way first, then the repositories. Collapsed,
 * the repositories take the rail's width and their stored width is kept for expanding.
 */
export function fitPanes(w: GitLogPanes, total: number, collapsed: boolean): GitLogPanes {
  let repos = collapsed ? REPOS_RAIL : Math.max(PANE_MIN.repos, Math.round(w.repos));
  let changes = Math.max(PANE_MIN.changes, Math.round(w.changes));
  let over = repos + changes - (total - PANE_CHROME - PANE_MIN.diff);
  if (over > 0) {
    const c = Math.min(over, changes - PANE_MIN.changes);
    changes -= c;
    over -= c;
  }
  if (over > 0 && !collapsed) repos -= Math.min(over, repos - PANE_MIN.repos);
  return { repos, changes };
}

/** The largest width `pane` can take next to the other as it is. */
export function paneMax(w: GitLogPanes, pane: keyof GitLogPanes, total: number): number {
  const other = pane === "repos" ? w.changes : w.repos;
  return Math.max(PANE_MIN[pane], total - PANE_CHROME - PANE_MIN.diff - other);
}

/** Whether a profile of that (trimmed) name exists. Own keys only: "constructor" is not a profile. */
export function hasProfile(profiles: Record<string, string[]>, name: string): boolean {
  return Object.hasOwn(profiles, name.trim());
}
