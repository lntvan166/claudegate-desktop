// The Settings screen's pure helpers, and how the settings shape the review: sources, the glob
// matcher shared by exclude and protected, the pending-age label, grouping by Claude session and
// the app-only hide patterns. Shapes mirror src-tauri/src/commands.rs `SettingsView` and
// core/src/settings.rs `Effective`.
import type { DiffKind, ProjectView } from "./api";
import type { RefPathStyle } from "./ref";
import { basename, rowsOf, scopeLabel, type Group, type Row } from "./rows";

export type Source = "app" | "vsCode" | "cursor" | "workspace" | "default";
export type Resolved<T> = { value: T; source: Source };

export type SettingsView = {
  exclude: Resolved<string[]>;
  protected: Resolved<string[]>;
  autoAdvance: Resolved<boolean>;
  groupBySession: Resolved<boolean>;
  pendingAgeMinDays: Resolved<number>;
  maxAttached: Resolved<number>;
  hookLog: Resolved<boolean>;
  /** The selected project's app-only hide patterns (`projects[root].extraHide`). */
  extraHide: string[];
  /**
   * Who writes the selected project's exclude for the hook: the app (it added the project and
   * still owns it, so the list is editable here), VS Code after taking an app project over, or
   * VS Code because the project is its own.
   */
  excludeOwner: ExcludeOwner;
  /** App-only: how "Copy reference for Claude" writes the path. */
  copyRefPathStyle: RefPathStyle;
};

export type ExcludeOwner = "owned" | "managedByVsCode" | "vsCode";

/** The keys `set_setting` takes. */
export type SettingKey = "autoAdvance" | "groupBySession" | "pendingAgeMinDays" | "maxAttached" | "protected" | "copyRefPathStyle";

/** The extension's built-in defaults (core/src/settings.rs), for the screen before the first load. */
export const DEFAULT_PROTECTED = [
  "**/.env", "**/.env.*", "**/*.pem", "**/*.key", "**/*.p12", "**/*.pfx",
  "**/id_rsa", "**/id_ed25519", "**/.npmrc", "**/credentials",
];

export const DEFAULT_SETTINGS: SettingsView = {
  exclude: { value: [], source: "default" },
  protected: { value: DEFAULT_PROTECTED, source: "default" },
  autoAdvance: { value: true, source: "default" },
  groupBySession: { value: false, source: "default" },
  pendingAgeMinDays: { value: 0, source: "default" },
  maxAttached: { value: 256, source: "default" },
  hookLog: { value: false, source: "default" },
  extraHide: [],
  excludeOwner: "vsCode",
  copyRefPathStyle: "relative",
};

export function sourceLabel(source: Source): string {
  switch (source) {
    case "app":
      return "This app";
    case "vsCode":
      return "From VS Code";
    case "cursor":
      return "From Cursor";
    case "workspace":
      return "From this project's .vscode";
    default:
      return "Default";
  }
}

/** Port of `globToRegex` in the extension's excludeMatcher.ts; null when it does not compile. */
function globToRegex(glob: string): RegExp | null {
  const g = glob.replace(/\\/g, "/");
  let re = "^";
  for (let i = 0; i < g.length; i++) {
    const c = g[i];
    if (c === "*") {
      if (g[i + 1] === "*") {
        re += ".*";
        i++;
        if (g[i + 1] === "/") i++;
      } else {
        re += "[^/]*";
      }
    } else if (c === "?") {
      re += "[^/]";
    } else if ("\\^$.|+()[]{}".includes(c)) {
      re += `\\${c}`;
    } else {
      re += c;
    }
  }
  try {
    return new RegExp(`${re}$`);
  } catch {
    return null;
  }
}

/**
 * The extension's matcher: each glob is tried on the absolute path, the root-relative path and
 * every ancestor directory of it. Used for exclude, protected and the app-only hide patterns.
 */
export function globMatcher(root: string, globs: string[]): (path: string) => boolean {
  const patterns = globs.map(globToRegex).filter((r): r is RegExp => r !== null);
  const base = root.replace(/\\/g, "/").replace(/\/+$/, "");
  return (path: string) => {
    if (patterns.length === 0) return false;
    const abs = path.replace(/\\/g, "/");
    const cands = [abs];
    if (base && abs.startsWith(`${base}/`)) {
      const rel = abs.slice(base.length + 1);
      cands.push(rel);
      const parts = rel.split("/").filter(Boolean);
      for (let i = 1; i < parts.length; i++) cands.push(parts.slice(0, i).join("/"));
    }
    return patterns.some((re) => cands.some((c) => re.test(c)));
  };
}

/** The paths among `paths` that the protected globs match, in order. */
export function protectedAmong(paths: string[], root: string, globs: string[]): string[] {
  const m = globMatcher(root, globs);
  return paths.filter(m);
}

/** The inline question before accepting protected files: up to three names, then "+N more". */
export function protectPrompt(paths: string[]): string {
  const names = paths.map(basename);
  if (names.length === 1) return `This looks like a secret file (${names[0]}). Accept anyway?`;
  const shown = names.slice(0, 3).join(", ");
  const more = names.length > 3 ? ` +${names.length - 3} more` : "";
  return `These look like secret files (${shown}${more}). Accept anyway?`;
}

/** The extension's `ageLabel`: "Nd" for a row at least `minDays` whole days old; 0 or less is off. */
export function ageLabel(capturedAt: string | null | undefined, now: number, minDays: number): string | null {
  if (!Number.isFinite(minDays) || minDays <= 0 || !capturedAt) return null;
  const then = Date.parse(capturedAt);
  if (Number.isNaN(then)) return null;
  const days = Math.max(0, Math.floor((now - then) / 86_400_000));
  return days >= minDays ? `${days}d` : null;
}

const norm = (p: string) => p.replace(/\\/g, "/").replace(/\/+$/, "");

const localTime = (iso: string) => new Date(iso).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });

/**
 * The tab's rows grouped by Claude session, as the extension's `groupBySession` does: each
 * session labelled "Session N · HH:MM" from its earliest capture (or decision), numbered oldest
 * first and listed newest first, rows with no session last as "Unknown session". Sessions are
 * split per scope so a folder action still acts inside one real scope; a worktree's groups say
 * which worktree they belong to.
 */
export function groupsBySession(view: ProjectView, kind: DiffKind, time: (iso: string) => string = localTime): Group[] {
  const rows = rowsOf(view, kind);
  const out: Group[] = [];
  for (const scope of view.scopes) {
    const mine = rows.filter((r) => norm(r.scope) === norm(scope.root));
    if (mine.length === 0) continue;
    const buckets = new Map<string | null, { rows: Row[]; earliest: string }>();
    for (const r of mine) {
      const stamp = (kind === "pending" ? r.capturedAt : r.decidedAt) ?? "";
      const id = r.sessionId ?? null;
      const b = buckets.get(id);
      if (b) {
        b.rows.push(r);
        if (stamp && (b.earliest === "" || stamp < b.earliest)) b.earliest = stamp;
      } else {
        buckets.set(id, { rows: [r], earliest: stamp });
      }
    }
    const prefix = scope.isWorktree ? `${scopeLabel(view, scope)} · ` : "";
    const known = [...buckets.entries()]
      .filter(([id]) => id !== null)
      .sort((a, b) => (a[1].earliest || "~").localeCompare(b[1].earliest || "~"));
    const labelled = known.map(([id, b], i) => ({
      id,
      rows: b.rows,
      label: `${prefix}Session ${i + 1}${b.earliest ? ` · ${time(b.earliest)}` : ""}`,
    }));
    const unknown = buckets.get(null);
    const ordered = [...labelled.reverse(), ...(unknown ? [{ id: null, rows: unknown.rows, label: `${prefix}Unknown session` }] : [])];
    for (const g of ordered) {
      const key = `s:${scope.root}|${g.id ?? "?"}`;
      out.push({ scope, label: g.label, key, rows: g.rows.map((r) => ({ ...r, group: key })) });
    }
  }
  return out;
}

/** The view without the rows the app-only hide patterns match (every tab). */
export function hideExtra(view: ProjectView, patterns: string[]): ProjectView {
  if (patterns.length === 0) return view;
  const hidden = globMatcher(view.root, patterns);
  return {
    ...view,
    pending: view.pending.filter((p) => !hidden(p.path)),
    accepted: view.accepted.filter((r) => !hidden(r.path)),
    rejected: view.rejected.filter((r) => !hidden(r.path)),
  };
}

/** Parses a list typed one pattern per line (or comma-separated): trimmed, no blanks, no repeats. */
export function parsePatterns(text: string): string[] {
  const out: string[] = [];
  for (const p of text.split(/[\n,]/).map((s) => s.trim())) if (p && !out.includes(p)) out.push(p);
  return out;
}
