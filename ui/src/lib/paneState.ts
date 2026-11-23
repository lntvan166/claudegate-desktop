// What the diff pane shows. Exactly one of these is on screen at a time; DiffView renders from
// this alone, so the empty state and the editor can never be drawn over each other.

export type PaneState = "empty" | "loading" | "error" | "binary" | "editor";

type Side = { kind: "text" | "binary" | "missing" };

/**
 * `payload` must be the diff fetched for `row` (null when none is, or when it belongs to another
 * row). A selected row whose re-fetch is in flight keeps showing its last payload.
 */
export function paneState(
  row: unknown | null,
  payload: { before: Side; after: Side } | null,
  loading: boolean,
  error: string | null,
): PaneState {
  if (!row) return "empty";
  if (error) return "error";
  if (!payload) return "loading";
  void loading;
  if (payload.before.kind === "binary" || payload.after.kind === "binary") return "binary";
  return "editor";
}

/** The file had text before and is gone now: Claude (or someone) deleted it. */
export function deletedOnDisk(payload: { before: Side; after: Side } | null): boolean {
  return !!payload && payload.before.kind === "text" && payload.after.kind === "missing";
}

/**
 * What the diff pane says when a diff can't be fetched. The backend's bare "not found" means the
 * entry left the session between the list and the fetch (decided in VS Code, or pruned); say so.
 */
export function diffErrorText(error: string): string {
  if (error.trim() === "not found") {
    return "This change is no longer in the review log: it was decided or dropped elsewhere. The list updates on its own.";
  }
  return error;
}
