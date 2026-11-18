// Typed wrappers over the Tauri commands in src-tauri/src/commands.rs and the watcher event.
// Field names mirror core/src/project.rs (serde camelCase).
//
// Outside Tauri, in a dev server only, calls go to a fixture-backed mock so the layout can be
// previewed in a plain browser. `import.meta.env.DEV` is a compile-time constant, so production
// builds drop the branch and never emit the mock chunk.
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { DecideAction, DecideReport, DecideTarget, RejectOutcome } from "./decide";
import type { EditorConfig } from "./editors";
import type { SettingKey, SettingsView } from "./settings";

export type { DecideAction, DecideReport, DecideTarget, EditorConfig, SettingKey, SettingsView };

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
  /** The Claude session that captured it; null when the hook had none. */
  sessionId?: string | null;
  added: number;
  removed: number;
  /** No change on disk yet, inside the settle window: Claude's write may still be on its way. */
  waiting: boolean;
  /** What a reject does to it on disk (core `RejectOutcome`). */
  rejectOutcome: RejectOutcome;
};

export type RecordItem = {
  id: string;
  path: string;
  rel: string;
  scope: string;
  decidedAt: string;
  sessionId?: string | null;
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

/** core `projects::Added`: the root as listed; `already` when it was a project before. */
export type AddedProject = { root: string; already: boolean };
/** core `projects::Removed`: `unregistered` false when VS Code also uses the folder (capture stays on). */
export type RemovedProject = { root: string; unregistered: boolean };

export interface Api {
  listProjects(): Promise<ProjectSummary[]>;
  getProject(root: string): Promise<ProjectView>;
  getDiff(root: string, scope: string, kind: DiffKind, key: string): Promise<DiffPayload>;
  onChanged(handler: (p: ChangedPayload) => void): Promise<() => void>;
  getConfig(): Promise<EditorConfig>;
  /** Remembers `template` as the editor in ~/.claudegate-desktop/config.json. */
  setEditor(template: string): Promise<EditorConfig>;
  /** Opens `path` at `line`:`col`; `template` null means the remembered editor. */
  openInEditor(path: string, line: number, col: number, template: string | null): Promise<void>;
  /** The tray tooltip's total. */
  setPendingCount(count: number): Promise<void>;
  /**
   * Accepts, rejects, reverts, reapplies or clears. Rejects with a JSON string (see
   * `parseDecideError`). Emits no change event: the caller reloads the project afterwards.
   */
  decide(root: string, action: DecideAction, target: DecideTarget): Promise<DecideReport>;
  /** The global read-only switch in ~/.claudegate-desktop/config.json. */
  setReadOnly(on: boolean): Promise<EditorConfig>;
  /** The hidden project roots in ~/.claudegate-desktop/config.json (never workspace-roots.json). */
  setHiddenProjects(roots: string[]): Promise<EditorConfig>;
  /** Every setting as it resolves for `root` (app, then the extension, then the default). */
  getSettings(root: string | null): Promise<SettingsView>;
  /** One app override; `null` drops it. Resolves to the settings as they now stand for `root`. */
  setSetting(key: SettingKey, value: unknown, root: string | null): Promise<SettingsView>;
  /** Creates or deletes ~/.claudegate/hooklog.enabled, the switch the extension shares. */
  setHookLog(on: boolean, root: string | null): Promise<SettingsView>;
  /** The project's app-only hide patterns. */
  setExtraHide(root: string, patterns: string[]): Promise<SettingsView>;
  /**
   * Registers a folder in ~/.claudegate/workspace-roots.json so the hook captures there. Rejects
   * with a message for the user (naming the conflicting root for a nested folder).
   */
  addProject(path: string): Promise<AddedProject>;
  /** Unregisters a project the app added. Its review history (session file) is kept. */
  removeProject(root: string): Promise<RemovedProject>;
  /** Whether VS Code or Cursor also uses the folder, so Remove leaves capture on. Read only. */
  projectInVsCode(root: string): Promise<boolean>;
  /** The native folder picker; null when cancelled. */
  pickFolder(): Promise<string | null>;
}

const tauriApi: Api = {
  listProjects: () => invoke<ProjectSummary[]>("list_projects"),
  getProject: (root) => invoke<ProjectView>("get_project", { root }),
  getDiff: (root, scope, kind, key) => invoke<DiffPayload>("get_diff", { root, scope, kind, key }),
  onChanged: (handler) => listen<ChangedPayload>(CHANGED_EVENT, (e) => handler(e.payload)),
  getConfig: () => invoke<EditorConfig>("get_config"),
  setEditor: (template) => invoke<EditorConfig>("set_editor", { template }),
  openInEditor: (path, line, col, template) => invoke<void>("open_in_editor", { path, line, col, template }),
  setPendingCount: (count) => invoke<void>("set_pending_count", { count }),
  decide: (root, action, target) => invoke<DecideReport>("decide", { root, action, target }),
  setReadOnly: (on) => invoke<EditorConfig>("set_read_only", { on }),
  setHiddenProjects: (roots) => invoke<EditorConfig>("set_hidden_projects", { roots }),
  getSettings: (root) => invoke<SettingsView>("get_settings", { root }),
  setSetting: (key, value, root) => invoke<SettingsView>("set_setting", { key, value, root }),
  setHookLog: (on, root) => invoke<SettingsView>("set_hook_log", { on, root }),
  setExtraHide: (root, patterns) => invoke<SettingsView>("set_extra_hide", { root, patterns }),
  addProject: (path) => invoke<AddedProject>("add_project", { path }),
  removeProject: (root) => invoke<RemovedProject>("remove_project", { root }),
  projectInVsCode: (root) => invoke<boolean>("project_in_vs_code", { root }),
  pickFolder: () => invoke<string | null>("pick_folder"),
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
export const openInEditor = async (path: string, line: number, col = 1, template: string | null = null) =>
  (await api()).openInEditor(path, line, col, template);
export const setPendingCount = async (count: number) => (await api()).setPendingCount(count);
export const decide = async (root: string, action: DecideAction, target: DecideTarget) =>
  (await api()).decide(root, action, target);
export const setReadOnly = async (on: boolean) => (await api()).setReadOnly(on);
export const setHiddenProjects = async (roots: string[]) => (await api()).setHiddenProjects(roots);
export const getSettings = async (root: string | null) => (await api()).getSettings(root);
export const setSetting = async (key: SettingKey, value: unknown, root: string | null) =>
  (await api()).setSetting(key, value, root);
export const setHookLog = async (on: boolean, root: string | null) => (await api()).setHookLog(on, root);
export const setExtraHide = async (root: string, patterns: string[]) => (await api()).setExtraHide(root, patterns);
export const addProject = async (path: string) => (await api()).addProject(path);
export const removeProject = async (root: string) => (await api()).removeProject(root);
export const pickFolder = async () => (await api()).pickFolder();
export const projectInVsCode = async (root: string) => (await api()).projectInVsCode(root);
