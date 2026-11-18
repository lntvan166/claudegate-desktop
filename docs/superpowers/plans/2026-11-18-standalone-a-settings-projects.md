# ClaudeGate Desktop — Standalone A: Settings, Projects, Exclude, Upkeep

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the app usable without the VS Code extension. It gets its own settings, which fall back to the extension's when unset. It can add and remove projects, so the hook captures there. It owns exclude for the projects it added. It runs upkeep for those projects.

**Architecture:**
- New core modules:
  - `settings` resolves each setting: app value, then extension value, then extension default.
  - `jsonc` is a read-only reader for VS Code settings files.
  - `roots_write` and the excludes writer live in `store.rs`, because that file is already the allowed writer.
  - `upkeep` ports the extension's session housekeeping.
- Tauri commands expose them.
- The UI adds a ⚙ Settings screen and Add/Remove project controls.

**Tech Stack:** As before. New: `tauri-plugin-dialog` (exact pin) for the folder picker; its capability is limited to `dialog:allow-open`.

**Spec:** `docs/superpowers/specs/2026-11-13-desktop-reviewer-design.md`, rules 5–7 plus "Addendum (2026-11-18): standalone app".

## Global Constraints

- **The extension stays read-only.** Never write `~/.config/Code/**`, `~/.config/Cursor/**`, or any `.vscode/settings.json`.
- **Shared files the app may write:**
  - `~/.claudegate/workspace-roots.json`: append and re-add only. Never remove a root the app did not add.
  - `~/.claudegate/excludes.json`: only the keys of roots the app added and still owns.
  - `~/.claudegate/hooklog.enabled`: create or delete.
  - The session files, through the existing store protocol.
- **Writers allowed by `no_writes`:** `core/src/store.rs`, `core/src/workspace_write.rs`, `src-tauri/src/config.rs`. The allow-list does not grow.
- **Shared-file writes are atomic and merge with what is on disk:**
  - write to `<file>.<12hex>.tmp`, then rename;
  - re-read immediately before writing;
  - keep every key and entry the app does not own.
- **Extension defaults, copied verbatim from tag v1.16.1** (`src/excludeMatcher.ts` `DEFAULT_EXCLUDES`/`DEFAULT_PROTECTED`, `package.json` defaults). `xcheck` asserts they still match:
  - `autoAdvance`: true
  - `groupBySession`: false
  - `pendingAge.minDays`: 0
  - `worktrees.maxAttached`: 256
  - `hookLog.enabled`: false
- **Upkeep constants, from v1.16.1:**
  - `NOOP_SETTLE_MS` 15000, `NEW_FILE_ABSENT_MS` 45000
  - `SESSION_SIZE_WARN_BYTES` 2_000_000, `MAX_ACCEPTED_BYTES` 1_500_000, `MAX_ACCEPTED_RECORDS` 500
- **Upkeep scope:** it runs only for roots the app added and that the app still owns.
- **Config:** existing keys stay as they are. New keys:
  - `settings: { [key]: value }`
  - `projects: { [root]: { addedByApp: true, exclude?: string[], excludeWritten?: string[], managedByVsCode?: bool } }`
- **Tests** use temp homes only.

## Review Focus

1. A VS Code `settings.json` with comments, trailing commas, or a parse error gets these values from the extension's defaults, and never crashes. (Task 1)
2. VS Code opens a project the app added, and the extension then rewrites `excludes.json[root]`. The app must notice the takeover and stop writing that key. (Task 4)
3. Adding a folder that is nested inside, or contains, an existing root is refused. The message names the conflicting root. (Task 3)
4. The extension's `workspace-roots.json` write drops a root the app added. The app re-adds it, and never duplicates it. (Task 3)
5. Upkeep must never prune a waiting entry inside its settle window, and must never touch a VS Code root. (Task 5)

---

### Task 1: Settings resolver with extension fallback

**Files:**
- Create: `core/src/settings.rs`, `core/src/jsonc.rs`
- Test: `core/tests/settings.rs`, `fixtures/settings/*.jsonc`

**Interfaces:**
- Produces:
  - `pub enum Source { App, VsCode, Cursor, Workspace, Default }`
  - `pub struct Resolved<T> { pub value: T, pub source: Source }`
  - `pub struct Effective { pub exclude: Resolved<Vec<String>>, pub protected: Resolved<Vec<String>>, pub auto_advance: Resolved<bool>, pub group_by_session: Resolved<bool>, pub pending_age_min_days: Resolved<u32>, pub max_attached: Resolved<u32>, pub hook_log: Resolved<bool> }` (Serialize, camelCase)
  - `pub fn effective(app: &AppSettings, root: Option<&Path>, sources: &ExtSources) -> Effective`
  - `pub struct ExtSources { pub vscode_user: Option<PathBuf>, pub cursor_user: Option<PathBuf>, pub excludes_json: PathBuf, pub hooklog_sentinel: PathBuf }`
  - `pub fn default_ext_sources(home: &Path) -> ExtSources`
  - `pub fn parse_jsonc(text: &str) -> Option<serde_json::Value>` (handles `//` and `/* */` comments and trailing commas; returns `None` on error)

- [ ] **Step 1: Write failing tests.**
  - Precedence:
    - app value set → `Source::App`;
    - else a project `.vscode/settings.json` `claudegate.*` → `Workspace`;
    - else VS Code user → `VsCode`;
    - else Cursor user → `Cursor`;
    - else → `Default`.
  - Exclude, for a root:
    - app `projects[root].exclude`;
    - else `excludes.json[root]` → `VsCode`;
    - else the settings files' `claudegate.exclude` map (keep only keys mapped to `true`);
    - else `DEFAULT_EXCLUDES`.
  - `hook_log` reads the sentinel's existence.
  - Settings files that are malformed, missing, or not an object → fall back, no panic.
  - JSONC: comments, trailing commas, `//` inside strings preserved, nested objects.
- [ ] **Step 2:** FAIL. **Step 3:** Implement. Keep it read-only; no write APIs. **Step 4:** PASS.
- [ ] **Step 5:** Extend `xcheck/run.ts` to assert the copied defaults equal v1.16.1's `DEFAULT_EXCLUDES`, `DEFAULT_PROTECTED`, and the `package.json` defaults for these keys.
- [ ] **Step 6: Commit** `feat(core): settings resolve app → extension → default`.

### Task 2: Settings screen, and the settings applied

**Files:**
- Modify: `src-tauri/src/config.rs` (the `settings` map; `set_setting(key, value | null)`), `src-tauri/src/commands.rs` (`get_settings(root?) -> Effective`, `set_setting`, `set_hook_log(on)`), `src-tauri/src/lib.rs`
- Modify: `core/src/store.rs` (`pub(crate) fn set_sentinel(path, on)`, guarded to `<home>/hooklog.enabled`)
- Create: `ui/src/components/Settings.svelte`, `ui/src/lib/settings.ts`
- Modify: `ui/src/App.svelte` (⚙ in the header, settings applied), `FileTree.svelte` (protected 🔒 marker), `ui/src/lib/mock.ts`
- Test: `src-tauri/tests/settings_cmd.rs`, `ui/src/lib/settings.test.ts`

**Interfaces:**
- Commands:
  - `get_settings(root: Option<String>) -> Effective`
  - `set_setting(key: String, value: serde_json::Value) -> Effective`. `null` means "reset to inherited". Keys: `autoAdvance`, `groupBySession`, `pendingAgeMinDays`, `maxAttached`, `protected`. An unknown key returns Err.
  - `set_hook_log(on: bool) -> Effective`
- [ ] **Step 1: Failing tests.**
  - `set_setting` round-trips and keeps unknown keys and the editor, readOnly and hiddenProjects fields.
  - A wrong value type returns Err.
  - `set_hook_log` creates and deletes only `<home>/hooklog.enabled`.
  - UI `settings.ts`: `sourceLabel(source)` returns "This app" / "From VS Code" / "From Cursor" / "From this project's .vscode" / "Default".
- [ ] **Step 2:** FAIL. **Step 3: Implement.**
  - **Settings screen** (⚙ opens a panel that replaces the diff pane; Esc closes it):
    - one row per setting, showing the value control, a source badge, and Override or Reset;
    - the exclude list for the selected project, with the source and an editable list when the app owns it; otherwise "Managed by VS Code" and an "Extra patterns (hide in app only)" list, stored in `projects[root].extraHide`;
    - Hook log, Editor, Read-only, Hidden projects.
  - **Applying the settings:**
    - **autoAdvance:** turns off the auto-advance after a decision.
    - **groupBySession:** groups the tree by the entry's `sessionId`, labelled with that session's first `capturedAt`.
    - **pendingAge.minDays:** shows "3d" on rows at least N days old.
    - **maxAttached:** passed to the worktree scan.
    - **protected:** shows 🔒 on matching rows. Accepting a protected file asks inline: "This looks like a secret file (.env). Accept anyway?"
- [ ] **Step 4:** PASS. Run check, build and the tauri debug build. Take browser-preview screenshots on port 1437 into `/tmp/claude-1000/-home-tuvan-Documents-src-personal-claudegate/d4cebbb5-5a79-43a0-bee6-0aa890ce3b9f/scratchpad/standalone/`.
- [ ] **Step 5: Commit** `feat: settings screen with extension fallback; settings applied`.

### Task 3: Add and remove projects

**Files:**
- Modify: `core/src/store.rs` (`pub(crate) fn roots_add(home, root) -> Result<(), RootsError>`, `pub(crate) fn roots_remove(home, root) -> Result<(), RootsError>`), `core/src/roots.rs` (the pure guard `check_new_root(existing, candidate) -> Result<(), RootsError>`)
- Create: `core/src/projects.rs` (`add_project`, `remove_project`, `ensure_app_roots_present`)
- Modify: `src-tauri` (commands `add_project(path)`, `remove_project(root)`, `pick_folder()` via tauri-plugin-dialog), `src-tauri/capabilities/default.json` (`dialog:allow-open` only), `ui` (rail "+ Add project", "Remove" on app-added projects, "added here" / "from VS Code" marker)
- Test: `core/tests/projects.rs`, `src-tauri/tests/projects_cmd.rs`

**Interfaces:**
- `pub enum RootsError { Nested { existing: String }, Contains { existing: String }, NotADirectory, NotAbsolute, NotAppAdded, Io(String) }`
- `add_project(home, desktop_cfg, path)`:
  1. resolve the path;
  2. check that it is an absolute directory;
  3. `check_new_root`;
  4. `roots_add`;
  5. record `projects[root].addedByApp = true`;
  6. write the default exclude for the new root (Task 4's writer).
- `remove_project` works only when `addedByApp`. It removes the root from `workspace-roots.json` and `excludes.json[root]`, keeps the session file, and drops the app's `projects[root]`.
- `ensure_app_roots_present(home, cfg)` re-adds missing app roots. It is called on every `workspace-roots.json` watch event.
- [ ] **Step 1: Failing tests.**
  - An add appends without reordering and without dropping roots the app didn't add.
  - The nested and contains cases are refused, and the error names the conflicting root.
  - A git worktree under a root is refused, with a hint that worktrees are found automatically.
  - A relative path or a file is refused.
  - Removing a VS Code root returns `NotAppAdded`.
  - A dropped app root is re-added exactly once.
  - Concurrent rewrites by the extension are simulated: write the file between read and rename. The merge keeps both.
- [ ] **Step 2:** FAIL. **Step 3:** Implement. **Step 4:** PASS, and `no_writes` passes unchanged.
- [ ] **Step 5: Commit** `feat: add and remove projects so the hook captures there`.

### Task 4: Exclude ownership for app-added roots

**Files:**
- Modify: `core/src/store.rs` (`pub(crate) fn excludes_set(home, root, globs) -> Result<(), StoreError>`, `pub(crate) fn excludes_remove(home, root)`), `core/src/projects.rs` (`sync_excludes(home, cfg) -> Vec<RootChange>`)
- Modify: `src-tauri` (call `sync_excludes` on excludes.json and config change events; command `set_project_exclude(root, globs)`)
- Test: `core/tests/excludes_owner.rs`

**Interfaces:**
- On each sync, for every root with `addedByApp && !managedByVsCode`:
  - if `excludes.json[root]` equals `excludeWritten` (or is absent): write the effective exclude, then record `excludeWritten`;
  - otherwise: set `managedByVsCode = true`, stop writing, and surface it in the UI.
- [ ] **Step 1: Failing tests.**
  - The owner writes only its own key, and other roots' keys are kept byte-for-byte.
  - A takeover is detected: the test changes `excludes.json[root]` externally, after which the app never writes that key again.
  - Removing a project deletes only its own key.
  - The hook's real matcher honours what the app wrote: run `hook.py` from `.xcheck-cache` against a sandbox with an excluded path → `skip-excluded`.
- [ ] **Step 2:** FAIL. **Step 3:** Implement. **Step 4:** PASS.
- [ ] **Step 5: Commit** `feat: the app owns exclude for the projects it added`.

### Task 5: Upkeep for app-added roots

**Files:**
- Create: `core/src/upkeep.rs` (`pub fn upkeep(paths: &DecidePaths, root: &Path, now_ms: i64) -> UpkeepReport`)
- Modify: `core/src/model.rs` or `merge.rs` (port `capAcceptedBytes` and `shouldPruneNoOp`), `src-tauri` (run upkeep for owned roots after load and on that root's session events, debounced to at most once per 5 s per root), `xcheck` (add cases for `capAcceptedBytes` and `shouldPruneNoOp`)
- Test: `core/tests/upkeep.rs`

**Interfaces:**
- `upkeep` does all of the following through `store::persist` with guards (`pruned_files`, `dropped_accepted_ids`):
  - prunes settled no-ops (`shouldPruneNoOp`);
  - when the file is over 2_000_000 bytes, caps accepted bytes to 1_500_000;
  - runs `cleanupExcludedOnce` (sets `excludeCleanupDone`);
  - prunes entries outside the root, the same as the extension.
- Read-only mode and Unknown shape → no-op.
- It runs only for roots that are owned and app-added.
- [ ] **Step 1: Failing tests.**
  - A waiting entry younger than 15 s (or 45 s for a new file) is kept, and one older than that is pruned.
  - An oversized session is trimmed, oldest records first.
  - The excluded sweep runs once only.
  - Nothing happens for a VS Code root.
  - Nothing happens in read-only mode.
  - A hook capture landing mid-upkeep survives.
- [ ] **Step 2:** FAIL. **Step 3:** Implement. **Step 4:** PASS, and the xcheck cases match TS.
- [ ] **Step 5: Commit** `feat: upkeep for the app's own projects`.
