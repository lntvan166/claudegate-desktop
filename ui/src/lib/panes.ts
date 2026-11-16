// Pane widths for the three columns (Projects | Review | Diff): clamping, keyboard steps, the
// collapsed Projects rail and persistence. Pure, so the splitters and the window-resize clamp share one set of rules.

export type Pane = "projects" | "tree";
export type Widths = Record<Pane, number>;

/** Today's widths (theme.css `--sidebar-w` and `--tree-w`). */
export const DEFAULTS: Readonly<Widths> = { projects: 228, tree: 372 };
export const MIN = { projects: 160, tree: 240, diff: 360 } as const;
/** Horizontal space that is neither pane: the sheet's right inset (8px) and its 1px borders. */
export const CHROME = 10;
export const STORE_KEY = "claudegate.paneWidths";
/** The collapsed Projects column: a rail of project initials. */
export const RAIL = 44;
/** At or below this window width the layout stacks: projects on top, no diff pane. Must match
 *  the `max-width` media query in App.svelte. */
export const STACK_MAX = 720;
export const stacked = (width: number) => width <= STACK_MAX;

const STEP = 16;
const BIG_STEP = 64;

/**
 * The widths that fit a window `total` px wide: each pane at least its minimum, and the diff left
 * with at least its own. When the window is too narrow the tree gives way first, then the
 * projects. The stored preference is untouched, so widening the window brings it back.
 */
export function fit(w: Widths, total: number): Widths {
  let projects = Math.max(MIN.projects, Math.round(w.projects));
  let tree = Math.max(MIN.tree, Math.round(w.tree));
  const room = total - CHROME - MIN.diff;
  let over = projects + tree - room;
  if (over > 0) {
    const t = Math.min(over, tree - MIN.tree);
    tree -= t;
    over -= t;
  }
  if (over > 0) projects -= Math.min(over, projects - MIN.projects);
  return { projects, tree };
}

/**
 * What is on screen: `fit` when the Projects column is expanded; when collapsed, the rail, with the
 * tree free to take the width it gave up. `w.projects` is never touched, so expanding restores it.
 */
export function layout(w: Widths, total: number, collapsed: boolean): Widths {
  if (!collapsed) return fit(w, total);
  const room = total - CHROME - MIN.diff - RAIL;
  return { projects: RAIL, tree: Math.max(MIN.tree, Math.min(Math.round(w.tree), room)) };
}

export const toggleCollapsed = (collapsed: boolean) => !collapsed;

/** The preference after the user sets `pane` to `width` (clamped). Collapsed, the stored
 *  projects width stays as it was. */
export function withWidth(w: Widths, pane: Pane, width: number, total: number, collapsed: boolean): Widths {
  const cur = layout(w, total, collapsed);
  const v = Math.min(maxFor(cur, pane, total), Math.max(MIN[pane], Math.round(width)));
  return collapsed ? { ...w, [pane]: v } : fit({ ...cur, [pane]: v }, total);
}

/** The largest width `pane` can take in a window `total` px wide, with the other pane as it is. */
export function maxFor(w: Widths, pane: Pane, total: number): number {
  const other = pane === "projects" ? w.tree : w.projects;
  return Math.max(MIN[pane], total - CHROME - MIN.diff - other);
}

/** `pane` moved by `delta` px, clamped. */
export function resize(w: Widths, pane: Pane, delta: number, total: number): Widths {
  const cur = fit(w, total);
  const want = Math.min(maxFor(cur, pane, total), Math.max(MIN[pane], cur[pane] + delta));
  return fit({ ...cur, [pane]: want }, total);
}

/** `pane` back to its default width; the other pane keeps its own. */
export function reset(w: Widths, pane: Pane, total: number): Widths {
  return fit({ ...w, [pane]: DEFAULTS[pane] }, total);
}

/** The px a key moves a splitter by: ←/→ 16px, with Shift 64px. Anything else is not ours. */
export function keyStep(key: string, shift: boolean): number | null {
  const n = shift ? BIG_STEP : STEP;
  if (key === "ArrowLeft") return -n;
  if (key === "ArrowRight") return n;
  return null;
}

export function save(w: Widths, collapsed = false): string {
  return JSON.stringify({ projects: w.projects, tree: w.tree, collapsed });
}

/** Whether the Projects column was left collapsed; anything but a stored `true` is expanded. */
export function loadCollapsed(raw: string | null): boolean {
  if (!raw) return false;
  try {
    const v: unknown = JSON.parse(raw);
    return !!v && typeof v === "object" && (v as Record<string, unknown>).collapsed === true;
  } catch {
    return false;
  }
}

/** A stored value, or the defaults when it is missing or not two positive finite numbers. */
export function load(raw: string | null): Widths {
  if (!raw) return { ...DEFAULTS };
  try {
    const v: unknown = JSON.parse(raw);
    if (v && typeof v === "object" && !Array.isArray(v)) {
      const { projects, tree } = v as Record<string, unknown>;
      const ok = (n: unknown): n is number => typeof n === "number" && Number.isFinite(n) && n > 0;
      if (ok(projects) && ok(tree)) return { projects, tree };
    }
  } catch {}
  return { ...DEFAULTS };
}
