// Pure helpers for decisions: what to undo, where to go next, what to say. The decide command
// itself lives in api.ts; its shapes mirror src-tauri/src/commands.rs and core/src/decide.rs.

/** The ActionBar's buttons; App maps each to a `decide` call with an `all` target. */
export type BulkAction = "acceptAll" | "rejectAll" | "revertAll" | "reapplyAll" | "clear";

export type DecideAction = "accept" | "reject" | "revert" | "reapply" | "clearAccepted" | "clearRejected";

/** For accept/reject/reapply `key` is the path; for revert it is the record id. */
export type DecideTarget =
  | { kind: "file"; scope: string; key: string }
  | { kind: "folder"; scope: string; prefix: string }
  | { kind: "all" };

export type DecideReport = {
  done: number;
  /** Rejected files left on disk (a null baseline not known to be Claude's new file). */
  left: string[];
  /** `[path, message]` of files whose workspace write failed; they were not decided. */
  errors: [string, string][];
};

export type DecideErrorKind = "store" | "readOnly" | "notFound" | "badTarget" | "invalid";

export type DecideError = {
  kind: DecideErrorKind;
  message: string;
  /** Store errors: files changed on disk whose review log was not updated. */
  changed?: string[];
  /** Store errors: what earlier scopes completed. */
  partial?: DecideReport;
  /** Store errors: the dir holding the changed files' bytes from just before they were written. */
  snapshot?: string | null;
};

const KINDS: readonly string[] = ["store", "readOnly", "notFound", "badTarget", "invalid"];

/** The backend rejects with a JSON string; anything else is reported as-is under "invalid". */
export function parseDecideError(raw: unknown): DecideError {
  const text = typeof raw === "string" ? raw : String(raw);
  if (typeof raw !== "string") return { kind: "invalid", message: text };
  let v: unknown;
  try {
    v = JSON.parse(raw);
  } catch {
    return { kind: "invalid", message: raw };
  }
  if (!v || typeof v !== "object" || Array.isArray(v)) return { kind: "invalid", message: raw };
  const o = v as Record<string, unknown>;
  if (typeof o.kind !== "string" || !KINDS.includes(o.kind) || typeof o.message !== "string") {
    return { kind: "invalid", message: raw };
  }
  const out: DecideError = { kind: o.kind as DecideErrorKind, message: o.message };
  if (o.kind === "store") {
    out.changed = Array.isArray(o.changed) ? o.changed.filter((c): c is string => typeof c === "string") : [];
    const p = o.partial as Partial<DecideReport> | undefined;
    out.partial = {
      done: typeof p?.done === "number" ? p.done : 0,
      left: Array.isArray(p?.left) ? p.left : [],
      errors: Array.isArray(p?.errors) ? p.errors : [],
    };
    out.snapshot = typeof o.snapshot === "string" ? o.snapshot : null;
  }
  return out;
}

/**
 * The second line of a store error, and whether to offer Retry. A reject is never retried: its
 * files were already restored or deleted, so a second reject would record the original as
 * Claude's version. Claude's real version is in the snapshot, which the note names.
 */
export function storeFailure(action: DecideAction, err: DecideError): { note: string; retry: boolean } {
  const changed = err.changed ?? [];
  const retry = action !== "reject";
  if (changed.length === 0) return { note: "Nothing was changed.", retry };
  if (err.snapshot) {
    const what = action === "reject" ? "Claude's version was" : "The files' previous contents were";
    return { note: `${what} saved to ${err.snapshot}`, retry };
  }
  return { note: "Files changed but the review log was not updated.", retry };
}

export type Undo = {
  action: DecideAction;
  target: { kind: "file"; scope: string; key: string };
  /**
   * The key is a path that must be resolved to the new record's id (in the refreshed Accepted
   * list, see `recordIdFor`) before the call.
   */
  byPath: boolean;
};

/**
 * The decision that undoes `action` on `target`: an accept by reverting its new record, a reject by
 * reapplying the same path, both in the same scope. Only a single file that was decided can be
 * undone; bulk and folder actions, revert, reapply and clear have none.
 */
export function undoFor(action: DecideAction, target: DecideTarget, report: DecideReport): Undo | null {
  if (target.kind !== "file" || report.done !== 1 || report.errors.length > 0) return null;
  const t = { kind: "file" as const, scope: target.scope, key: target.key };
  if (action === "accept") return { action: "revert", target: t, byPath: true };
  if (action === "reject") return { action: "reapply", target: t, byPath: false };
  return null;
}

const norm = (p: string) => p.replace(/\\/g, "/").replace(/\/+$/, "");

/**
 * The accepted record a decision made: the newest for `path` in `scope` stamped at or after
 * `sinceIso`, the moment just before the call (the backend's id is `<decidedAt>::<path>`). An
 * older accept of the same path is never returned: undoing it would revert the wrong history entry.
 */
export function recordIdFor(
  accepted: { id: string; path: string; scope: string; decidedAt: string }[],
  scope: string,
  path: string,
  sinceIso: string,
): string | null {
  const since = Date.parse(sinceIso);
  let best: { id: string; at: number } | null = null;
  for (const r of accepted) {
    if (r.path !== path || norm(r.scope) !== norm(scope)) continue;
    const at = Date.parse(r.decidedAt);
    if (!(at >= since)) continue;
    if (!best || at > best.at) best = { id: r.id, at };
  }
  return best?.id ?? null;
}

/** Must match `SETTINGS_UNREADABLE_MESSAGE` in src-tauri/src/commands.rs. */
export const SETTINGS_UNREADABLE_MESSAGE =
  "ClaudeGate Desktop couldn't read its settings file (~/.claudegate-desktop/config.json), so decisions are paused. Fix or delete that file.";

/** The read-only refusal caused by an unreadable settings file, and no other. */
export function isSettingsUnreadable(err: DecideError): boolean {
  return err.kind === "readOnly" && err.message === SETTINGS_UNREADABLE_MESSAGE;
}

/** The row to select once `decidedKey` leaves the list: the next one, else the previous, else none. */
export function nextAfterDecision(rows: { key: string }[], decidedKey: string): string | null {
  const i = rows.findIndex((r) => r.key === decidedKey);
  if (i === -1) return null;
  return rows[i + 1]?.key ?? rows[i - 1]?.key ?? null;
}

/**
 * How many rows a bulk accept or reject touches. Excluded entries never reach the view; rows still
 * waiting for Claude's write are skipped by the backend, so they are not counted.
 */
export function actionableCount(rows: { waiting: boolean }[]): number {
  return rows.filter((r) => !r.waiting).length;
}

/** The absolute folder path a folder row stands for: its scope root plus its relative path. */
export function folderPrefix(scope: string, rel: string): string {
  return `${scope.replace(/[\\/]+$/, "")}/${rel}`;
}

const PAST: Record<DecideAction, string> = {
  accept: "Accepted",
  reject: "Rejected",
  revert: "Reverted",
  reapply: "Reapplied",
  clearAccepted: "Cleared",
  clearRejected: "Cleared",
};
const VERB: Record<DecideAction, string> = {
  accept: "accept",
  reject: "reject",
  revert: "revert",
  reapply: "reapply",
  clearAccepted: "clear",
  clearRejected: "clear",
};

function nameOf(key: string): string {
  const path = key.includes("::") ? key.slice(key.indexOf("::") + 2) : key;
  return norm(path).split("/").pop() || path;
}

export type Tone = "ok" | "warn" | "error";

/**
 * The result bar's sentence and tone: "error" when nothing was decided because it failed, "warn"
 * when something failed or a rejected file was left on disk, else "ok".
 */
export function summarize(
  action: DecideAction,
  target: DecideTarget,
  report: DecideReport,
): { text: string; tone: Tone } {
  const failed = report.errors.length > 0;
  const tone: Tone = failed && report.done === 0 ? "error" : failed || report.left.length > 0 ? "warn" : "ok";
  if (target.kind === "file") {
    const name = nameOf(target.key);
    if (report.done === 0 && failed) return { text: `Couldn't ${VERB[action]} ${name}`, tone };
    if (report.left.length > 0) return { text: `${PAST[action]} ${name} · left on disk (no original to restore)`, tone };
    return { text: `${PAST[action]} ${name}`, tone };
  }
  const clearing = action === "clearAccepted" || action === "clearRejected";
  const noun = clearing ? (report.done === 1 ? "record" : "records") : report.done === 1 ? "file" : "files";
  const extra: string[] = [];
  if (report.left.length > 0) extra.push(`${report.left.length} left on disk`);
  if (failed) extra.push(`${report.errors.length} failed`);
  const text = extra.length
    ? [`${PAST[action]} ${report.done}`, ...extra].join(" · ")
    : `${PAST[action]} ${report.done} ${noun}`;
  return { text, tone };
}

/** What the result bar shows after a decision. `id` changes on every new result. */
export type ResultMessage = {
  id: number;
  text: string;
  /** Only "ok" auto-hides. */
  tone: Tone;
  /** A second line, e.g. the store error's "Files changed but…". */
  note?: string;
  undo?: () => void;
  retry?: () => void;
  /** Per-file failures, listed under [Details]. */
  details?: [string, string][];
};

/** The result bar hides itself after this long, unless hovered, focused or showing an error. */
export const RESULT_HIDE_MS = 6000;
