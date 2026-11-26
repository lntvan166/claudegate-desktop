# Git Log View (phase 1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a read-only "Git log" view to ClaudeGate Desktop: Polylog's merged, filterable log across every repository in the workspace, with a Changes tree and the Monaco diff.

**Architecture:**
- **Pure logic in `core/src/gitlog/`:** parsing, filter→argv, merge with a horizon, discovery, remote URLs. Ported from Polylog's pure modules and tests.
- **`src-tauri/src/gitlog.rs`:** spawns `git` through a pool of 16 with per-query cancellation, and exposes `gitlog_*` commands.
- **`ui/src/gitlog/`:** the view, behind a header switch `[ Review │ Git log ]`, reusing the app's diff, icons, tree, splitter, context menu and theme.

**Tech Stack:** Rust (Tauri 2, `claudegate-core`), Svelte 5, vitest, Monaco (existing).

**Spec:** `docs/superpowers/specs/2026-11-26-git-log-view-design.md`. Its "Decisions after the builder check" section overrides the earlier text.

**Reference code (read-only):** `~/Documents/src/personal/polylog/src`. Each task names the Polylog modules it ports from.

## Global Constraints

- **Read-only.** Nothing in this plan writes to a repository or to `~/.claudegate*`. The only write is the app's own `config.json`, under one `gitLog` key, through the existing `update_config` and `CONFIG_LOCK`. Keep `core/tests/no_writes.rs` green.
- **No index, cache or database.** Every filter is git argv. Rust and JS only merge streams that are already filtered.
- **Git invocation:**
  - `git --no-pager -c color.ui=never -c core.quotepath=off …` with env `GIT_TERMINAL_PROMPT=0` and `GIT_OPTIONAL_LOCKS=0`.
  - Resolve the git binary like the app resolves python3: PATH, then `/usr/local/bin`, `/usr/bin`, `/opt/homebrew/bin`.
- **Formats:**
  - Log: `--format=%H%x00%ct%x00%an%x00%ae%x00%s%x00%P%x1e`
  - Changes: `show --numstat --name-status -M --diff-merges=first-parent --format= <sha>`. Combine numstat and name-status in one or two calls; the builder decides.
- **Paging:**
  - `pageSize` 200, concurrency 16.
  - Each repo keeps a `fetched` count, and the next page passes `--skip=<fetched>`.
  - Rows are shown down to the horizon (Polylog's `takeReady`).
  - "Now" is fixed at the first page.
- **Responsiveness:**
  - Filter input is debounced by 250 ms.
  - A new query kills the previous query's children.
  - A result from a stale query id is never rendered.
  - The list is virtualised.
- **Performance targets** (synthetic fixture: 70 repos × 300 commits, warm cache):
  - First page in under 300 ms.
  - Changes for one commit in under 100 ms.
- **Identity:**
  - A repo is its git common dir, resolved.
  - Its log covers the HEADs of the main checkout and every worktree in one `git log`.
- **Defaults:** `gitLog.scanDepth` default 2. Discovery never enters `.git`, `node_modules`, `vendor` or `target`.
- **Diff:** the before/after content cap is 2 MB. Above it, show "Too large to show", with Open in editor offered. Binary files show "Binary file".
- **Neutral names** in code, tests, fixtures and docs (`acme-api`, `acme-web`, `svc-lib`). Before every commit, run the controller's internal-name scrub grep. Never write the scrub list into the repo.
- **Commits:** no version bump, push or release. End commit messages with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- **Tests:**
  - Rust: `cargo test --workspace` with `CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4`, using fixture repos created in temp dirs by a test helper (`git init`, commits with fixed dates via `GIT_COMMITTER_DATE`).
  - UI: `npm --prefix ui test` and `npm --prefix ui run check`.

## Review Focus

1. **A broken repo among good ones** (no commits, a shallow clone, a deleted folder, an interrupted rebase): only that repo shows a failure row; the others render. Pinned in Task 4 (`failing_repo_is_contained`).
2. **Subjects and authors with unicode, emoji, quotes or tabs:** they parse intact. Pinned in Task 1 (`parses_tricky_fields`).
3. **Typing fast in search:** no stale page appears after a newer one, and no orphan git processes remain. Pinned in Task 4 (`stale_query_never_returned`, `cancel_kills_children`).
4. **Commits sharing the same second across repos, and Load more:** no row repeats and none is lost. Pinned in Task 2 (`horizon_no_dup_no_gap`).
5. **A path pattern with spaces or glob characters, and a branch missing in some repos:** the argv is correct, and the footer counts are right. Pinned in Task 2 (`path_glob_argv`, `branch_fallback_counts`).

---

### Task 1: Parsing and remote URLs (core, pure)

**Ports:** Polylog `gitLog.ts` (parseLog), `commitDetail.ts` (numstat and name-status parsing), remote URL building (`remoteUrl` or equivalent in polylog/src).

**Files:**
- Create `core/src/gitlog/mod.rs`, `core/src/gitlog/parse.rs` and `core/src/gitlog/remote.rs`.
- Register the `gitlog` module in `core/src/lib.rs`.
- Test: `core/tests/gitlog_parse.rs`.

**Interfaces (produces):**
- `pub struct Commit { pub sha: String, pub time: i64, pub author: String, pub email: String, pub subject: String, pub parents: Vec<String> }`, plus `repo: String` (the repo id), set by the caller.
- `pub fn parse_log(out: &str) -> Vec<Commit>`.
- `pub enum FileStatus { Added, Modified, Deleted, Renamed { from: String }, Copied { from: String }, TypeChanged }`.
- `pub struct ChangedFile { pub path: String, pub status: FileStatus, pub added: Option<u32>, pub removed: Option<u32> }`. `None` means binary.
- `pub fn parse_changes(name_status: &str, numstat: &str) -> Vec<ChangedFile>`.
- `pub fn web_url(remote: &str, sha: &str) -> Option<String>`. It handles GitHub, GitLab, Bitbucket and Azure DevOps in both ssh and https forms, and strips any `user:pass@`.

- [ ] **Step 1: Write failing tests.**
  - `parses_tricky_fields`: subjects with emoji, quotes, tabs, `%` and a trailing space, and an author with unicode.
  - `parses_root_and_merge_parents`: zero and two parents.
  - `parses_rename_and_binary`: `R100 a b`, and numstat `-\t-\t`.
  - `web_url_providers`: each provider, ssh and https, and one with credentials, which must come out stripped.
  - `web_url_unknown_host_none`.
- [ ] **Step 2:** FAIL. **Step 3:** Implement. **Step 4:** PASS.
- [ ] **Step 5: Commit** `feat(gitlog): parse log records, changed files and remote URLs`.

### Task 2: Filter to argv, and merge with a horizon (core, pure)

**Ports:** Polylog `filterModel.ts` (`logArgs`, `selectRepos`), `mergeStream.ts` (`takeReady`, `RepoProgress`), `logQuery.ts` (page assembly, branch use). Port their tests.

**Files:**
- Create `core/src/gitlog/filter.rs` and `core/src/gitlog/merge.rs`.
- Test: `core/tests/gitlog_filter.rs` and `core/tests/gitlog_merge.rs`.

**Interfaces (produces):**
- `pub struct Filter { pub search: Option<String>, pub authors: Vec<String>, pub mine: bool, pub path: Option<String>, pub branch: Option<String>, pub since: Option<i64>, pub until: Option<i64> }`.
- `pub fn log_args(f: &Filter, heads: &[String], me: Option<&str>, page: usize, skip: usize) -> Vec<String>`.
  - `heads` is the refs to log. Without a branch filter that is the HEADs of the main checkout and its worktrees. With one, it is `[branch]` when the repo has the branch, else the current HEADs.
  - search → `--grep=<t> -i -F`
  - authors → one `--author=` per chip, with `mine` adding the `me` email
  - path → `--` then `:(glob)<p>`
  - since/until as epoch seconds
- `pub struct RepoProgress { pub fetched: usize, pub pending: Vec<Commit>, pub exhausted: bool }`.
- `pub fn take_ready(progress: &mut BTreeMap<String, RepoProgress>) -> Vec<Commit>`. Sorted by time descending, ties broken by sha ascending. It takes only rows at or above the horizon.
- `pub struct BranchUse { pub branch: String, pub found: usize, pub fallback: usize }`.

- [ ] **Step 1: Write failing tests.** Port Polylog's `filterModel` and `mergeStream` cases. Add:
  - `path_glob_argv`: the paths `docs/my notes/*.md` and `*.sql`.
  - `branch_fallback_counts`.
  - `horizon_no_dup_no_gap`: three repos, a busy one and two quiet ones, with commits sharing seconds. Paging three times yields every commit exactly once, in order.
  - `mine_without_email_skips_repo`.
- [ ] **Step 2:** FAIL. **Step 3:** Implement. **Step 4:** PASS.
- [ ] **Step 5: Commit** `feat(gitlog): filters as git argv; merge pages at the horizon`.

### Task 3: Discovery from the workspace (core)

**Ports:** Polylog `discoverWalk.ts` and `repos.ts` (labels, colors). Worktree grouping is new.

**Files:**
- Create `core/src/gitlog/discover.rs`.
- Test: `core/tests/gitlog_discover.rs`.

**Interfaces:**
- **Consumes:** the workspace roots (the resolved roots the rail lists), `scan_depth: usize` and `exclude: &[String]`.
- **Produces** `pub struct Repo { pub id: String, pub name: String, pub color: u8, pub checkouts: Vec<Checkout> }`, where `id` is the common dir and `color` is an index into the UI palette.
- **Produces** `pub struct Checkout { pub path: String, pub head: String, pub branch: Option<String>, pub is_main: bool }`.
- **Produces** `pub fn discover(roots: &[PathBuf], scan_depth: usize, exclude: &[String], git: &dyn GitRun) -> Vec<Repo>`.
  - `GitRun` is a trait so tests can use real fixture repos.
  - Group by `git rev-parse --git-common-dir`. Checkouts come from `git worktree list --porcelain`.
  - Name a repo by the main checkout's folder name. Repos with the same folder name get a parent-folder suffix.
  - The color is stable: a hash of the id.
- **Produces** `pub fn ahead_behind(repo: &Repo, git: &dyn GitRun) -> Option<(u32, u32)>`, from `git rev-list --left-right --count @{u}...HEAD` on the main checkout. It returns `None` when there is no upstream.

- [ ] **Step 1: Write failing tests.**
  - `finds_root_and_nested_repos_depth2`: an umbrella root plus 3 nested repos, one of them at depth 3, which is not found.
  - `skips_node_modules_and_target`.
  - `worktrees_group_into_one_repo`: a main checkout plus 2 worktrees give one `Repo` with 3 checkouts.
  - `same_name_disambiguated`.
  - `excluded_repo_removed`.
  - `ahead_behind_none_without_upstream`.
- [ ] **Step 2:** FAIL. **Step 3:** Implement. **Step 4:** PASS.
- [ ] **Step 5: Commit** `feat(gitlog): discover repositories from the workspace`.

### Task 4: Git runner, pool, cancellation and commands (src-tauri)

**Files:**
- Create `src-tauri/src/gitlog.rs` and register its commands in `lib.rs`.
- Test: `src-tauri/tests/gitlog_cmd.rs`, plus `src-tauri/tests/gitlog_perf.rs`, which is `#[ignore]` by default and run explicitly.

**Interfaces:**
- **Consumes** Tasks 1–3.
- **Produces** these Tauri commands:
  - `gitlog_repos() -> Vec<RepoView>`: discovery over the workspace, with ahead/behind.
  - `gitlog_page(query_id: u64, repos: Vec<String>, filter: Filter, more: bool) -> PageView`.
    - `PageView { query_id, rows, failures: Vec<{repo, reason}>, branch_use, done }`.
    - Paging state is held in Rust per window, keyed by the latest `query_id`. A new `query_id` drops the old state and kills its children.
  - `gitlog_changes(repo, sha) -> Vec<ChangedFile>`.
  - `gitlog_file(repo, sha, path, side: "before"|"after") -> FileView`. `FileView` is `Text(String) | Binary | TooLarge(u64) | Missing`. Before = `parents[0]:<from or path>`; a root commit or an added file gives `Missing`.
  - `gitlog_web_url(repo, sha) -> Option<String>`.
  - `gitlog_message(repo, sha) -> String`, the full message.
- **Produces** the pool: at most 16 concurrent children. Each child is tracked by `query_id` and killed on supersede.

- [ ] **Step 1: Write failing tests.**
  - `stale_query_never_returned`: start query 1 on a slow repo (simulated with a large fixture or a `GIT_TRACE`-free sleep wrapper), then query 2. Query 1's result is discarded and query 2 returns.
  - `cancel_kills_children`: after the supersede, no `git` child of query 1 is alive.
  - `failing_repo_is_contained`: an empty repo, a deleted path and a corrupt HEAD alongside 2 good repos give 2 good row streams and 3 failures.
  - `file_before_after`: added, deleted, renamed and binary files, and a file over 2 MB.
  - `web_url_from_origin`.
- [ ] **Step 2:** FAIL. **Step 3:** Implement. **Step 4:** PASS.
- [ ] **Step 5: Measure.** Run `cargo test -p claudegate-desktop --test gitlog_perf -- --ignored` with 70 repos × 300 commits. Assert the first page is under 300 ms and changes are under 100 ms, and report the numbers.
- [ ] **Step 6: Commit** `feat(gitlog): commands, pooled git runner and cancellation`.

### Task 5: View switch, layout and the repositories pane (UI)

**Files:**
- Create `ui/src/gitlog/GitLogView.svelte`, `ui/src/gitlog/RepoPane.svelte` and `ui/src/gitlog/repos.ts`.
- Modify `ui/src/App.svelte` for the header switch, the shortcut and lazy mount.
- Modify `ui/src/lib/api.ts` and `ui/src/lib/mock.ts`.
- Modify `src-tauri/src/config.rs` for the `gitLog` key: `ticked: string[] | null`, `profiles: {name: string[]}`, `lastFilter`, `scanDepth`, `excludeRepos`, `panes`.
- Test: `ui/src/gitlog/repos.test.ts`.

**Interfaces:**
- **Consumes** `gitlog_repos`.
- **Produces:**
  - The header segmented control `[ Review │ Git log ]`. Its shortcut must not clash with the review's existing keys; check the keys module.
  - The view mounts on first open only.
  - The Repos pane:
    - tick and untick, fuzzy search, multi-select;
    - profiles: save, apply, rename, delete;
    - right-click: show only this, hide, show all, copy path;
    - ↓/↑ counts.
  - Ticks and profiles persist through add-only and remove-only config commands, never a whole-list overwrite from UI state.

- [ ] **Step 1: Write failing vitest tests:**
  - fuzzy matching;
  - applying a profile ticks exactly its repos and leaves the filters alone;
  - a profile keyed by an id that is now missing is skipped silently and counted;
  - `ticked: null` means all repos.
- [ ] **Step 2:** FAIL. **Step 3:** Implement. **Step 4:** PASS, then run `npm --prefix ui run check`.
- [ ] **Step 5: Commit** `feat(ui): Git log view shell and repositories pane`.

### Task 6: Log list, filters, keyboard and context menu (UI)

**Ports:** Polylog `webview/list.ts` and `webview/filters.ts` behaviour.

**Files:**
- Create `ui/src/gitlog/LogList.svelte`, `ui/src/gitlog/Filters.svelte` and `ui/src/gitlog/log.ts`.
- Test: `ui/src/gitlog/log.test.ts`.

**Interfaces:**
- **Consumes** `gitlog_page`, `gitlog_message` and `gitlog_web_url`.
- **Produces:**
  - A virtualised list of rows: repo chip (color index → theme palette), short SHA, subject, relative date, author. A badge shows the worktree or branch when it differs from the main checkout.
  - Load more.
  - Failure rows, which can be dismissed.
  - Filters: search, author chips with Me, path, branch, and date range (24h default / 7d / 30d / all / custom), with the "found in N · current branch in M" footer.
  - Keys: ↑/↓, Home/End, PageUp/PageDown, Enter (open the first file), `/`, Esc.
  - Right-click a commit: Copy Commit ID, Copy Message, Open on Remote (opened with the app's existing opener).

- [ ] **Step 1: Write failing tests:**
  - query-id gating: a response with a stale id is ignored;
  - the debounce is 250 ms;
  - relative-date text;
  - the footer text;
  - the keyboard selection model, including Load more appending without losing the selection.
- [ ] **Step 2:** FAIL. **Step 3:** Implement. **Step 4:** PASS, then check.
- [ ] **Step 5: Commit** `feat(ui): merged log list, filters and keyboard`.

### Task 7: Changes tree and diff (UI)

**Files:**
- Create `ui/src/gitlog/Changes.svelte` and `ui/src/gitlog/changes.ts`.
- Modify `DiffView.svelte` only if it needs a source-agnostic entry. Prefer passing it before and after text, plus a title.
- Test: `ui/src/gitlog/changes.test.ts`.

**Interfaces:**
- **Consumes** `gitlog_changes` and `gitlog_file`.
- **Produces:**
  - A Changes tree with A/M/D/R badges, +/− counts, Material icons and folders. Its title reads `acme-api · 3f9a2c1 · 2 files`.
  - Clicking a file opens the Monaco diff, read-only, with the existing Dracula/Alucard theme. Binary and too-large files show their messages.
  - The diff follows the selection.
  - Open in editor (the existing editor template) opens the file at that line in the working tree.

- [ ] **Step 1: Write failing tests:**
  - building the tree from `ChangedFile[]`, including renames;
  - the title text;
  - the binary, too-large and missing states mapped to the pane state.
- [ ] **Step 2:** FAIL. **Step 3:** Implement. **Step 4:** PASS, then check.
- [ ] **Step 5: Browser preview.** Load the mock with the Git log view, then screenshot it narrow and wide, in dark and light. Stop the dev server by PID, never with `pkill -f`. Move the Playwright screenshots out of the extension repo.
- [ ] **Step 6: Commit** `feat(ui): commit changes tree and diff`.

### Task 8: `claudegate log [PATH]`

**Files:**
- Modify `src-tauri/claudegate`: add the `log` subcommand. `claudegate log` with no path uses `$PWD`, and `claudegate ./log` still opens a folder named log.
- Modify `src-tauri/src/launch.rs` for the target kind (review or log).
- Modify `ui/src/lib/launch.ts` and `App.svelte`.
- Modify `README.md`.
- Test: `src-tauri/tests/launch*.rs` and `ui/src/lib/launch.test.ts`.

**Interfaces:**
- **Produces:**
  - The launch target `{ kind: "review" | "log", path, isDir, exists, canonical }`.
  - A `log` launch opens the Git log view with the repos under the path ticked for this launch only, with nothing written to config.
  - A path outside the workspace shows the existing "isn't in your workspace" notice with [Add to workspace].

- [ ] **Step 1: Write failing tests:**
  - argv parsing: `log`, `log PATH`, `./log` and `--`;
  - the launch pull and the second-instance hand-off carry the kind;
  - ticks from a launch are not persisted.
- [ ] **Step 2:** FAIL. **Step 3:** Implement. **Step 4:** Run everything: cargo, vitest, check, `scripts/decide-e2e.sh`.
- [ ] **Step 5: Commit** `feat: claudegate log opens the Git log view`.
