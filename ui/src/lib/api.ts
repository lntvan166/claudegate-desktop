// Typed wrappers over the Tauri commands in src-tauri/src/commands.rs and the watcher event.
// Field names mirror core/src/project.rs (serde camelCase).
//
// Outside Tauri, in a dev server only, calls go to a fixture-backed mock so the layout can be
// previewed in a plain browser. `import.meta.env.DEV` is a compile-time constant, so production
// builds drop the branch and never emit the mock chunk.
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { DecideAction, DecideReport, DecideTarget } from "./decide";
import type { EditorConfig } from "./editors";

export type { DecideAction, DecideReport, DecideTarget, EditorConfig };

export type ProjectSummary = {
  root: string;
  name: string;
  missing: boolean;
  pending: number;
  /** See `ProjectView.nextRecheckAt`. */
  nextRecheckAt: number | null;
};

export type PendingItem = {
  path: string;
  rel: string;
  scope: string;
  newFile: boolean;
  capturedAt: string | null;
  added: number;
  removed: number;
  /** No change on disk yet, inside the settle window: Claude's write may still be on its way. */
  waiting: boolean;
};

export type RecordItem = {
  id: string;
  path: string;
  rel: string;
  scope: string;
  decidedAt: string;
  added: number;
  removed: number;
};

export type ScopeView = {
  root: string;
  isWorktree: boolean;
  shapeUnknown: string[];
  error: string | null;
};

export type ProjectView = {
  root: string;
  missing: boolean;
  scopes: ScopeView[];
  pending: PendingItem[];
  accepted: RecordItem[];
  rejected: RecordItem[];
  /** Epoch ms when a waiting row may settle; re-fetch the project then. Null when none waits. */
  nextRecheckAt: number | null;
};

export type DiffKind = "pending" | "accepted" | "rejected";

export type TextSide = { kind: "missing" } | { kind: "binary" } | { kind: "text"; text: string };

export type DiffPayload = { path: string; before: TextSide; after: TextSide };

/** `roots` empty means the project list itself may have changed: re-list everything. */
export type ChangedPayload = { roots: string[] };

export const CHANGED_EVENT = "claudegate://changed";

export interface Api {
  listProjects(): Promise<ProjectSummary[]>;
  getProject(root: string): Promise<ProjectView>;
  getDiff(root: string, scope: string, kind: DiffKind, key: string): Promise<DiffPayload>;
  onChanged(handler: (p: ChangedPayload) => void): Promise<() => void>;
  getConfig(): Promise<EditorConfig>;
  /** Remembers `template` as the editor in ~/.claudegate-desktop/config.json. */
  setEditor(template: string): Promise<EditorConfig>;
  /** Opens `path` at `line`; `template` null means the remembered editor. */
  openInEditor(path: string, line: number, template: string | null): Promise<void>;
  /** The tray tooltip's total. */
  setPendingCount(count: number): Promise<void>;
  /**
   * Accepts, rejects, reverts, reapplies or clears. Rejects with a JSON string (see
   * `parseDecideError`). Emits no change event: the caller reloads the project afterwards.
   */
  decide(root: string, action: DecideAction, target: DecideTarget): Promise<DecideReport>;
  /** The global read-only switch in ~/.claudegate-desktop/config.json. */
  setReadOnly(on: boolean): Promise<EditorConfig>;
}

const tauriApi: Api = {
  listProjects: () => invoke<ProjectSummary[]>("list_projects"),
  getProject: (root) => invoke<ProjectView>("get_project", { root }),
  getDiff: (root, scope, kind, key) => invoke<DiffPayload>("get_diff", { root, scope, kind, key }),
  onChanged: (handler) => listen<ChangedPayload>(CHANGED_EVENT, (e) => handler(e.payload)),
  getConfig: () => invoke<EditorConfig>("get_config"),
  setEditor: (template) => invoke<EditorConfig>("set_editor", { template }),
  openInEditor: (path, line, template) => invoke<void>("open_in_editor", { path, line, template }),
  setPendingCount: (count) => invoke<void>("set_pending_count", { count }),
  decide: (root, action, target) => invoke<DecideReport>("decide", { root, action, target }),
  setReadOnly: (on) => invoke<EditorConfig>("set_read_only", { on }),
};

let chosen: Promise<Api> | undefined;

function api(): Promise<Api> {
  if (!chosen) {
    if (import.meta.env.DEV && !isTauri()) {
      chosen = import("./mock").then((m) => m.mockApi);
    } else {
      chosen = Promise.resolve(tauriApi);
    }
  }
  return chosen;
}

/** True when running against fixtures in a plain browser (dev server only). */
export const usingMock = import.meta.env.DEV && !isTauri();

export const listProjects = async () => (await api()).listProjects();
export const getProject = async (root: string) => (await api()).getProject(root);
export const getDiff = async (root: string, scope: string, kind: DiffKind, key: string) =>
  (await api()).getDiff(root, scope, kind, key);
export const onChanged = async (handler: (p: ChangedPayload) => void) =>
  (await api()).onChanged(handler);
export const getConfig = async () => (await api()).getConfig();
export const setEditor = async (template: string) => (await api()).setEditor(template);
export const openInEditor = async (path: string, line: number, template: string | null = null) =>
  (await api()).openInEditor(path, line, template);
export const setPendingCount = async (count: number) => (await api()).setPendingCount(count);
export const decide = async (root: string, action: DecideAction, target: DecideTarget) =>
  (await api()).decide(root, action, target);
export const setReadOnly = async (on: boolean) => (await api()).setReadOnly(on);
