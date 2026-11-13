# ClaudeGate Desktop — Read-Only Viewer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship the first usable ClaudeGate Desktop build: a Tauri app that shows every registered project's pending, accepted and rejected Claude edits with file icons and a Monaco diff, live, and that cannot write to `~/.claudegate`.

**Architecture:** A new repo `claudegate-desktop`. A Rust library crate `core` reads the extension's shared files (sessions, `workspace-roots.json`, `excludes.json`) and builds view models; it has no write API at all in this plan. A Tauri crate wraps `core` in commands and a file watcher. A Svelte + Monaco frontend renders projects, trees and diffs.

**Tech Stack:** Rust (stable), Tauri 2 (`@tauri-apps/cli` 2.12.4), Svelte 5 + Vite + TypeScript, `monaco-editor` 0.57.0, `material-icon-theme` 5.40.0, Rust crates `serde`, `serde_json`, `md-5`, `regex`, `similar`, `notify`, `dirs`; Vitest for frontend tests.

**Spec:** `docs/superpowers/specs/2026-11-13-desktop-reviewer-design.md` (milestones 1 and 2). Plans for milestones 3 (decisions), 4 (projects) and 5 (packaging) come after this one lands.

## Global Constraints

- **Nothing in this plan writes under the ClaudeGate home.** No `std::fs::write`, `rename`, `remove_*`, `create_dir*` or `OpenOptions` with write/create may target a path under the resolved ClaudeGate home. The app's own state lives in `~/.claudegate-desktop/`.
- ClaudeGate home = `$CLAUDEGATE_HOME` if set, else `~/.claudegate`. Every test and every dev run uses a temp `CLAUDEGATE_HOME`.
- The extension repo (`lntvan166/claudegate`) is never modified. Its files are read from a pinned checkout of tag `v1.16.1`.
- Session file name: `md5(resolved absolute path)` hex + `.json`, path lower-cased on Windows only. `/ws/proj` → `3315a6271f371869636a76e1afc8237e`.
- Known session fields (extension v1.16.1): top level `sessionId, status, files, accepted, rejected, excludeCleanupDone`; file entry `originalContent, reviewStatus, newFile, sessionId, capturedAt, claudeContent`; record `id, path, before, after, decidedAt, sessionId, newFile, reason`.
- Worktree scan: skip `node_modules .git dist build out target vendor __pycache__ .next .nuxt .venv venv`, max depth 6, never follow symlinks, at most 256 worktrees per project.
- Monaco and icon SVGs are bundled; the app makes no network requests.
- Material Icon Theme is MIT; keep its `LICENSE` in the bundle.
- Pin every npm and crate version exactly.

## Review Focus

1. **A session file caught mid-write, empty or corrupt** → that scope shows "Couldn't read this session" and the rest of the project still renders; the next good write recovers it. (Task 4)
2. **A registered root that no longer exists on disk** → project listed as "Folder missing", no error dialog, no scan. (Task 4)
3. **A pending file deleted since capture, or binary / non-UTF-8 content** → diff shows an empty side or "Binary file, not shown", never a crash or mojibake. (Task 5)
4. **Multi-MB session files and many projects** (real machine: 53 sessions, one 1.1 MB) → a watch event re-parses only the session that changed; the UI stays responsive. (Task 6)
5. **A symlink loop or huge tree under a root** → worktree scan terminates within its depth/skip bounds. (Task 3)

---

### Task 1: Repo, toolchain, and `core` paths

**Files:**
- Create: `~/Documents/src/personal/claudegate-desktop/` (git repo), `Cargo.toml` (workspace: `core`, `src-tauri`), `core/Cargo.toml`, `core/src/lib.rs`, `core/src/paths.rs`, `docs/superpowers/specs/2026-11-13-desktop-reviewer-design.md` and `docs/superpowers/plans/2026-11-13-desktop-viewer.md` (copied from the extension repo's `docs/desktop-reviewer-design` branch), `.gitignore` (`target/`, `node_modules/`, `dist/`, `.xcheck-cache/`), `README.md` (one paragraph + "never writes in v0").
- Test: `core/src/paths.rs` (unit tests)

**Interfaces:**
- Produces: `pub fn claudegate_home() -> PathBuf`; `pub fn session_file_for(home: &Path, root: &Path) -> PathBuf`; `pub fn resolve_root(root: &Path) -> PathBuf` (lexical absolute + normalised, no symlink resolution, mirrors Node `path.resolve`).

- [ ] **Step 1: Toolchain.** Rust is not installed on the maintainer's machine. Ask the maintainer to run `! curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y` and `! pkexec apt install -y libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev build-essential libssl-dev`, then add both to bifrost's `bootstrap.sh` (global CLAUDE.md rule). Verify: `cargo --version` and `pkg-config --modversion webkit2gtk-4.1` print versions.
- [ ] **Step 2: Create the repo** (`git init`, workspace `Cargo.toml`, `core` crate, copy the two docs). `cargo build -p claudegate-core` succeeds.
- [ ] **Step 3: Write failing tests**

```rust
#[test] fn hash_matches_extension() {
    let p = session_file_for(Path::new("/h"), Path::new("/ws/proj"));
    assert_eq!(p, Path::new("/h/sessions/3315a6271f371869636a76e1afc8237e.json"));
}
#[test] fn resolve_root_normalises() {
    assert_eq!(resolve_root(Path::new("/ws/./a/../proj/")), PathBuf::from("/ws/proj"));
}
#[test] fn home_honours_env() { /* set CLAUDEGATE_HOME=/tmp/x → claudegate_home() == /tmp/x */ }
```
- [ ] **Step 4:** `cargo test -p claudegate-core` → FAIL (unresolved functions).
- [ ] **Step 5: Implement** in `core/src/paths.rs`. On `cfg(windows)` lower-case before hashing.
- [ ] **Step 6:** `cargo test -p claudegate-core` → PASS.
- [ ] **Step 7: Commit** `feat(core): ClaudeGate home and session path hashing`.

### Task 2: Session model, migration and shape check

**Files:**
- Create: `core/src/model.rs`, `fixtures/sessions/*.json` (cases below)
- Test: `core/tests/model.rs`

**Interfaces:**
- Produces:
  - `pub struct FileEntry { original_content: Option<String>, new_file: Option<bool>, session_id: Option<String>, captured_at: Option<String> }`
  - `pub struct ReviewRecord { id: String, path: String, before: Option<String>, after: Option<String>, decided_at: String, session_id: Option<String>, new_file: Option<bool>, reason: Option<String> }`
  - `pub struct Session { session_id: String, status: Status, files: BTreeMap<String, FileEntry>, accepted: Vec<ReviewRecord>, rejected: BTreeMap<String, ReviewRecord>, exclude_cleanup_done: bool }`
  - `pub enum Shape { Known, Unknown(Vec<String>) }` — the `Vec` lists offending field paths like `files./a.ts.foo`.
  - `pub fn parse_session(text: &str) -> Result<(Session, Shape), SessionError>`
  - `pub fn has_real_change(original: Option<&str>, disk: Option<&str>) -> bool`

- [ ] **Step 1: Write fixtures + failing tests.** Each fixture is `{ "input": <raw session JSON>, "expected": <Session as migrated JSON>, "shape": "known" | ["field.path", …] }`. Cases:
  - `current.json` — v1.16.1 shape with one pending, one accepted, one rejected, `excludeCleanupDone: true` → identical, known.
  - `legacy-statuses.json` — `files` entries with `reviewStatus: "accepted"` and `"rejected"` and `claudeContent` → moved to `accepted[]` / `rejected{}` exactly as `migrateSession` does (id = `<capturedAt ?? sessionId>::<path>`, `after = claudeContent ?? originalContent ?? null`), known.
  - `missing-top-level.json` — no `status`, no `accepted` → `status: "active"`, `accepted: []`, known.
  - `unknown-field.json` — top-level `"agent": "x"` and an entry field `"hunks": []` → shape `["agent", "files./ws/proj/a.ts.hunks"]`.
  - `accepted-over-cap.json` — 501 accepted records → oldest dropped to 500 (`MAX_ACCEPTED_RECORDS`).
  - Tests: `has_real_change(None, None) == false`, `(None, Some("x")) == true`, `(Some("a"), Some("a")) == false`, `(Some("a"), None) == true`; `parse_session("")` and `parse_session("{")` return `Err`.
- [ ] **Step 2:** `cargo test -p claudegate-core --test model` → FAIL.
- [ ] **Step 3: Implement** `model.rs`. Parse into `serde_json::Value` first, compute the shape against the known-field lists in Global Constraints, then port `migrateSession` from `src/reviewModel.ts` (v1.16.1) line for line; unknown `reviewStatus` is pending.
- [ ] **Step 4:** tests PASS.
- [ ] **Step 5: Commit** `feat(core): session model, migration and shape check`.

### Task 3: Excludes, roots and worktree discovery

**Files:**
- Create: `core/src/excludes.rs`, `core/src/roots.rs`, `core/src/worktrees.rs`, `fixtures/exclude_cases.json` (copied verbatim from the extension's `hooks/tests/exclude_cases.json` at `v1.16.1`)
- Test: `core/tests/excludes.rs`, `core/tests/worktrees.rs`, unit tests in `roots.rs`

**Interfaces:**
- Produces:
  - `pub fn glob_to_regex(glob: &str) -> Option<Regex>` (port of `globToRegExp`)
  - `pub struct ExcludeMatcher; impl ExcludeMatcher { pub fn new(root: &Path, globs: &[String]) -> Self; pub fn is_excluded(&self, path: &str) -> bool }` — tests the absolute path, the root-relative path and every ancestor dir of the relative path, `\` normalised to `/`.
  - `pub fn read_excludes(home: &Path) -> HashMap<String, Vec<String>>` (missing / invalid → empty)
  - `pub fn read_roots(home: &Path) -> Vec<PathBuf>` (missing / invalid / non-list → empty; non-string items skipped)
  - `pub fn is_worktree_root(dir: &Path) -> bool`; `pub fn nested_worktrees_under(root: &Path) -> Vec<PathBuf>` (sorted, capped at 256)

- [ ] **Step 1: Failing tests.**
  - Every case in `fixtures/exclude_cases.json` yields its `excluded` value.
  - `read_roots` on `["/a", 3, "/b"]` → `[/a, /b]`; on `{}` → `[]`; on missing file → `[]`.
  - Worktrees (temp dir): `root/wt/.git` file containing `gitdir: /x/.git/worktrees/wt` → found; `root/sub/.git` with `gitdir: /x/.git/modules/sub` → not found; `root/node_modules/w/.git` worktree file → not found; a directory symlink `root/loop -> root` → scan terminates and `loop` is not followed; a worktree at depth 7 → not found.
- [ ] **Step 2:** `cargo test -p claudegate-core` → FAIL.
- [ ] **Step 3: Implement.** Port `globToRegExp` exactly (`**` → `.*` consuming a following `/`; `*` → `[^/]*`; `?` → `[^/]`; escape `\^$.|+()[]{}`; anchored). Worktree scan is a breadth-first walk with the skip set and limits from Global Constraints, using `symlink_metadata` so symlinks are never followed; it does not descend into a found worktree.
- [ ] **Step 4:** PASS.
- [ ] **Step 5: Commit** `feat(core): exclude matcher, roots and worktree discovery`.

### Task 4: Project view model

**Files:**
- Create: `core/src/project.rs`, `core/src/textfile.rs`
- Test: `core/tests/project.rs`

**Interfaces:**
- Consumes: Tasks 1–3.
- Produces (all `serde::Serialize`, camelCase, sent to the UI as-is):
  - `pub struct ProjectSummary { root: String, name: String, missing: bool, pending: usize }`
  - `pub struct PendingItem { path: String, rel: String, scope: String, new_file: bool, captured_at: Option<String>, added: usize, removed: usize }`
  - `pub struct RecordItem { id: String, path: String, rel: String, scope: String, decided_at: String, added: usize, removed: usize }`
  - `pub struct ScopeView { root: String, is_worktree: bool, shape_unknown: Vec<String>, error: Option<String> }`
  - `pub struct ProjectView { root: String, missing: bool, scopes: Vec<ScopeView>, pending: Vec<PendingItem>, accepted: Vec<RecordItem>, rejected: Vec<RecordItem> }`
  - `pub fn list_projects(home: &Path) -> Vec<ProjectSummary>`
  - `pub fn load_project(home: &Path, root: &Path) -> ProjectView`
  - `pub fn read_text(path: &Path) -> TextFile` where `pub enum TextFile { Missing, Binary, Text(String) }` (in `textfile.rs`; invalid UTF-8 or a NUL in the first 8 KiB → `Binary`)

- [ ] **Step 1: Failing tests** (temp home + temp workspace):
  - Pending entry whose file on disk equals `originalContent` → not in `pending`; one with a real change → in `pending` with correct `added`/`removed` (line diff via `similar`).
  - Pending entry matching `excludes.json[root]` (matcher anchored at the **primary** root, also for worktree scopes) → hidden.
  - Worktree under root with its own session file → its pending items appear with `scope` = worktree root, and `scopes` has two entries.
  - `pending` sorted by `path` as one list across scopes; `accepted` newest `decided_at` first; `rejected` sorted by `path`.
  - Session file containing `{` → that scope has `error: Some(..)`, other scope still loads.
  - Root that does not exist → `missing: true`, empty lists, no scan.
  - `list_projects`: `name` = last path segment; `pending` equals `load_project(..).pending.len()`.
  - `read_text`: missing → `Missing`; bytes `[0xff, 0xfe, 0x00]` → `Binary`; UTF-8 → `Text`.
- [ ] **Step 2:** FAIL.
- [ ] **Step 3: Implement.** A scope's session missing on disk is not an error (empty). `rel` is the path relative to the primary root, or the absolute path when outside it.
- [ ] **Step 4:** PASS.
- [ ] **Step 5: Commit** `feat(core): project view model`.

### Task 5: Cross-check against the extension

**Files:**
- Create: `xcheck/package.json` (pinned `esbuild`, `tsx`), `xcheck/fetch-extension.sh`, `xcheck/run.ts`, `.github/workflows/ci.yml`

**Interfaces:**
- Consumes: `fixtures/sessions/*.json`, `fixtures/exclude_cases.json`; the extension's `src/reviewModel.ts` (`migrateSession`, `hasRealChange`) and `src/excludeMatcher.ts` (`ExcludeMatcher`) at tag `v1.16.1`.

- [ ] **Step 1:** `fetch-extension.sh` does `git clone --depth 1 --branch v1.16.1 https://github.com/lntvan166/claudegate .xcheck-cache/claudegate` if absent. The tag lives in one variable at the top.
- [ ] **Step 2:** `run.ts` feeds every session fixture's `input` through `migrateSession` and asserts deep-equality with `expected` (ignoring `shape`, which the extension does not have); feeds every exclude case through `new ExcludeMatcher().reload(Object.fromEntries(globs.map(g => [g, true])), root)`; exits non-zero with the case name on any mismatch.
- [ ] **Step 3:** `cd xcheck && npm ci && ./fetch-extension.sh && npx tsx run.ts` → `ok - 5 session cases, N exclude cases`.
- [ ] **Step 4:** Break one fixture's `expected` on purpose → run fails naming it → revert.
- [ ] **Step 5:** `ci.yml`: jobs `core` (`cargo test -p claudegate-core`), `xcheck` (step 3), `ui` (`npm ci && npm run check && npm test` in `ui/`, added in Task 7). Ubuntu only for now.
- [ ] **Step 6: Commit** `test: cross-check fixtures against the extension at v1.16.1`.

### Task 6: Tauri shell, commands and live watcher

**Files:**
- Create: `src-tauri/` (via `npm create tauri-app` template, then trimmed), `src-tauri/src/main.rs`, `src-tauri/src/commands.rs`, `src-tauri/src/watch.rs`
- Test: unit tests in `watch.rs` and `commands.rs`

**Interfaces:**
- Consumes: Task 4.
- Produces (Tauri commands, names exact):
  - `list_projects() -> Vec<ProjectSummary>`
  - `get_project(root: String) -> ProjectView`
  - `get_diff(root: String, scope: String, kind: DiffKind, key: String) -> DiffPayload` where `DiffKind = "pending" | "accepted" | "rejected"`, `key` = path (pending, rejected) or record id (accepted), and `DiffPayload { path, before: TextSide, after: TextSide }`, `TextSide = { kind: "missing" | "binary" | "text", text?: string }`. Pending: before = `originalContent`, after = disk. Records: `before`/`after` from the record.
  - Event `claudegate://changed` with payload `{ roots: string[] }` (roots whose view may have changed; empty = re-list projects).
- [ ] **Step 1: Failing test** for `pub fn affected_roots(changed: &[PathBuf], known: &[(String /*root*/, PathBuf /*session file*/)]) -> Vec<String>`: a change to scope B's session file → `[B's project root]`; a change to `workspace-roots.json` or `excludes.json` → `[]` plus a `relist` flag; a `.lock` / `.tmp` file → ignored.
- [ ] **Step 2:** FAIL. **Step 3:** implement `affected_roots`; watcher uses `notify` (non-recursive on `<home>/sessions` and `<home>`), coalesces events for 150 ms, then emits one `claudegate://changed`. Commands are thin wrappers over `core` using `claudegate_home()`. **Step 4:** PASS.
- [ ] **Step 5:** Sandbox run: `HOME=$(mktemp -d) python3 .xcheck-cache/claudegate/manual-test-seed.py /tmp/cg-ws` then `CLAUDEGATE_HOME=$HOME/.claudegate npm run tauri dev` with the default template page calling `list_projects` → the seeded project and its pending count print in the devtools console.
- [ ] **Step 6: Commit** `feat(app): tauri commands and coalesced session watcher`.

### Task 7: UI shell — projects, tabs, file tree with icons

**Files:**
- Create: `ui/` (Vite + Svelte 5 + TS), `ui/src/lib/api.ts`, `ui/src/lib/tree.ts`, `ui/src/lib/icons.ts`, `ui/src/components/{ProjectList,ReviewTabs,FileTree}.svelte`, `ui/src/App.svelte`, `ui/src/theme.css`
- Test: `ui/src/lib/tree.test.ts`, `ui/src/lib/icons.test.ts`

**Interfaces:**
- Consumes: Task 6 commands and event (typed wrappers in `api.ts`).
- Produces: `buildTree(items: { rel: string }[]) -> TreeNode[]` with `TreeNode = { name: string; rel: string; kind: "folder" | "file"; children?: TreeNode[]; item?: T }` — folders first, then files, each alphabetical (case-insensitive); single-child folder chains compacted (`src/api`). `iconFor(name: string, kind: "file" | "folder", open: boolean) -> string` (asset URL).

- [ ] **Step 1: Failing tests.** `buildTree` on `src/a.ts, src/api/x.ts, README.md` → `[src{api{x.ts}, a.ts}, README.md]`; on `a/b/c/d.ts` → one folder `a/b/c`. `iconFor("package.json","file",false)` → URL ending `nodejs.svg`; `("utils.ts","file")` → `typescript.svg`; `("src","folder",true)` → `folder-src-open.svg`; `("weird.zzz","file")` → `file.svg`.
- [ ] **Step 2:** `npm test` → FAIL. **Step 3:** implement. `icons.ts` reads `material-icon-theme`'s generated manifest (`fileNames`, `fileExtensions` with longest-suffix match, `folderNames`, `folderNamesExpanded`, defaults `file` / `folder`); copy only the referenced SVGs into `ui/public/icons/` with a build script. **Step 4:** PASS.
- [ ] **Step 5:** Build the layout from the spec's UI sketch: project list (name, pending badge, "Folder missing" state, missing projects sorted last), Pending / Accepted / Rejected tabs with counts, file tree with icons and `+N −M`, worktree scopes as top-level groups when there is more than one scope. Light and dark from `prefers-color-scheme`, colours as CSS custom properties in `theme.css`. On `claudegate://changed` re-fetch the affected project (and the list when asked).
- [ ] **Step 6: Mockup checkpoint.** Screenshot both themes in the sandbox and show the maintainer before Task 8 polishes anything. Record their feedback in the task report.
- [ ] **Step 7: Commit** `feat(ui): project list, review tabs and icon tree`.

### Task 8: Monaco diff, keyboard, open in editor, tray count

**Files:**
- Create: `ui/src/components/DiffView.svelte`, `ui/src/lib/language.ts`, `ui/src/lib/keys.ts`, `src-tauri/src/editor.rs`, `src-tauri/src/tray.rs`, `src-tauri/src/config.rs`
- Test: `ui/src/lib/language.test.ts`, `ui/src/lib/keys.test.ts`, unit tests in `editor.rs`, `config.rs`

**Interfaces:**
- Consumes: `get_diff` (Task 6), tree/selection (Task 7).
- Produces: command `open_in_editor(path: String, line: u32)`; `pub fn render_editor_command(template: &str, path: &str, line: u32) -> Vec<String>`; `pub struct Config { editor: String }` read from `~/.claudegate-desktop/config.json`, default `"zed {path}:{line}"`, created with defaults if missing (this is the app's own directory, allowed). `languageFor(path: string) -> string` (Monaco language id, `plaintext` fallback). `nextIndex(current: number, len: number, dir: 1 | -1) -> number` (clamped, no wrap).

- [ ] **Step 1: Failing tests.** `render_editor_command("zed {path}:{line}", "/a b/c.ts", 12)` → `["zed", "/a b/c.ts:12"]` (split the template on whitespace **before** substituting, so spaces in paths survive; no shell). `render_editor_command("code -g {path}:{line}", …)` → `["code","-g","/a b/c.ts:12"]`. Missing config → default editor. `languageFor("x.go")` → `go`, `("Dockerfile")` → `dockerfile`, `("x.unknown")` → `plaintext`. `nextIndex(0,3,-1)` → `0`, `nextIndex(2,3,1)` → `2`.
- [ ] **Step 2:** FAIL. **Step 3:** implement.
  - `DiffView`: one Monaco `DiffEditor` instance reused across selections (dispose models on switch), `readOnly: true`, `renderSideBySide` toggled by a button and remembered in `localStorage`, `vs` / `vs-dark` following the OS. `missing` side → empty model; `binary` → no editor, a centred "Binary file, not shown". Monaco workers via Vite `?worker` imports, bundled.
  - Keys in `keys.ts`: `j`/`↓` next row, `k`/`↑` previous row, `n`/`p` next/previous **pending** file across scopes, `o` open in editor at the first changed line (from Monaco's `getLineChanges()`, else 1), `1`/`2`/`3` switch tabs. Ignore keys while an input has focus.
  - Tray: icon with tooltip `ClaudeGate — N pending` (sum of `list_projects` pending), updated on every `claudegate://changed`; menu items "Show" and "Quit".
- [ ] **Step 4:** PASS.
- [ ] **Step 5:** Sandbox run: select each seeded file → diff shows; a new file shows an empty left side; `o` opens Zed at the right line; editing a seeded file in another editor updates the counts within a second.
- [ ] **Step 6: Commit** `feat(ui): monaco diff, keyboard navigation, open in editor, tray count`.

### Task 9: Read-only proof against real data

**Files:**
- Create: `scripts/readonly-proof.sh`, `core/tests/no_writes.rs`

- [ ] **Step 1: Failing test** `no_writes.rs`: scan every `.rs` file under `core/src` and `src-tauri/src` for `fs::write`, `File::create`, `OpenOptions`, `rename(`, `remove_file`, `remove_dir`, `create_dir`; allow them only in `src-tauri/src/config.rs` (the app's own dir). Seed a violation → test fails → remove it.
- [ ] **Step 2:** `readonly-proof.sh`: record `find ~/.claudegate -type f -printf '%p %s %T@\n' | sort | sha256sum`, run the release build against the real home for 60 s while the maintainer clicks through projects, record again, and print `UNCHANGED` or the diff. Files the hook or the extension changed in that window are expected; the script lists changed files and their last writer from `hook.log` so the maintainer can tell.
- [ ] **Step 3:** `cargo test --workspace` and `npm test` PASS; run the proof with the maintainer.
- [ ] **Step 4: Commit** `test: prove the viewer never writes to the ClaudeGate home`; tag `v0.1.0` (no release upload; packaging is milestone 5).
