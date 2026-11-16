# ClaudeGate Desktop — Decisions (Accept / Reject) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the maintainer accept, reject, revert, reapply and clear Claude's edits from ClaudeGate Desktop. This works per file, per folder, and for all files at once (the maintainer asked for **Accept all / Reject all**). Every write follows the same protocol as the extension, so the app, the hook and VS Code never corrupt each other's work.

**Architecture:** New core modules:
- `store` owns the session write protocol: lock, re-read, merge, atomic write, backup.
- `merge` ports the extension's pure decision functions.
- `decide` applies an action to a project across all its scopes and restores workspace files on reject.

One Tauri command, `decide`, exposes them, and writes are serialised behind one mutex. The UI adds row, folder and bulk actions, keys `a` and `r`, a result bar with Undo, and the read-only banner.

**Tech Stack:** As in milestone 1–2: Rust core, Tauri 2, Svelte 5, Vitest. Crate addition: `rand` for temp-file names (exact pin).

**Spec:** `docs/superpowers/specs/2026-11-13-desktop-reviewer-design.md` — milestone 3 ("Decisions"), section "Protecting production" rules 1–4 and 8. The read-only viewer plan is `docs/superpowers/plans/2026-11-13-desktop-viewer.md`.

## Global Constraints

- **Reference behaviour.** The extension at tag `v1.16.1` (`~/Documents/src/personal/claudegate`, read-only) is the reference. `src/reviewModel.ts` provides `mergeFreshCaptures`, `MergeGuards`, `acceptEntry`, `rejectEntry` and `capAcceptedLog`. `src/sessionManager.ts` provides `acceptFile/Folder/All`, `applyReject`, `rejectFile/Folder/All`, `revertAccepted*`, `reapply*`, `clearAccepted/Rejected`, `persist`, `acquireLock` and `atomicWrite`. Semantics match these line for line.
- **Lock.** The lock is `<session file>.lock`, created with `O_CREAT|O_EXCL`. The writer puts its pid in it. Wait up to 200 ms, polling every 5 ms. A lock older than 3000 ms (by mtime) is stolen. On timeout, proceed unlocked (fail-open, like the extension).
- **Session write.**
  1. Re-read the file from disk.
  2. `parse_session` and migrate it.
  3. `merge_fresh_captures(mine, disk, guards)`.
  4. Set `status` = `reviewed` if `files` is empty, otherwise `active`.
  5. Pretty-print as JSON with 2-space indent.
  6. Atomic write: `<target>.<12 hex>.tmp`, then rename.
- **Atomic write of workspace files.** This applies to reject restore and reapply. Follow a symlink to its target and preserve the existing file's mode. A new file gets the default mode and its parent dirs are created.
- **Writes are refused** with `DecideError::ReadOnly(reason)` when:
  - the scope's session shape is `Unknown` ("This project was written by a newer ClaudeGate. Update the app to make decisions here."), or
  - the global switch `readOnly` is `true` in `~/.claudegate-desktop/config.json`. The default is `false`.
- **Backup.** Before every session write, copy the current session file to `~/.claudegate-desktop/backups/<session hash>/<UTC yyyymmddThhmmss.mmmZ>.json` and keep the newest 50 per hash. If the backup fails, the write is refused (`DecideError::Backup`). A missing session file needs no backup.
- **Bulk and folder actions** (accept and reject) skip entries that are excluded or have no real change (`has_real_change` false). This includes "waiting for write" rows, as the extension does with `hasRealPendingChange`. Single-file actions apply to any pending entry.
- **Reject on disk:**
  - `originalContent != null` → restore it.
  - `originalContent == null && newFile == true` → delete the file.
  - otherwise → leave the file on disk and report it as `left`.
  - A failed restore records nothing for that file and reports an error.
- **Writers allowed.** Only these files may contain write APIs, enforced by `core/tests/no_writes.rs`:
  - `core/src/store.rs`: session file, lock, temp, backups
  - `core/src/workspace_write.rs`: workspace files
  - `src-tauri/src/config.rs`
  `store.rs` refuses any path not under `<ClaudeGate home>/sessions/` or the backups dir. `workspace_write.rs` refuses any path that isn't a key of the session being decided.
- **Concurrency.** All writes run on the blocking pool behind one `Mutex<()>` in Tauri state, so the app never races itself.
- **Exact pins, no network, Svelte 5 runes, theme tokens.** The UI follows the taste-skill app-screen rules already applied.

## Review Focus

1. **The hook writes the session between the app's read and its write.** That capture must survive. Task 2 adds a merge case; Task 3 adds a test that writes a disk-side capture after load and before persist.
2. **VS Code accepts the same file at the same moment.** Neither decision is lost (union by id), and no file shows up both in Pending and in a log. Task 2 covers this with a merge case.
3. **A reject whose restore fails** (permission denied, read-only file) leaves that entry pending, reports the error, and still applies the other files in a bulk action. Covered in Task 3.
4. **Reject all on 50+ files with some new files**, where Claude-created files are deleted and the rest restored. The result bar shows done / left / errors. Reject all must ask for confirmation first. Covered in Tasks 3 and 5.
5. **A session with an unknown shape, or the global read-only switch on.** No action is shown and none can be invoked: the command refuses even if the UI is bypassed. Covered in Tasks 3, 4 and 5.

---

### Task 1: Store: lock, atomic write, backup, persist

**Files:**
- Create: `core/src/store.rs`
- Modify: `core/src/lib.rs`, `core/Cargo.toml` (`rand`)
- Test: `core/tests/store.rs`

**Interfaces:**
- Consumes: `paths::{session_file_for, claudegate_home}`, `model::{parse_session, Session, Shape}`.
- Produces:
  - `pub struct Lock` (released on Drop) and `pub fn acquire_lock(session_file: &Path) -> Option<Lock>`
  - `pub fn atomic_write(target: &Path, content: &str, preserve_existing: bool) -> io::Result<()>` (crate-private helper reused by `workspace_write`)
  - `pub fn backup(home_desktop: &Path, session_file: &Path) -> Result<(), StoreError>`
  - `pub fn persist(paths: &StorePaths, mine: Session, guards: &MergeGuards) -> Result<Session, StoreError>`, which runs lock → re-read → merge → status → backup → atomic write and returns what was written
  - `pub struct StorePaths { pub home: PathBuf, pub desktop_home: PathBuf, pub session_file: PathBuf }`
  - `pub enum StoreError { Backup(String), Write(String), OutsideHome }`

- [ ] **Step 1: Failing tests**:
  - `lock_excludes_second_writer`: the second acquire waits about 200 ms, then returns `None`.
  - `stale_lock_is_stolen`: a lock with mtime now−4 s is taken.
  - `atomic_write_preserves_mode_and_follows_symlink`.
  - `atomic_write_leaves_no_tmp_on_error`.
  - `backup_keeps_newest_50`: write 55 backups and assert 50 remain, all the newest.
  - `persist_refuses_path_outside_sessions`.
  - `persist_sets_status_reviewed_when_files_empty`.
  - `persist_writes_pretty_json_two_space`.
  - Review Focus 1, `persist_keeps_capture_written_after_load`: load a session; write a disk session with an extra pending entry; persist `mine`; the result contains both.
- [ ] **Step 2:** `cargo test -p claudegate-core --test store` → FAIL.
- [ ] **Step 3: Implement.** Use the values from Global Constraints. `persist` holds the lock across the read and the write.
- [ ] **Step 4:** PASS.
- [ ] **Step 5: Commit** `feat(core): session store with the extension's lock, merge and atomic write`.

### Task 2: Merge and decision functions, cross-checked against the extension

**Files:**
- Create: `core/src/merge.rs`, `fixtures/merge/*.json`
- Modify: `xcheck/run.ts`
- Test: `core/tests/merge.rs`

**Interfaces:**
- Produces:
  - `pub struct MergeGuards { pub pruned_files: HashMap<String, Option<String>>, pub dropped_accepted_ids: HashSet<String>, pub dropped_rejected_paths: HashSet<String> }` (Default)
  - `pub fn merge_fresh_captures(mine: Session, disk: Session, guards: &MergeGuards) -> Session`
  - `pub fn accept_entry(s: &mut Session, path: &str, after: Option<String>, decided_at: &str)`
  - `pub fn reject_entry(s: &mut Session, path: &str, after: Option<String>, decided_at: &str)` (no reason in v1)
  - Reuse the existing accepted-log cap in `model.rs` (500, oldest dropped). Keep one implementation, and make it `pub` if `merge.rs` needs it.

- [ ] **Step 1: Fixtures.** Each fixture is `{ "mine", "disk", "guards", "expected" }`. Generate `expected` by running the real TS `mergeFreshCaptures` at v1.16.1, as milestone 1 did. Cases:
  1. A new disk capture is merged.
  2. Disk capture older than our accept → dropped.
  3. Capture time equal to our decision → kept.
  4. Disk accepted record unioned and sorted oldest-first.
  5. A union pushing the log past 500 → oldest dropped.
  6. Disk reject newer than ours → taken.
  7. `dropped_accepted_ids` tombstone respected.
  8. `dropped_rejected_paths` respected.
  9. `pruned_files` exact capturedAt respected; a newer capturedAt still merges.
  10. Our pending entry decided on disk later → removed (Review Focus 2).
  11. `excludeCleanupDone` latch.
  12. A capture with no capturedAt is ignored.
  
  Plus `accept_entry` and `reject_entry` fixtures: accept removes any prior reject; a reject record carries `newFile` and `sessionId`.
- [ ] **Step 2:** `cargo test -p claudegate-core --test merge` → FAIL.
- [ ] **Step 3: Implement** a port of `mergeFreshCaptures` and `latestDecisionMs`. JS `Date.parse` semantics use `time::parse_iso_ms`. A NaN comparison means "unknown", exactly as in the TS.
- [ ] **Step 4:** PASS. Then extend `xcheck/run.ts` to run every `fixtures/merge/*.json` case through the extension's `mergeFreshCaptures`, `acceptEntry` and `rejectEntry`. Run `cd xcheck && npx tsx run.ts` and expect `ok - … merge cases`.
- [ ] **Step 5: Commit** `feat(core): port the extension's merge and decision functions, cross-checked`.

### Task 3: Decide: actions across scopes, workspace file writes, read-only gate

**Files:**
- Create: `core/src/decide.rs`, `core/src/workspace_write.rs`
- Modify: `core/tests/no_writes.rs` (ALLOWED = `core/src/store.rs`, `core/src/workspace_write.rs`, `src-tauri/src/config.rs`)
- Test: `core/tests/decide.rs`

**Interfaces:**
- Consumes: Tasks 1–2; `project::load_project` scopes; `excludes::ExcludeMatcher` (anchored at the primary root, as in milestone 1).
- Produces:
  - `pub enum Action { Accept, Reject, Revert, Reapply, ClearAccepted, ClearRejected }`
  - `pub enum Target { File { scope: String, key: String }, Folder { scope: String, prefix: String }, All }`. `key` is a path for Accept, Reject and Reapply, and a record id for Revert.
  - `pub struct DecideReport { pub done: usize, pub left: Vec<String>, pub errors: Vec<(String, String)> }` (Serialize, camelCase)
  - `pub enum DecideError { ReadOnly(String), NotFound, Store(StoreError) }`
  - `pub fn decide(paths: &DecidePaths, root: &Path, action: Action, target: Target, read_only: bool, now_iso: &str) -> Result<DecideReport, DecideError>`
  - `pub fn restore(path: &Path, content: &str) -> io::Result<()>`, `pub fn delete(path: &Path) -> io::Result<()>` in `workspace_write.rs`

- [ ] **Step 1: Failing tests** (temp ClaudeGate home and temp workspace). Each one also asserts the session JSON on disk afterwards:
  - accept a file: a record with `after` equal to the disk content; the entry leaves `files`; status `reviewed` when it was the last.
  - reject restores the original; reject deletes a `newFile`; reject leaves a non-confident new file and reports it in `left`.
  - Review Focus 3: a restore into a read-only file → that entry is still pending, the error is reported, and the other files in the same Reject all are still done.
  - Folder accept only touches paths under the prefix.
  - `All` spans the primary and worktree scopes, skips excluded entries and no-op ("waiting") entries.
  - Revert reopens as pending only when disk equals the record's `after`; otherwise it only drops the record (as the TS does).
  - Reapply writes `after` back and reopens the entry; `after == None` is an error.
  - ClearAccepted and ClearRejected tombstone ids and paths, so a concurrent disk copy doesn't resurrect them.
  - Review Focus 5: `read_only: true` → `ReadOnly`, and no file anywhere changes (snapshot the sandbox before and after). An Unknown-shape scope → `ReadOnly` with the spec's exact message.
  - Every write produced a backup.
- [ ] **Step 2:** FAIL.
- [ ] **Step 3: Implement.**
  - Group targets by scope.
  - For each scope: load the session (refuse if the shape is Unknown), apply the mutations and workspace writes in memory order, then `store::persist` with the guards the TS uses for that action.
  - Accept and reject: guard `dropped_rejected_paths` with the accepted paths, like `acceptFile`. Revert: `dropped_accepted_ids`. Reapply and ClearRejected: `dropped_rejected_paths`. ClearAccepted: `dropped_accepted_ids`.
  - `workspace_write` refuses a path that isn't a key in that scope's session.
- [ ] **Step 4:** PASS. `cargo test -p claudegate-core` is green, including `no_writes` with the new allow-list.
- [ ] **Step 5: Commit** `feat(core): decide — accept, reject, revert, reapply, clear across scopes`.

### Task 4: Tauri command, write mutex, read-only switch

**Files:**
- Modify: `src-tauri/src/commands.rs`, `src-tauri/src/config.rs` (`readOnly: bool`, default false), `src-tauri/src/lib.rs`
- Test: `src-tauri/tests/decide_cmd.rs`

**Interfaces:**
- Produces Tauri commands:
  - `decide(root: String, action: String, target: TargetDto) -> Result<DecideReport, String>`. `action` is one of `"accept" | "reject" | "revert" | "reapply" | "clearAccepted" | "clearRejected"`. `TargetDto = { kind: "file" | "folder" | "all", scope?: string, key?: string, prefix?: string }`.
  - `set_read_only(on: bool) -> Config`
  - `get_config` now includes `readOnly`.
- After a successful decide, emit `claudegate://changed` with `{ roots: [root] }` straight away, without waiting for the watcher.

- [ ] **Step 1: Failing tests:**
  - an unknown action string → Err;
  - a folder target without a prefix → Err;
  - `readOnly` in config → Err containing "read-only" and the sandbox unchanged;
  - two concurrent `decide` calls on one project → both applied, one after the other (the mutex);
  - `set_read_only` persists the flag and `save_editor` keeps it.
- [ ] **Step 2:** FAIL. **Step 3:** Implement. Run on `spawn_blocking` holding `State<WriteLock>`. **Step 4:** PASS.
- [ ] **Step 5: Commit** `feat(app): decide command behind one write lock; read-only switch`.

### Task 5: UI: actions, bulk with confirm, keys, result bar with Undo, read-only banner

**Files:**
- Create: `ui/src/components/ActionBar.svelte`, `ui/src/components/ResultBar.svelte`, `ui/src/lib/decide.ts`
- Modify: `ui/src/lib/api.ts` (+ mock), `ui/src/components/FileTree.svelte`, `ui/src/App.svelte`, `ui/src/lib/keys.ts`
- Test: `ui/src/lib/decide.test.ts`, `ui/src/lib/keys.test.ts`

**Interfaces:**
- Consumes: the Task 4 commands.
- Produces: `undoFor(action, target, report) -> { action, target } | null`. Accept is undone by Revert of the new record. Reject is undone by Reapply of the same path. Revert, Reapply and Clear have no undo. `nextAfterDecision(rows, decidedKey) -> key | null` picks the next pending row, else the previous one, else null.

- [ ] **Step 1: Failing tests:**
  - `undoFor` for each action;
  - `nextAfterDecision` for the first, middle and last row and a single row;
  - keys: `a` → accept the selected pending row; `r` → reject; both ignored on the Accepted and Rejected tabs, while typing, while the menu is open, and in read-only mode.
- [ ] **Step 2:** FAIL. **Step 3: Implement.**
  - **Pending rows:** ✓ and ✗ icon buttons on hover or focus. They are always visible on the selected row, matching the extension's inline actions.
  - **Folder rows:** "Accept folder" and "Reject folder".
  - **Accepted rows:** "Revert". **Rejected rows:** "Reapply".
  - **ActionBar** above the tree, per tab:
    - Pending: **Accept all** and **Reject all**. Reject all asks for confirmation inline: "Reject N files? Originals are restored and files Claude created are deleted." with buttons [Reject N] [Cancel]. Accept all needs no confirmation.
    - Accepted: Revert all and Clear.
    - Rejected: Reapply all and Clear.
    - Clear asks for confirmation.
    - The N counts only files the action will touch: real changes, not excluded.
  - **After a decision**, select `nextAfterDecision` and show the next diff (auto-advance, like the extension).
  - **ResultBar:** a slim bar at the bottom of the diff pane, e.g. "Accepted client.ts" with [Undo], or "Rejected 12 · 2 left on disk · 1 failed" with [Details]. It auto-hides after 6 s. Undo calls `undoFor`.
  - **Read-only:** when the project has an Unknown shape scope or `readOnly` is on, hide every action. Show a banner with the spec's text, or "Read-only mode is on" with a [Turn off] button that calls `set_read_only(false)`. The header's "Read-only" chip becomes a toggle.
- [ ] **Step 4:** Tests PASS. `check` and `build` pass. In the browser preview, screenshot: the pending row actions, the Reject all confirmation, the ResultBar with Undo, and the read-only banner.
- [ ] **Step 5: Commit** `feat(ui): accept/reject per file, folder and all, with undo and read-only mode`.

### Task 6: Real-data verification with the extension running

**Files:**
- Modify: `README.md` (decisions, backups, read-only switch)

- [ ] **Step 1:** A sandbox end-to-end script, `scripts/decide-e2e.sh`, run headless. It seeds a sandbox, runs `hook.py` (from `.xcheck-cache`) to capture an edit while the app is open, then accepts through the `decide` command in a small Rust example. It asserts that the hook's second capture made during the accept survives (Review Focus 1).
- [ ] **Step 2:** With the maintainer, on the real machine:
  1. In a scratch git repo registered in VS Code, have Claude edit 3 files.
  2. Accept one in the app; VS Code's panel shows it in Accepted within about 1 s.
  3. Reject one in the app; the file is restored on disk.
  4. Accept the third in VS Code; the app shows it.
  5. Run Undo on the reject; the file holds Claude's version again.
  6. Check `~/.claudegate-desktop/backups/` has one backup per write.
- [ ] **Step 3: Commit** `docs: decisions, backups and read-only switch`.
