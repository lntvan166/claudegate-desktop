// Typed wrappers over the Tauri commands in src-tauri/src/commands.rs and the watcher event.
// Field names mirror core/src/project.rs (serde camelCase).
//
// Outside Tauri, in a dev server only, calls go to a fixture-backed mock so the layout can be
// previewed in a plain browser. `import.meta.env.DEV` is a compile-time constant, so production
// builds drop the branch and never emit the mock chunk.
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { EditorConfig } from "./editors";

export type { EditorConfig };

export type ProjectSummary = {
  root: string;
  name: string;
  missing: boolean;
  pending: number;
};

export type PendingItem = {
  path: string;
  rel: string;
  scope: string;
  newFile: boolean;
  capturedAt: string | null;
  added: number;
  removed: number;
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
