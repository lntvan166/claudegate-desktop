// Pure helpers for decisions: what to undo, where to go next, what to say. The decide command
// itself lives in api.ts; its shapes mirror src-tauri/src/commands.rs and core/src/decide.rs.
import { sameRow, type RowRef } from "./rows";

/** The ActionBar's buttons; App maps each to a `decide` call with an `all` target. */
export type BulkAction = "acceptAll" | "rejectAll" | "revertAll" | "reapplyAll" | "clear";

export type DecideAction = "accept" | "reject" | "revert" | "reapply" | "clearAccepted" | "clearRejected";

/**
 * For accept/reject/reapply `key` is the path; for revert it is the record id. `keys` on a folder or
 * all target are the rows the user counted and confirmed; the backend acts on no other row and
 * reports any that changed since.
 */
export type DecideTarget =
  | { kind: "file"; scope: string; key: string }
  | { kind: "folder"; scope: string; prefix: string; keys?: string[] }
  /** `scopedKeys`, when given, replaces `keys`: each confirmed row with its scope. */
  | { kind: "all"; keys?: string[]; scopedKeys?: RowRef[] };

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
  action: "revert" | "reapply";
  /**
   * What the undo sends. A file target's key, and a bulk or folder target's `keys`, are filled in
   * by `undoKeys` just before the call: paths for a reapply, the new records' ids for a revert.
   */
  target: DecideTarget;
  /** The keys are record ids resolved by path (see `recordIdFor`) against a fresh fetch. */
  byPath: boolean;
  /** The files the decision actually decided, each with its scope. */
  rows: { scope: string; path: string }[];
};

/**
 * The decision that undoes `action` on `target`: an accept by reverting the records it made, a
 * reject by reapplying the same paths, in the same scopes. A single file must have been decided
 * cleanly. A bulk or folder decision needs `decided`, the rows the user confirmed; the undo covers
 * those minus any that failed or changed since. Revert, reapply and clear have none.
 */
export function undoFor(
  action: DecideAction,
  target: DecideTarget,
  report: DecideReport,
  decided?: { scope: string; path: string }[],
): Undo | null {
  if (action !== "accept" && action !== "reject") return null;
  const undo = action === "accept" ? "revert" : "reapply";
  const byPath = action === "accept";
  if (target.kind === "file") {
    if (report.done !== 1 || report.errors.length > 0) return null;
    const t = { kind: "file" as const, scope: target.scope, key: target.key };
    return { action: undo, target: t, byPath, rows: [{ scope: target.scope, path: target.key }] };
  }
  if (!decided || report.done === 0) return null;
  const failed = new Set(report.errors.map(([p]) => p));
  const rows = decided.filter((r) => !failed.has(r.path));
  if (rows.length === 0) return null;
  const keys = byPath ? [] : rows.map((r) => r.path);
  return { action: undo, target: { ...target, keys }, byPath, rows };
}

/**
 * The keys an undo sends: its paths for a reapply; for a revert, the record each accept made,
 * found in `accepted` (a fresh fetch) by scope, path and `decidedAt` ≥ `since`. `missing` names
 * the paths whose record could not be found; they are not sent.
 */
export function undoKeys(
  u: Undo,
  accepted: { id: string; path: string; scope: string; decidedAt: string }[],
  since: string,
): { keys: string[]; missing: string[] } {
  if (!u.byPath) return { keys: u.rows.map((r) => r.path), missing: [] };
  const keys: string[] = [];
  const missing: string[] = [];
  for (const r of u.rows) {
    const id = recordIdFor(accepted, r.scope, r.path, since);
    if (id) keys.push(id);
    else missing.push(r.path);
  }
  return { keys, missing };
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
export function nextAfterDecision<R extends RowRef>(rows: R[], decided: RowRef): RowRef | null {
  const i = rows.findIndex((r) => sameRow(r, decided));
  if (i === -1) return null;
  const n = rows[i + 1] ?? rows[i - 1];
  return n ? { scope: n.scope, key: n.key } : null;
}

/** `path` lies strictly inside `dir`, either separator, as the backend's `is_under`. */
function isUnder(path: string, dir: string): boolean {
  const d = norm(dir);
  return norm(path).startsWith(`${d}/`);
}

/**
 * The keys a tab's bulk action acts on, which are also what its count shows. Pending: rows whose
 * write has landed and that lie inside their scope root (the backend skips waiting rows and rows
 * outside the root; excluded ones never reach the view). Accepted: record ids. Rejected: paths.
 */
export function bulkKeys(
  view: {
    pending: { path: string; scope: string; waiting: boolean }[];
    accepted: { id: string }[];
    rejected: { path: string }[];
  },
  tab: "pending" | "accepted" | "rejected",
): string[] {
  if (tab === "accepted") return view.accepted.map((r) => r.id);
  if (tab === "rejected") return view.rejected.map((r) => r.path);
  return view.pending.filter((r) => !r.waiting && isUnder(r.path, r.scope)).map((r) => r.path);
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

/** A confirmation's question and its go button. `danger` styles the button as destructive. */
export type Prompt = { text: string; go: string; danger: boolean };

const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

/** Accept all asks first above this many files. */
export const ACCEPT_ALL_ASK_ABOVE = 5;

/**
 * The question asked before a bulk action, or null for an Accept all of 5 files or fewer. Revert
 * and Reapply use the extension's wording (`revertAcceptedAll`, `reapplyAll` in extension.ts).
 */
/**
 * `reappear`, for Clear: older pending entries of the parent that only these records keep out of
 * the list (a repo nested in it decided those files later), and the parent's name.
 */
export function bulkPrompt(
  action: BulkAction,
  tab: "pending" | "accepted" | "rejected",
  count: number,
  reappear?: { n: number; where: string },
): Prompt | null {
  switch (action) {
    case "rejectAll":
      // The real question is `rejectPrompt`, built from the rows; this is only its fallback.
      return {
        text: `Reject ${plural(count, "file", "files")}? Originals are restored and files Claude created are deleted.`,
        go: `Reject ${count}`,
        danger: true,
      };
    case "revertAll":
      return { text: `Revert all ${plural(count, "accepted file", "accepted files")} back to pending review?`, go: `Revert ${count}`, danger: false };
    case "reapplyAll":
      return { text: `Re-apply Claude's version to all ${plural(count, "rejected file", "rejected files")} on disk?`, go: `Reapply ${count}`, danger: true };
    case "clear":
      return {
        text:
          `Clear ${plural(count, "record", "records")} from ${tab === "accepted" ? "Accepted" : "Rejected"}? Files on disk stay as they are.` +
          (reappear && reappear.n > 0
            ? ` ${plural(reappear.n, "older change", "older changes")} in ${reappear.where} will show again.`
            : ""),
        go: `Clear ${count}`,
        danger: true,
      };
    case "acceptAll":
      // Accepting changes no file, so it is not destructive: asked only for a large batch.
      return count > ACCEPT_ALL_ASK_ABOVE ? { text: `Accept ${plural(count, "file", "files")}?`, go: `Accept ${count}`, danger: false } : null;
  }
}

/** What rejecting a pending file does on disk (core `RejectOutcome`). */
export type RejectOutcome = "restore" | "delete" | "leave";

/** Deleted files named in a reject prompt before "+N more". */
const NAMED = 3;

/**
 * The question before Reject all (no `folderRel`) or a folder reject: how many files are restored,
 * which new files are deleted (by name, at most 3), and any left as they are. `rows` are exactly
 * the rows the reject will send.
 */
export function rejectPrompt(rows: { path: string; rejectOutcome: RejectOutcome }[], folderRel?: string): Prompt {
  const n = rows.length;
  const restored = rows.filter((r) => r.rejectOutcome === "restore").length;
  const deleted = rows.filter((r) => r.rejectOutcome === "delete").map((r) => nameOf(r.path));
  const left = rows.filter((r) => r.rejectOutcome === "leave").length;
  const where = folderRel ? ` in "${norm(folderRel).split("/").pop() || folderRel}"` : "";
  const ask = `Reject ${plural(n, "file", "files")}${where}?`;
  if (deleted.length === 0 && left === 0) return { text: `${ask} Originals are restored.`, go: `Reject ${n}`, danger: true };
  const parts: string[] = [];
  if (restored > 0) parts.push(`${restored} restored to ${restored === 1 ? "its original" : "their originals"}`);
  if (left > 0) parts.push(`${left} left as is (no original to restore)`);
  if (deleted.length > 0) {
    const shown = deleted.slice(0, NAMED).join(", ");
    const more = deleted.length > NAMED ? ` +${deleted.length - NAMED} more` : "";
    parts.push(`${plural(deleted.length, "new file", "new files")} deleted: ${shown}${more}`);
  }
  return { text: `${ask} ${parts.join(", ")}`, go: `Reject ${n}`, danger: true };
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
