// Typed wrappers over the Tauri commands in src-tauri/src/commands.rs and the watcher event.
// Field names mirror core/src/project.rs (serde camelCase).
//
// Outside Tauri, in a dev server only, calls go to a fixture-backed mock so the layout can be
// previewed in a plain browser. `import.meta.env.DEV` is a compile-time constant, so production
// builds drop the branch and never emit the mock chunk.
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { LaunchTarget } from "./launch";
import type { DecideAction, DecideReport, DecideTarget, RejectOutcome } from "./decide";
import type { CaptureView, HookReplacedView, HookView, InstallView } from "./capture";
import type { EditorConfig } from "./editors";
import type { SettingKey, SettingsView } from "./settings";
import type { UpdateView } from "./update";
import type { GitLogConfig, RepoView } from "../gitlog/repos";
import type { FilterView, PageView } from "../gitlog/log";

export type { CaptureView, HookReplacedView, HookView, InstallView, UpdateView };
export type { GitLogConfig, RepoView };
export type { DecideAction, DecideReport, DecideTarget, EditorConfig, SettingKey, SettingsView };

export type ProjectSummary = {
  root: string;
  name: string;
  missing: boolean;
  pending: number;
  /** See `ProjectView.nextRecheckAt`. */
  nextRecheckAt: number | null;
  /** Registered roots nested inside this one, shown as its scopes instead of as projects. */
  nested?: string[];
  /** `pending` without the rows still waiting for their write: what counts outside the workspace. */
  ready?: number;
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
  /** A registered root nested inside the project's root: its own repo, session and exclude. */
  nested?: boolean;
  /** Older parent entries only this scope's accepted (rejected) records hide; a Clear shows them. */
  revealedByClearAccepted?: number;
  revealedByClearRejected?: number;
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
  /** The summary's `pending` and `ready` as of this load (without the app-only hidden rows). */
  countedPending?: number;
  countedReady?: number;
};

export type DiffKind = "pending" | "accepted" | "rejected";

export type TextSide = { kind: "missing" } | { kind: "binary" } | { kind: "text"; text: string };

export type DiffPayload = { path: string; before: TextSide; after: TextSide };

/** `roots` empty means the project list itself may have changed: re-list everything. */
export type ChangedPayload = { roots: string[] };

export const CHANGED_EVENT = "claudegate://changed";
export const LAUNCH_EVENT = "claudegate://launch";

/** core `projects::Added`: the root as listed; `already` when it was a project before. */
export type AddedProject = { root: string; already: boolean };
/** core `projects::Removed`: `unregistered` false when VS Code also uses the folder (capture stays on). */
export type RemovedProject = { root: string; unregistered: boolean };
/** commands.rs `WorkspaceRemoved`: the config now, and the capture side when it was stopped too. */
export type WorkspaceRemoved = {
  config: EditorConfig;
  removed: RemovedProject | null;
  /** Capture was stopped, but the project could not be taken off the rail: why. */
  workspaceError?: string | null;
};

export interface Api {
  listProjects(): Promise<ProjectSummary[]>;
  getProject(root: string): Promise<ProjectView>;
  getDiff(root: string, scope: string, kind: DiffKind, key: string): Promise<DiffPayload>;
  onChanged(handler: (p: ChangedPayload) => void): Promise<() => void>;
  /** A launch (`claudegate [PATH]`, first or second instance) asked for this path. */
  onLaunch(handler: (t: LaunchTarget) => void): Promise<() => void>;
  /** The launch paths stored so far (the first launch, second ones before this call), once; later ones are events. */
  launchPath(): Promise<LaunchTarget[]>;
  getConfig(): Promise<EditorConfig>;
  /** Remembers `template` as the editor in ~/.claudegate-desktop/config.json. */
  setEditor(template: string): Promise<EditorConfig>;
  /** Opens `path` at `line`:`col`; `template` null means the remembered editor. */
  openInEditor(path: string, line: number, col: number, template: string | null): Promise<void>;
  /** The tray's total: the workspace's pending, plus what waits in known projects outside it. */
  setPendingCount(count: number, elsewhere: number): Promise<void>;
  /**
   * Accepts, rejects, reverts, reapplies or clears. Rejects with a JSON string (see
   * `parseDecideError`). Emits no change event: the caller reloads the project afterwards.
   */
  decide(root: string, action: DecideAction, target: DecideTarget): Promise<DecideReport>;
  /** The global read-only switch in ~/.claudegate-desktop/config.json. */
  setReadOnly(on: boolean): Promise<EditorConfig>;
  /** Adds one known project to the workspace in config.json (add-only; never workspace-roots.json). */
  addToWorkspace(root: string): Promise<EditorConfig>;
  /**
   * Takes one project off the rail, with the nested roots it folds (remove-only, view only).
   * `stopCapture` (an app-added root) first runs the Remove flow; when that fails nothing changes.
   */
  removeFromWorkspace(root: string, nested: string[], stopCapture: boolean): Promise<WorkspaceRemoved>;
  /** Every setting as it resolves for `root` (app, then the extension, then the default). */
  getSettings(root: string | null): Promise<SettingsView>;
  /** One app override; `null` drops it. Resolves to the settings as they now stand for `root`. */
  setSetting(key: SettingKey, value: unknown, root: string | null): Promise<SettingsView>;
  /** Creates or deletes ~/.claudegate/hooklog.enabled, the switch the extension shares. */
  setHookLog(on: boolean, root: string | null): Promise<SettingsView>;
  /** The project's app-only hide patterns. */
  /** `viewRoot`: the project whose settings to return, when `root` is a repo nested in it. */
  setExtraHide(root: string, patterns: string[], viewRoot?: string): Promise<SettingsView>;
  /**
   * The exclude list of a project the app added and still owns, written to
   * ~/.claudegate/excludes.json for the hook; `null` resets it to the defaults. Rejects for a VS
   * Code project, one VS Code took over, a blank pattern, and in read-only mode.
   */
  setProjectExclude(root: string, globs: string[] | null): Promise<SettingsView>;
  /** For an app project VS Code took over: the app manages its exclude again. Blocked in read-only mode. */
  takeBackExclude(root: string): Promise<SettingsView>;
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
  /** Whether Claude Code runs the ClaudeGate hook. `recheck` looks for python3 again. Read only. */
  captureStatus(recheck: boolean): Promise<CaptureView>;
  /**
   * Writes ~/.claudegate/hook.py, hook.sh and the marker, and adds one hook to
   * ~/.claude/settings.json (with a backup). Only after the user confirmed. Rejects with a message
   * for the user; blocked in read-only mode.
   */
  setUpCapture(): Promise<InstallView>;
  /** Replaces ~/.claudegate/hook.py with the bundled hook, after a backup (after the UI's confirmation). */
  updateCaptureHook(): Promise<HookReplacedView>;
  /** A newer release on GitHub, or null. Read only. */
  checkUpdate(): Promise<UpdateView | null>;
  /** Download, verify, install and restart (macOS, AppImage only). */
  installUpdate(): Promise<void>;

  // The Git log view (src-tauri/src/gitlog.rs, config.rs). Every config writer below adds ids,
  // removes ids or sets one key under the config lock, and answers with `gitLog` as it now stands.
  /** The workspace's repositories, discovery cached until the workspace changes or `refresh`. Read only. */
  gitlogRepos(refresh: boolean): Promise<RepoView[]>;
  /** `gitLog` in config.json. Read only. */
  gitlogConfig(): Promise<GitLogConfig>;
  /** Ticks `ids`, add-only. */
  gitlogTick(ids: string[]): Promise<GitLogConfig>;
  /** Unticks `ids`, remove-only (with every repo ticked, the backend's own list is made explicit). */
  gitlogUntick(ids: string[]): Promise<GitLogConfig>;
  /** Ticks every repository. */
  gitlogShowAll(): Promise<GitLogConfig>;
  /** Ticks exactly `ids`, the rows the user picked. */
  gitlogShowOnly(ids: string[]): Promise<GitLogConfig>;
  /** Ticks exactly the stored profile's repositories. */
  gitlogProfileApply(name: string): Promise<GitLogConfig>;
  /** Saves the stored ticks as profile `name`, replacing one of that name. */
  gitlogProfileSave(name: string): Promise<GitLogConfig>;
  gitlogProfileRename(from: string, to: string): Promise<GitLogConfig>;
  gitlogProfileDelete(name: string): Promise<GitLogConfig>;
  gitlogScanDepth(depth: number): Promise<GitLogConfig>;
  /** Adds one pattern to `excludeRepos`. */
  gitlogExclude(glob: string): Promise<GitLogConfig>;
  /** Removes one pattern from `excludeRepos`. */
  gitlogInclude(glob: string): Promise<GitLogConfig>;
  /** Sets one pane's width. */
  gitlogPane(pane: string, width: number): Promise<GitLogConfig>;
  /** The filter bar as last left; null drops it. */
  gitlogLastFilter(filter: Record<string, unknown> | null): Promise<GitLogConfig>;
  /** One page of the merged log. `more: false` starts query `queryId`; `more: true` continues it. */
  gitlogPage(queryId: number, repos: string[], filter: FilterView, more: boolean): Promise<PageView>;
  /** A commit's full message. */
  gitlogMessage(repo: string, sha: string): Promise<string>;
  /** The commit's page on its remote, or null when the host is not one we can build a link for. */
  gitlogWebUrl(repo: string, sha: string): Promise<string | null>;
  /** Opens the commit's page on its remote in the system browser; false when the remote has no link. */
  gitlogOpenRemote(repo: string, sha: string): Promise<boolean>;
}

const tauriApi: Api = {
  listProjects: () => invoke<ProjectSummary[]>("list_projects"),
  getProject: (root) => invoke<ProjectView>("get_project", { root }),
  getDiff: (root, scope, kind, key) => invoke<DiffPayload>("get_diff", { root, scope, kind, key }),
  onChanged: (handler) => listen<ChangedPayload>(CHANGED_EVENT, (e) => handler(e.payload)),
  onLaunch: (handler) => listen<LaunchTarget>(LAUNCH_EVENT, (e) => handler(e.payload)),
  launchPath: () => invoke<LaunchTarget[]>("launch_path"),
  getConfig: () => invoke<EditorConfig>("get_config"),
  setEditor: (template) => invoke<EditorConfig>("set_editor", { template }),
  openInEditor: (path, line, col, template) => invoke<void>("open_in_editor", { path, line, col, template }),
  setPendingCount: (count, elsewhere) => invoke<void>("set_pending_count", { count, elsewhere }),
  decide: (root, action, target) => invoke<DecideReport>("decide", { root, action, target }),
  setReadOnly: (on) => invoke<EditorConfig>("set_read_only", { on }),
  addToWorkspace: (root) => invoke<EditorConfig>("add_to_workspace_cmd", { root }),
  removeFromWorkspace: (root, nested, stopCapture) => invoke<WorkspaceRemoved>("remove_from_workspace", { root, nested, stopCapture }),
  getSettings: (root) => invoke<SettingsView>("get_settings", { root }),
  setSetting: (key, value, root) => invoke<SettingsView>("set_setting", { key, value, root }),
  setHookLog: (on, root) => invoke<SettingsView>("set_hook_log", { on, root }),
  setExtraHide: (root, patterns, viewRoot) => invoke<SettingsView>("set_extra_hide", { root, patterns, viewRoot: viewRoot ?? null }),
  setProjectExclude: (root, globs) => invoke<SettingsView>("set_project_exclude", { root, globs }),
  takeBackExclude: (root) => invoke<SettingsView>("take_back_exclude", { root }),
  addProject: (path) => invoke<AddedProject>("add_project", { path }),
  removeProject: (root) => invoke<RemovedProject>("remove_project", { root }),
  projectInVsCode: (root) => invoke<boolean>("project_in_vs_code", { root }),
  pickFolder: () => invoke<string | null>("pick_folder"),
  captureStatus: (recheck) => invoke<CaptureView>("capture_status", { recheck }),
  setUpCapture: () => invoke<InstallView>("set_up_capture"),
  updateCaptureHook: () => invoke<HookReplacedView>("update_capture_hook"),
  checkUpdate: () => invoke<UpdateView | null>("check_update"),
  installUpdate: () => invoke<void>("install_update"),
  gitlogRepos: (refresh) => invoke<RepoView[]>("gitlog_repos", { refresh }),
  gitlogConfig: () => invoke<GitLogConfig>("gitlog_config"),
  gitlogTick: (ids) => invoke<GitLogConfig>("gitlog_tick", { ids }),
  gitlogUntick: (ids) => invoke<GitLogConfig>("gitlog_untick", { ids }),
  gitlogShowAll: () => invoke<GitLogConfig>("gitlog_show_all"),
  gitlogShowOnly: (ids) => invoke<GitLogConfig>("gitlog_show_only", { ids }),
  gitlogProfileApply: (name) => invoke<GitLogConfig>("gitlog_profile_apply", { name }),
  gitlogProfileSave: (name) => invoke<GitLogConfig>("gitlog_profile_save", { name }),
  gitlogProfileRename: (from, to) => invoke<GitLogConfig>("gitlog_profile_rename", { from, to }),
  gitlogProfileDelete: (name) => invoke<GitLogConfig>("gitlog_profile_delete", { name }),
  gitlogScanDepth: (depth) => invoke<GitLogConfig>("gitlog_scan_depth", { depth }),
  gitlogExclude: (glob) => invoke<GitLogConfig>("gitlog_exclude", { glob }),
  gitlogInclude: (glob) => invoke<GitLogConfig>("gitlog_include", { glob }),
  gitlogPane: (pane, width) => invoke<GitLogConfig>("gitlog_pane", { pane, width }),
  gitlogLastFilter: (filter) => invoke<GitLogConfig>("gitlog_last_filter", { filter }),
  gitlogPage: (queryId, repos, filter, more) => invoke<PageView>("gitlog_page", { queryId, repos, filter, more }),
  gitlogMessage: (repo, sha) => invoke<string>("gitlog_message", { repo, sha }),
  gitlogWebUrl: (repo, sha) => invoke<string | null>("gitlog_web_url", { repo, sha }),
  gitlogOpenRemote: (repo, sha) => invoke<boolean>("gitlog_open_remote", { repo, sha }),
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
export const onLaunch = async (handler: (t: LaunchTarget) => void) => (await api()).onLaunch(handler);
export const launchPath = async () => (await api()).launchPath();
export const getConfig = async () => (await api()).getConfig();
export const setEditor = async (template: string) => (await api()).setEditor(template);
export const openInEditor = async (path: string, line: number, col = 1, template: string | null = null) =>
  (await api()).openInEditor(path, line, col, template);
export const setPendingCount = async (count: number, elsewhere = 0) => (await api()).setPendingCount(count, elsewhere);
export const decide = async (root: string, action: DecideAction, target: DecideTarget) =>
  (await api()).decide(root, action, target);
export const setReadOnly = async (on: boolean) => (await api()).setReadOnly(on);
export const addToWorkspace = async (root: string) => (await api()).addToWorkspace(root);
export const removeFromWorkspace = async (root: string, nested: string[], stopCapture: boolean) =>
  (await api()).removeFromWorkspace(root, nested, stopCapture);
export const getSettings = async (root: string | null) => (await api()).getSettings(root);
export const setSetting = async (key: SettingKey, value: unknown, root: string | null) =>
  (await api()).setSetting(key, value, root);
export const setHookLog = async (on: boolean, root: string | null) => (await api()).setHookLog(on, root);
export const setExtraHide = async (root: string, patterns: string[], viewRoot?: string) =>
  (await api()).setExtraHide(root, patterns, viewRoot);
export const takeBackExclude = async (root: string) => (await api()).takeBackExclude(root);
export const setProjectExclude = async (root: string, globs: string[] | null) =>
  (await api()).setProjectExclude(root, globs);
export const addProject = async (path: string) => (await api()).addProject(path);
export const removeProject = async (root: string) => (await api()).removeProject(root);
export const pickFolder = async () => (await api()).pickFolder();
export const projectInVsCode = async (root: string) => (await api()).projectInVsCode(root);
export const captureStatus = async (recheck = false) => (await api()).captureStatus(recheck);
export const setUpCapture = async () => (await api()).setUpCapture();
export const updateCaptureHook = async () => (await api()).updateCaptureHook();
export const checkUpdate = async () => (await api()).checkUpdate();
export const installUpdate = async () => (await api()).installUpdate();

export const gitlogRepos = async (refresh = false) => (await api()).gitlogRepos(refresh);
export const gitlogConfig = async () => (await api()).gitlogConfig();
export const gitlogTick = async (ids: string[]) => (await api()).gitlogTick(ids);
export const gitlogUntick = async (ids: string[]) => (await api()).gitlogUntick(ids);
export const gitlogShowAll = async () => (await api()).gitlogShowAll();
export const gitlogShowOnly = async (ids: string[]) => (await api()).gitlogShowOnly(ids);
export const gitlogProfileApply = async (name: string) => (await api()).gitlogProfileApply(name);
export const gitlogProfileSave = async (name: string) => (await api()).gitlogProfileSave(name);
export const gitlogProfileRename = async (from: string, to: string) => (await api()).gitlogProfileRename(from, to);
export const gitlogProfileDelete = async (name: string) => (await api()).gitlogProfileDelete(name);
export const gitlogScanDepth = async (depth: number) => (await api()).gitlogScanDepth(depth);
export const gitlogExclude = async (glob: string) => (await api()).gitlogExclude(glob);
export const gitlogInclude = async (glob: string) => (await api()).gitlogInclude(glob);
export const gitlogPane = async (pane: string, width: number) => (await api()).gitlogPane(pane, width);
export const gitlogLastFilter = async (filter: Record<string, unknown> | null) => (await api()).gitlogLastFilter(filter);
export const gitlogPage = async (queryId: number, repos: string[], filter: FilterView, more: boolean) =>
  (await api()).gitlogPage(queryId, repos, filter, more);
export const gitlogMessage = async (repo: string, sha: string) => (await api()).gitlogMessage(repo, sha);
export const gitlogWebUrl = async (repo: string, sha: string) => (await api()).gitlogWebUrl(repo, sha);
export const gitlogOpenRemote = async (repo: string, sha: string) => (await api()).gitlogOpenRemote(repo, sha);
