// The capture audit as the UI shows it. Mirrors core/src/audit.rs (`AuditView`, serde camelCase).
// Pure and tested: every word the user reads and the refresh policy live here.
import { relTime } from "./time";

export type AuditItem = {
  path: string;
  /** When the audit line that last saw this path was written, epoch ms. */
  atMs: number;
  /** The first words of the Bash command that changed it. */
  commandHead: string;
  /** The innermost registered root the audit line belongs to (one of the project's scopes). */
  root: string;
};

export type AuditStats = {
  calls: number;
  captured: number;
  missed: number;
  uncertain: number;
  phantom: number;
  covered: number;
  implicit: number;
  outOfScope: number;
  truncated: number;
};

export type AuditView = { missed: AuditItem[]; uncertain: AuditItem[]; stats: AuditStats };

export const EMPTY_AUDIT: AuditView = {
  missed: [],
  uncertain: [],
  stats: { calls: 0, captured: 0, missed: 0, uncertain: 0, phantom: 0, covered: 0, implicit: 0, outOfScope: 0, truncated: 0 },
};

/** Fired by the watcher with the project roots whose audit file changed. */
export const AUDIT_EVENT = "claudegate://audit-changed";

export const UNCERTAIN_TITLE = "Changed while Claude was running (may be yours)";

/** The quiet header line; null when nothing was missed. */
export function missedHeader(n: number): string | null {
  if (n <= 0) return null;
  return n === 1 ? "1 file Claude changed wasn't captured" : `${n} files Claude changed weren't captured`;
}

/** Settings → Capture. Zero terms are left out, except captured. */
export function statsLine(s: AuditStats): string {
  const parts = [`${s.captured} captured`];
  if (s.missed > 0) parts.push(`${s.missed} missed`);
  if (s.phantom > 0) parts.push(`${s.phantom} phantom`);
  if (s.implicit > 0) parts.push(`${s.implicit} implicit`);
  return `Last 7 days: ${parts.join(" · ")}`;
}

function under(root: string, path: string): boolean {
  const r = root.replace(/\/+$/, "");
  return path.startsWith(r + "/");
}

/** The scope `item` belongs to: the deepest of its own root and the project's scopes that holds the path. */
export function scopeOf(item: AuditItem, scopeRoots: string[]): string | null {
  let best: string | null = null;
  for (const r of [item.root, ...scopeRoots]) if (r && under(r, item.path) && (best === null || r.length > best.length)) best = r;
  return best;
}

/** The path relative to its scope; the full path when no scope holds it. */
export function relPath(item: AuditItem, scopeRoots: string[]): string {
  const s = scopeOf(item, scopeRoots);
  return s ? item.path.slice(s.replace(/\/+$/, "").length + 1) : item.path;
}

/** The scope's folder name when it is not the project's own root (a worktree or nested repo), else null. */
export function scopeTag(item: AuditItem, projectRoot: string, scopeRoots: string[]): string | null {
  const s = scopeOf(item, scopeRoots);
  if (!s || s === projectRoot) return null;
  return s.split("/").filter(Boolean).pop() ?? null;
}

export function itemTime(item: AuditItem, now: number = Date.now()): string {
  return relTime(new Date(item.atMs).toISOString(), now);
}

/** The view without `paths`: what Dismiss shows at once, before the backend answers. */
export function withoutPaths(v: AuditView, paths: string[]): AuditView {
  const gone = new Set(paths);
  return { ...v, missed: v.missed.filter((i) => !gone.has(i.path)), uncertain: v.uncertain.filter((i) => !gone.has(i.path)) };
}

/** Puts back the items of `before` named by `paths` (a failed dismiss), keeping what `now` has. */
export function restorePaths(now: AuditView, before: AuditView, paths: string[]): AuditView {
  const back = new Set(paths);
  const merge = (cur: AuditItem[], old: AuditItem[]) => {
    const have = new Set(cur.map((i) => i.path));
    const order = new Map(old.map((i, n) => [i.path, n]));
    return [...cur, ...old.filter((i) => back.has(i.path) && !have.has(i.path))].sort(
      (a, b) => (order.get(a.path) ?? Infinity) - (order.get(b.path) ?? Infinity),
    );
  };
  return { ...now, missed: merge(now.missed, before.missed), uncertain: merge(now.uncertain, before.uncertain) };
}

/** Roving tabindex: the row after/before `i`, clamped. */
export function rove(i: number, key: "ArrowDown" | "ArrowUp" | "Home" | "End", n: number): number {
  if (n === 0) return 0;
  if (key === "Home") return 0;
  if (key === "End") return n - 1;
  return Math.min(n - 1, Math.max(0, i + (key === "ArrowDown" ? 1 : -1)));
}

/**
 * Coalesces audit reads: never two in flight, and at most one start per `minMs` while events keep
 * arriving (a trailing read runs once the interval has passed, so the last change is never lost).
 * `request(true)` skips the interval (a project was just selected) but still waits for a read in
 * flight. No timer runs unless a request is waiting; nothing polls.
 */
export class AuditRefresher {
  private inflight = false;
  private dirty = false;
  private dirtyNow = false;
  private last = -Infinity;
  private timer: ReturnType<typeof setTimeout> | undefined;
  private dead = false;

  constructor(
    private run: () => Promise<void>,
    private minMs = 2000,
    private now: () => number = Date.now,
  ) {}

  request(immediate = false) {
    if (this.dead) return;
    if (this.inflight) {
      this.dirty = true;
      if (immediate) this.dirtyNow = true;
      return;
    }
    const wait = immediate ? 0 : this.last + this.minMs - this.now();
    if (wait <= 0) {
      this.start();
    } else if (this.timer === undefined) {
      this.timer = setTimeout(() => {
        this.timer = undefined;
        this.request();
      }, wait);
    }
  }

  private start() {
    clearTimeout(this.timer);
    this.timer = undefined;
    this.inflight = true;
    this.dirty = false;
    this.dirtyNow = false;
    this.last = this.now();
    void this.run()
      .catch(() => {})
      .finally(() => {
        this.inflight = false;
        if (this.dirty) this.request(this.dirtyNow);
      });
  }

  stop() {
    this.dead = true;
    clearTimeout(this.timer);
    this.timer = undefined;
  }
}
