# Git Log Phase 2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port Polylog's remaining read-only tools into the Git log view: Combined Changes (several commits, each file once), File History, Blame in diffs, and Compare Branches.

**Architecture:**
- **Pure logic in `core/src/gitlog/`**, ported from Polylog with its tests: `line_merge.rs` (diff3), `combined.rs` (range log, net status, the "selected changes taken out" left side, behind a `GitRead` trait so it is tested with a fake git), `history.rs`, `blame.rs`, `compare.rs`.
- **`src-tauri/`** runs git through the existing `Runner` (pool of 16, `RESERVED` slots, kill on supersede). Each feature gets its own cancellation **channel** (`<window>#combined`, `#blame`, `#compare`), so a new combined selection never kills the log's paging and vice versa. Combined and Compare live in `src-tauri/src/gitlog_combined.rs` and `src-tauri/src/gitlog_compare.rs`; History and Blame are small and stay in `gitlog.rs`.
- **`ui/src/gitlog/`**: pure helpers with vitest tests (`selection.ts`, `combined.ts`, `blame.ts`, `compare.ts`), and Svelte components that draw them. The diff stays the shared `DiffView.svelte` through its `external` prop, which gains a `banner` and a `blame` hook.

**Tech Stack:** Rust (Tauri 2, `claudegate-core`, `claudegate-desktop`), Svelte 5, vitest, Monaco (existing). No new dependencies.

**Spec:** `docs/superpowers/specs/2026-11-27-git-log-phase2-design.md`. Phase 1's spec (`2026-11-26-git-log-view-design.md`) still binds. The behaviour reference, rule for rule, is Polylog (`~/Documents/src/personal/polylog`): `docs/superpowers/specs/2026-10-20-combined-changes-design.md` (cited below as **CC §n**), `docs/superpowers/specs/2026-10-01-compare-branches-design.md` (**CB §n**), and the source files each task names. Executors read the Polylog file a task ports before writing it.

## Global Constraints

### From the phase 1 spec, verbatim

**Anti-goals (would be a failure even if it works)**

- **An index, cache or database of commits.** Polylog measured 25 ms for `git log -n 50` across 68 repos (16-way parallel). Spawn on demand and merge in memory, as Polylog does.
- **Filtering in Rust or JS what `git log` can filter.** Every filter goes into git's argv, and the app only merges already-filtered streams.
- **Writing to a repository, or to ClaudeGate's shared files.** Phase 1 writes only the app's own `config.json`: profiles, ticked repos, last filters.
- **Slowing the review view.** Git work runs off the UI thread, and the log never loads until the Git log view is first opened.
- **One broken repo breaking the view.** Failures stay with their repo: a dismissible row names the repo and the reason. Typical causes are a shallow clone, permissions, an interrupted rebase, a path that is no longer a repo, or a repo with no commits.

**Constraints**

- **Stack:** Tauri 2 + Rust (`claudegate-core` for the pure logic, `src-tauri` for commands and process spawning) and Svelte 5. Reuse the app's diff, icons, file tree, theme tokens (Dracula/Alucard), settings screen and context menu.
- **Code layout, so phases 2 and 3 or a later split stay cheap:**
  - `core/src/gitlog/`: pure parsing, merge, filter→argv, discovery, remote URLs, with unit tests.
  - `src-tauri/src/gitlog.rs`: commands, the spawn pool, cancellation.
  - `ui/src/gitlog/`: view components and their pure helpers.
  - Shared UI pieces are imported, not copied.
- **Git invocation.**
  - Run `git` as a child process with `--no-pager -c color.ui=never -c core.quotepath=off`.
  - Never prompt for credentials: `GIT_TERMINAL_PROMPT=0`.
  - The git binary comes from PATH, with the same GUI-PATH fallback directories the app already uses for python3 and editors.
- **Responsiveness.**
  - Filter input is debounced 250 ms.
  - A new query kills the previous query's child processes.
  - The list is virtualised: thousands of rows stay smooth.
- **Neutral names** in all code, tests, fixtures and docs (`acme-api`, `acme-web`, `svc-lib`): the repo is public. Polylog's §7 "neutral fixtures" rule applies.
- No version bump, release or push without the maintainer saying so.

### From the phase 2 spec, verbatim

- **Merge in Rust.** The line merge (diff3) is a pure Rust module in core. It never uses `git merge-file` or `merge-tree`, because those write objects into `.git`. Inputs are read with `git show <sha>:<path>`. Nothing is cached. The round-trip guard and the 1 MB / 20,000-line / 2,000-edited-line limits all carry over.
- **Storage.** Favourites and recent pairs are kept in the app's own `config.json` under `gitLog`, with set-one-key and add-only writers, as in phase 1.
- **Anti-goals (on top of phase 1's)**
  - Writing anything into a repository, including git objects. This is why the merge is our own code and not `git merge-file`.
  - A diff that passes off an unselected commit's change as part of the selection without saying so.
  - Changing how a single click and a single commit behave.
- Commands use the prefix `gitlog_`.

### Facts of the current code (`main` at 2def27a); they supersede the quoted text where it differs

- Log records are **six NUL-terminated fields, no RS** (`LOG_FORMAT` in `core/src/gitlog/parse.rs`). New formats that carry free text (subjects, paths) are parsed **positionally** on NUL tokens, never by splitting on RS or US: a subject or path may contain `\x1e` or `\x1f`.
- `Runner::run` already adds `-c i18n.logOutputEncoding=UTF-8`, strips every `GIT_*` variable (`sanitize_git_env`) and sets `GIT_TERMINAL_PROMPT=0` and `GIT_OPTIONAL_LOCKS=0`. Every new git call goes through `Runner` (src-tauri) or the `GitRead` trait (core); none spawns git itself, except the test-only `git merge-file` oracle in a temp dir.

### Plan-wide rules

- **No repo writes, including no git objects.** Allowed git subcommands are reads only: `log`, `show`, `cat-file`, `rev-parse`, `rev-list`, `merge-base`, `diff` (between commits only), `blame`, `for-each-ref`, `config --get`, `remote get-url`, `worktree list`. `core/tests/no_writes.rs` stays green. The Combined and Compare integration tests assert `.git/objects` and `git status --porcelain` are unchanged.
- **No commit cache.** Combined reads the range again on every selection, and the left side is recomputed each time a file is opened. The UI's existing `ChangesCache` (24 commits' file lists) is phase 1's and stays as is; nothing new is cached.
- **Filters go into git argv.** File History hands git its paths; Compare hands git the pair. Rust and TS only merge, parse and group.
- **Config writes are set-one-key or add-only under `CONFIG_LOCK`**, through `update_gitlog` in `src-tauri/src/config.rs`. No writer takes a whole list from the UI.
- **Nothing runs at app start.** Every new command is called only from the Git log view after it is opened, and Compare reads only while Compare mode is shown.
- **Neutral names:** repos `acme-web`, `acme-api`, `svc-lib` (plus `acme-old` where a fourth is needed), people `dana` and `rin`, branches `main`, `release-1.4`, `origin/main`, `origin/release-1.4`. Before each commit, run the controller's internal-name scrub over the staged diff; never write the scrub list into the repo.
- **Commits:** on the current branch only, `git add <files>` then `git commit`; never checkout, reset, stash, rebase or push. End each message with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- **Tests:** Rust with `CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4 cargo test --workspace` (single files as each task says); UI with `npm --prefix ui test` and `npm --prefix ui run check`. Every new API method is added to `ui/src/lib/api.ts` (the interface, the Tauri implementation and the exported wrapper) and to `ui/src/lib/mock.ts` in the same task.
- **Ids:** every query id comes from the existing `queryGate` (`ui/src/gitlog/log.ts`), which only rises.

## Review Focus

1. **Merges and branches in the selection** (a selected merge commit, an unselected merge in range, selected commits on two branches): a selected merge diffs against its first parent; an unselected merge sends the file to the slow path; two branches take the slow path and fall back with ⚠ when the revert does not merge; nothing panics. Pinned in Task 3 (`unselected_merge_in_range_is_slow`, `two_branches_take_slow_path`) and Task 4 (`combined_merges_and_branches`).
2. **Renames** (renamed inside the selection, renamed by an unselected commit, renamed in File History, renamed on one Compare side): one file, shown at its newest path, read at the old path where it lived. Pinned in Task 3 (`renames_followed_by_path`, `left_side_follows_unselected_rename`), Task 7 (`history_follows_rename`) and Task 10 (`tip_paths_follow_a_rename`).
3. **Binary and huge files** in a combined selection: no merge is attempted; the file falls back with "binary" or "too large", never an error and never a hang. Pinned in Task 2 (`huge_rewrite_is_too_large`), Task 3 (`left_side_binary_and_too_large`) and Task 4 (`combined_binary_and_huge`).
4. **Cancellation on selection change**: a new selection (or pair) kills the previous read's git children, its answer is never drawn, and the log's own paging is left alone. Pinned in Task 4 (`combined_superseded_is_stale_and_killed`, `combined_channel_leaves_the_log_alone`), Task 5 (`a superseded combined answer is ignored`) and Task 10 (`compare_superseded_is_stale`).
5. **Hostile file names** (newline, tab, `\x1e`, `\x1f`, a leading `-`, `:`, ` => `, a name that looks like a raw record): parsed intact and passed only after `--` or inside `<rev>:<path>`. Pinned in Task 3 (`parse_range_log_hostile_names`), Task 4 (`combined_hostile_names`), Task 7 (`parse_history_hostile_names`) and Task 10 (`parse_diff_z_hostile_names`).

---

## Combined Changes

### Task 1: Multi-select in the commit list (UI)

**Ports:** Polylog `src/webview/selectionModel.ts` and its tests; CC §2.

**Files:**
- Create `ui/src/gitlog/selection.ts`, test `ui/src/gitlog/selection.test.ts`.
- Modify `ui/src/gitlog/LogList.svelte` (clicks, keys, rendering, ARIA, reload) and `ui/src/gitlog/GitLogView.svelte` (receives the picked rows).

**Interfaces:**
- **Produces** in `selection.ts`:
  - `export const SELECTION_LIMIT = 100;`
  - `export const LIMIT_NOTICE = "Combined Changes shows up to 100 commits";`
  - `export type Selection = { keys: string[]; focus: string | null; anchor: string | null };` Keys are `rowKey(row)`; `keys` is in list order.
  - `export const EMPTY_SELECTION: Selection`.
  - `export type Gesture = { kind: "click"; key: string; ctrl: boolean; shift: boolean } | { kind: "arrow"; key: string; shift: boolean } | { kind: "context"; key: string };`
  - `export function applyGesture(s: Selection, g: Gesture, order: readonly string[]): { next: Selection; overLimit: boolean }`
  - `export function keepListed(s: Selection, order: readonly string[]): Selection`
  - `export function clickGesture(e: { ctrlKey: boolean; metaKey: boolean; shiftKey: boolean }, key: string, mac: boolean, multi: boolean): Gesture`. "ctrl" is `metaKey` on macOS and `ctrlKey` elsewhere; with `multi: false` (File History, Task 8) every click is a plain click.
  - `export function pickedRows(rows: RowView[], s: Selection): RowView[]`, in list order.
  - `export function single(key: string): Selection`, giving `{ keys: [key], focus: key, anchor: key }`. Task 5's Clear selection uses it.
- **Produces** on `LogList.svelte`: the prop `selectedKey` is replaced by `selection = $bindable<Selection>(EMPTY_SELECTION)` and `multi: boolean` (default true). `onselect(row)` becomes `onselect: (focus: RowView | null, picked: RowView[]) => void`.
- **Produces** in `GitLogView.svelte`: `let picked = $state<RowView[]>([])`. `selected` stays the focused row and keeps driving the single-commit Changes exactly as today. (Task 5 switches to combined mode when `picked.length >= 2`.)

- [ ] **Step 1: Write the failing tests.** Port every Polylog case, with `order = k0..k9` and `one(k)` as there:
  - `it("a plain click selects one commit")`: `click(one("k1"), "k3").next` equals `{ keys: ["k3"], focus: "k3", anchor: "k3" }`.
  - `it("Ctrl-click adds and removes, never the last one")`: adding k5 to k3 gives `{ keys: ["k3","k5"], focus: "k5", anchor: "k5" }`; removing k5 again leaves `keys ["k3"]`, `focus "k3"`; Ctrl-click on the only selected row returns it unchanged.
  - `it("Shift-click selects from the anchor")`: k3 then Shift k6 gives `{ keys: ["k3","k4","k5","k6"], focus: "k6", anchor: "k3" }`; Shift k1 gives `["k1","k2","k3"]`.
  - `it("Ctrl+Shift-click adds the range")`: k1, Ctrl k5, Ctrl+Shift k7 gives `["k1","k5","k6","k7"]`.
  - `it("Shift+arrow extends, a plain arrow goes back to one")`.
  - `it("right-click keeps a selection it is inside, else selects that row")`.
  - `it("more than 100 commits is refused")`: Shift from m0 to m100 over 150 rows gives `overLimit: true` and the selection unchanged; Ctrl-click a 101st does too; m0 to m99 is allowed.
  - `it("a reload keeps the commits still listed")`: the three `keepListed` cases.
  - `it("clickGesture maps Cmd on macOS and Ctrl elsewhere")`, and with `multi: false` returns `{ ctrl: false, shift: false }`.
  - `it("pickedRows keeps list order")`.
- [ ] **Step 2: Run `npm --prefix ui test -- selection`.** Expected: FAIL, module not found.
- [ ] **Step 3: Implement `selection.ts`** as a line-for-line port of `selectionModel.ts`.
- [ ] **Step 4: Run the tests.** Expected: PASS.
- [ ] **Step 5: Wire `LogList.svelte`.**
  - Click: `applyGesture(selection, clickGesture(e, key, isMac, multi), order)`. On `overLimit`, call `onnotice(LIMIT_NOTICE)` and change nothing.
  - Keys: ↑/↓/Home/End/PageUp/PageDown compute the target row with the existing `navigate()`, then apply `{ kind: "arrow", key, shift: e.shiftKey && multi }`. The existing early return on `ctrlKey/metaKey/altKey` stays; Shift no longer returns early.
  - Right-click applies `{ kind: "context" }` before the menu opens. The menu still acts on the right-clicked row only.
  - Rendering: each row in `selection.keys` gets class `selected` and `aria-selected="true"`; the focus row also gets class `focused`. The scroller gets `aria-multiselectable="true"`. Selected rows use the theme's inactive-selection background and the focused row the active one, as the list styles already define for `.selected`.
  - Reload: replace the `keepSelection` effect with `selection = keepListed(selection, rows.map(rowKey))`. When it empties, `onselect(null, [])` as today.
  - Every change of `selection` calls `onselect(focusRow, pickedRows(rows, selection))`.
- [ ] **Step 6: Run `npm --prefix ui test` and `npm --prefix ui run check`.** Expected: PASS, 0 errors. Single click, ↑/↓ and Enter behave exactly as before (the existing `log.test.ts` cases still pass).
- [ ] **Step 7: Commit.**
```bash
git add ui/src/gitlog/selection.ts ui/src/gitlog/selection.test.ts ui/src/gitlog/LogList.svelte ui/src/gitlog/GitLogView.svelte
git commit -m "feat(gitlog): select several commits with Ctrl, Shift and Shift+arrows"
```

### Task 2: Three-way line merge (core, pure)

**Ports:** Polylog `src/lineMerge.ts` and `src/lineMerge.test.ts`. This is the algorithm to port, line for line: `lines` (each line keeps its terminator, so CRLF and a missing final newline survive), `Hunk`, the Myers O(ND) `diff` with its `trace` and the `MAX_EDITS` cap, `sideText`, and `merge3`'s grouping of overlapping **or touching** hunks (`h.start <= end`), which is what makes adjacent edits conflict as `git merge-file` does. Do not substitute the `similar` crate: its alignment differs, and the oracle test pins this one.

**Files:**
- Create `core/src/gitlog/line_merge.rs`; register it in `core/src/gitlog/mod.rs` and re-export `merge3, MergeResult, MAX_EDITS`.
- Test: `core/tests/gitlog_line_merge.rs`.

**Interfaces:**
- **Produces:**
  - `pub const MAX_EDITS: usize = 2000;`
  - `#[derive(Debug, Clone, PartialEq, Eq)] pub enum MergeResult { Clean(String), Conflict, TooLarge }`
  - `pub fn merge3(base: &str, ours: &str, theirs: &str) -> MergeResult`. Lines compare as `&str` slices, terminators included.

- [ ] **Step 1: Write the failing tests.**
```rust
#[test] fn separated_changes_merge() {
    assert_eq!(merge3("a\nb\nc\nd\ne\n", "a\nB\nc\nd\ne\n", "a\nb\nc\nd\nE\n"), MergeResult::Clean("a\nB\nc\nd\nE\n".into()));
}
#[test] fn adjacent_changes_conflict() {
    assert_eq!(merge3("a\nb\nc\n", "a\nB\nc\n", "a\nb\nC\n"), MergeResult::Conflict);
}
#[test] fn overlapping_conflict_unless_identical() {
    assert_eq!(merge3("a\nb\nc\n", "a\nX\nc\n", "a\nY\nc\n"), MergeResult::Conflict);
    assert_eq!(merge3("a\nb\nc\n", "a\nX\nc\n", "a\nX\nc\n"), MergeResult::Clean("a\nX\nc\n".into()));
}
#[test] fn one_side_changed_takes_that_side() { /* the two Polylog cases */ }
#[test] fn empty_sides() {
    assert_eq!(merge3("", "x\n", ""), MergeResult::Clean("x\n".into()));
    assert_eq!(merge3("", "", "y\n"), MergeResult::Clean("y\n".into()));
    assert_eq!(merge3("x\n", "x\n", ""), MergeResult::Clean("".into()));
}
#[test] fn no_trailing_newline_and_crlf_keep_bytes() {
    assert_eq!(merge3("a\nb", "a\nb", "a\nB"), MergeResult::Clean("a\nB".into()));
    assert_eq!(merge3("a\r\nb\r\n", "a\r\nb\r\n", "a\r\nB\r\n"), MergeResult::Clean("a\r\nB\r\n".into()));
}
```
  - `agrees_with_git_merge_file`: the 7 fixed Polylog cases plus 40 random ones from the same generator (LCG `seed = (seed * 1103515245 + 12345) % 2147483648` on `u64`, seed 7; `edit()` inserts `new{n}`, deletes, or replaces with `chg{n}`). The oracle writes `base`, `ours` and `theirs` into a `tempfile::tempdir()` and runs `git merge-file -p ours base theirs` there, with every `GIT_*` variable removed. Exit 0 means clean with stdout as the text; any other exit means conflict. For each case, `Clean`/`Conflict` agrees with the oracle, and a clean text is byte-equal to its stdout. The message names the case.
  - `huge_rewrite_is_too_large`: base is 6,000 lines `line {i}\n`, ours 6,000 lines `other {i}\n`, theirs = base. The result is `MergeResult::TooLarge`, in under 1,500 ms (debug build).
- [ ] **Step 2: Run `cargo test -p claudegate-core --test gitlog_line_merge`.** Expected: FAIL to compile, `merge3` not found.
- [ ] **Step 3: Implement `merge3`** as the port described above. `diff()` returns `None` past `MAX_EDITS`, and `merge3` maps that to `TooLarge`.
- [ ] **Step 4: Run the test.** Expected: PASS, with the oracle case count printed by `eprintln!`.
- [ ] **Step 5: Commit.**
```bash
git add core/src/gitlog/line_merge.rs core/src/gitlog/mod.rs core/tests/gitlog_line_merge.rs
git commit -m "feat(gitlog): three-way line merge, checked against git merge-file"
```

### Task 3: Combined model and the left side (core, pure)

**Ports:** Polylog `src/combinedModel.ts` (`mergeBaseArgs`, `rangeLogArgs`, `parseRangeLog`, `combineRepo`, `netStatus`, `ancestors`) and `src/combinedRead.ts` (`showArgs`, `unmergeable`, `leftSide` including the round-trip guard, `readCombined`), with `combinedModel.test.ts` and `combinedRead.test.ts`. CC §4.1–4.5.

**Files:**
- Create `core/src/gitlog/combined.rs`; register and re-export it in `core/src/gitlog/mod.rs`.
- Modify `core/src/gitlog/discover.rs`: add `par_map_limit`, and make `par_map` call it with `PARALLEL`.
- Test: `core/tests/gitlog_combined.rs`.

**Interfaces:**
- **Consumes:** `merge3`, `MergeResult` (Task 2); `is_sha` (parse.rs).
- **Produces** in `discover.rs`: `pub fn par_map_limit<T: Sync, R: Send>(items: &[T], limit: usize, f: impl Fn(&T) -> R + Sync) -> Vec<R>`, results in input order.
- **Produces** in `combined.rs`:
  - `pub const COMBINED_LIMIT: usize = 100;`, `pub const MERGE_LIMIT_BYTES: u64 = 1_000_000;`, `pub const MERGE_LIMIT_LINES: usize = 20_000;`, `pub const SLOW_FILES_AT_ONCE: usize = 4;`
  - `pub const RANGE_FORMAT: &str = "--format=%x1e%H %P%x1f%s";`
  - `pub struct RawChange { pub status: char, pub path: String, pub old_path: Option<String>, pub old_blob: String, pub new_blob: String }`. `status` is one of `A M D R C T`.
  - `pub struct RangeCommit { pub sha: String, pub parents: Vec<String>, pub subject: Option<String>, pub changes: Vec<RawChange> }`
  - `pub enum NetStatus { Added, Modified, Deleted, Renamed }`
  - `pub struct CombinedFile { pub path: String, pub old_path: Option<String>, pub status: NetStatus, pub shas: Vec<String>, pub last: String, pub before_ref: Option<String>, pub before_path: String, pub included: Vec<String>, pub fast: bool }`. `shas` is oldest first.
  - `pub fn merge_base_args(selected: &[String]) -> Vec<String>`, which gives `["merge-base", "--octopus", ..selected]`.
  - `pub fn exclude_for(base: &str, selected: &[String]) -> Vec<String>`. An empty base (unrelated histories) gives `[]`. A base that is itself selected gives `[format!("{base}^@")]` (its parents; a root commit then excludes nothing, so no `--not` is needed). Any other base gives `[base]`. My call: `^@` replaces Polylog's UI-supplied parents, so the backend never trusts parent ids from the webview.
  - `pub fn range_log_args(selected: &[String], exclude: &[String]) -> Vec<String>`, which gives `["log", "--topo-order", "--raw", "-z", "-M", "--no-abbrev", "--diff-merges=first-parent", RANGE_FORMAT, ..selected, ("--not", ..exclude when non-empty), "--"]`.
  - `pub fn show_raw_args(sha: &str) -> Vec<String>`, which gives `["show", "--raw", "-z", "-M", "--no-abbrev", "--diff-merges=first-parent", RANGE_FORMAT, sha]`.
  - `pub fn parse_range_log(out: &str) -> Vec<RangeCommit>`. This is a **positional** NUL tokenizer, the algorithm the tests cannot fully pin:
    ```text
    tokens = out.split('\0'); i = 0
    while i < tokens.len():
      t = tokens[i].trim_start_matches('\n'); i += 1
      if t is empty: continue
      if !t.starts_with('\x1e'): continue                  // stray token: skip
      header = t[1..]; (ids, subject) = split at the FIRST '\x1f' (subject None if absent)
      sha, parents = ids split on ' ' (empty dropped); skip the record unless is_sha(sha)
      while i < tokens.len():
        r = tokens[i].trim_start_matches('\n')
        if !RAW.is_match(r): break                        // RAW = ^:\d{6} \d{6} [0-9a-f]+ [0-9a-f]+ [AMDRCT]\d*$
        two = status is R or C; paths = tokens[i+1] (and tokens[i+2] when two), taken verbatim
        i += 1 + (2 if two else 1); a missing path token ends the record (cut off by a kill)
    ```
    Paths are never matched against anything: a path that contains `\x1e`, a newline or `:100644 …` is still just a path.
  - `pub fn combine_repo(selected: &HashSet<String>, log: &[RangeCommit]) -> (Vec<CombinedFile>, Vec<String>)`, returning `(files, no_net_change)`. A straight port of `combineRepo` and `netStatus`.
  - `#[derive(Debug, Clone, PartialEq, Eq)] pub enum ReadError { Cancelled, Failed(String) }`
  - `pub enum Blob { Bytes(Vec<u8>), Missing, TooLarge(u64) }`
  - `pub trait GitRead: Sync { fn read(&self, args: &[&str]) -> Result<Vec<u8>, ReadError>; fn blob(&self, spec: &str, max: u64) -> Result<Blob, ReadError>; }`. `spec` is a blob id or `<sha>:<path>`. `blob` never reads more than `max` bytes: the src-tauri side checks `cat-file -s` first.
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum Fallback { Conflict, Binary, TooLarge }`
  - `#[derive(Debug, Clone, PartialEq, Eq)] pub enum LeftSide { Text(String), Fallback(Fallback) }`
  - `pub fn left_side(git: &dyn GitRead, last: &str, path: &str, without: &[String]) -> Result<LeftSide, ReadError>`. `without` is newest first. It is a port of `leftSide`:
    - An all-zero blob id reads as empty without a call.
    - The right side (`last:path`) that is `Missing` starts as empty.
    - Rename following uses `["diff", "--raw", "-z", "-M", "--no-abbrev", sha, state]`.
    - The final round-trip guard must reproduce the right side byte for byte.
    - `unmergeable`: a NUL in the first 8,000 bytes is `Binary`. Bytes that are not UTF-8 are `Binary` (my call, as `file_view_of` does). Over `MERGE_LIMIT_BYTES` (or a `Blob::TooLarge`) is `TooLarge`. More than `MERGE_LIMIT_LINES` newlines is `TooLarge`.
  - `pub enum Mode { Fast, Merge, Fallback }`
  - `pub struct CombinedRead { pub file: CombinedFile, pub mode: Mode, pub reason: Option<Fallback> }`
  - `pub struct RepoRead { pub commits: Vec<String>, pub files: Vec<CombinedRead>, pub no_net_change: Vec<String>, pub subjects: BTreeMap<String, String> }`. `commits` holds the selected shas present in the range log, oldest first (topological). `subjects` covers the unselected commits in range.
  - `pub fn read_combined(git: &dyn GitRead, selected: &[String]) -> Result<RepoRead, ReadError>`, a port of `readCombined`:
    1. Run merge-base; a `Failed` result means an empty base.
    2. Compute `exclude_for`, then read the range log. Its failure is the repo's `Err(Failed)`.
    3. Run `combine_repo`.
    4. For the slow files, at most `SLOW_FILES_AT_ONCE` at a time through `par_map_limit`: run `left_side` against the right blob. Equal texts move the file to `no_net_change`.
    5. Any `Cancelled` returns `Err(Cancelled)`.

- [ ] **Step 1: Write the failing tests.** Port every case of `combinedModel.test.ts` and `combinedRead.test.ts` with the same fixtures and expected values. Polylog's shas `"a"`, `"c"` become 40-char ids where `is_sha` matters (`parse_range_log`, `left_side`); `combine_repo` takes any strings. Test names:
  - `range_log_args_and_root`: `range_log_args(&[c, a], &[p])` equals the list above ending `"--not", p, "--"`; with `&[]` it has no `--not`.
  - `exclude_below_the_base`: `exclude_for("", ..)` is `[]`; `exclude_for(a, &[a, c])` is `[format!("{a}^@")]`; `exclude_for(p, &[a, c])` is `[p]`.
  - `parse_range_log_merges_renames_roots`: Polylog's three-commit fixture, `"old name.ts"` included.
  - `parse_range_log_hostile_names`: a commit whose subject is `"first\x1esub\x1fmore"`, renaming `"a\nb.ts"` to `"x\x1ey.ts"`, and modifying `":100644 100644 0 0 M"`, `"-rf.ts"`, `"tab\there.ts"` and `"old => new.ts"`. It parses to one commit with that exact subject and those exact five paths. A record cut off after its raw header (no path token) drops only that change.
  - `combine_each_file_once` (the spec example: header.ts and header.css included `["x"]`, only-a.ts fast, debug.ts in no-net-change); `later_unselected_outside_range`; `net_status_table` (A, D, M for D→A, M, and the cancelling pair left out); `renames_followed_by_path`; `unselected_merge_in_range_is_slow` (`included == ["t", "m"]`, `fast == false`); `two_branches_take_slow_path`; `later_unselected_rename_does_not_move_file`.
  - The `left_side` cases run against a `FakeGit` (in the test file) that implements `GitRead` from maps: `show --raw` records keyed by the last argument, `diff --raw` output keyed `"diff {a} {b}"`, and blobs keyed by id or `sha:path`. A `blob` larger than `max` returns `TooLarge`. Cases:
    - `show_raw_args_shape`;
    - `left_side_takes_selected_out`: the exact text, keeping rin's line 1;
    - `left_side_overlap_falls_back` (`Fallback::Conflict`);
    - `left_side_binary_and_too_large`: the `PNG\0` blob is `Binary`, 20,001 lines is `TooLarge`, and bytes `[0xff, 0xfe]` are `Binary`;
    - `left_side_follows_unselected_rename`;
    - `left_side_older_than_rename_reads_old_path`;
    - `left_side_past_edit_limit_too_large`;
    - `left_side_lost_path_falls_back`;
    - `left_side_round_trips`: 400 fuzz cases, LCG seed 11, the same pool; every `Text` left re-applies to v3 exactly;
    - `left_side_added_file_is_empty`.
  - `read_combined_modes` (FakeGit, spec example): header.ts is `Merge`, header.css is `Fallback` with `Some(Fallback::Conflict)`, only-a.ts is `Fast`, `no_net_change == ["src/debug.ts"]`, `subjects` holds x's subject, `commits == [a, c]`.
  - `read_combined_cancel_propagates`: a FakeGit whose `blob` returns `Err(Cancelled)` gives `Err(ReadError::Cancelled)`.
  - `par_map_limit_keeps_order_and_limit`: 20 items, limit 4; the results are in input order, and the peak concurrency (an `AtomicUsize` high-water mark) is ≤ 4.
- [ ] **Step 2: Run `cargo test -p claudegate-core --test gitlog_combined`.** Expected: FAIL to compile.
- [ ] **Step 3: Implement** the functions above as ports, with the tokenizer as given.
- [ ] **Step 4: Run the test, then `cargo test -p claudegate-core`.** Expected: PASS, with `no_writes` still green.
- [ ] **Step 5: Commit.**
```bash
git add core/src/gitlog/combined.rs core/src/gitlog/mod.rs core/src/gitlog/discover.rs core/tests/gitlog_combined.rs
git commit -m "feat(gitlog): combined changes model and the selected-changes-out left side"
```

### Task 4: Combined commands, cancellation channels (src-tauri)

**Files:**
- Create `src-tauri/src/gitlog_combined.rs`; register its module and commands in `src-tauri/src/lib.rs`.
- Modify `src-tauri/src/gitlog.rs`:
  - add `channel()` and the `forget_window` change;
  - make `GitLog::cwd_of`, `GitLog::cached`, `Runner::text` and `tree_path_ok` `pub(crate)`;
  - add `GitLog::rev_file`.
- Modify `src-tauri/tests/gitfix/mod.rs`: add `commit_as`.
- Test: `src-tauri/tests/gitlog_combined_cmd.rs`, plus unit tests in `gitlog.rs`'s `mod tests`.

**Interfaces:**
- **Consumes:** `read_combined`, `left_side`, `GitRead`, `ReadError`, `Blob`, `COMBINED_LIMIT`, `MERGE_LIMIT_BYTES` (Task 3).
- **Produces** in `gitlog.rs`:
  - `pub fn channel(window: &str, name: &str) -> String`, which gives `format!("{window}#{name}")`. `Runner::forget_window(w)` now also drops and kills every key starting `"{w}#"`.
  - `pub fn rev_file(&self, repo: &str, rev: &str, path: &str) -> Result<FileView, String>`. It reads `<rev>:<path>`: `is_sha(rev)` and `tree_path_ok(path)` or `Err("Not a commit id" / "Not a path in the repository")`. It runs `cat-file -s` first, so over `MAX_FILE` is `TooLarge` and no object is `Missing`, then `cat-file blob` and `file_view_of`.
  - `#[tauri::command] gitlog_rev_file(repo: String, rev: String, path: String, state) -> Result<FileView, String>`.
- **Produces** in `gitlog_combined.rs`:
  - `#[derive(Deserialize)] #[serde(rename_all = "camelCase")] pub struct CommitRef { pub repo: String, pub sha: String }`
  - Serialized camelCase views:
    - `CombinedView { query_id: u64, repos: Vec<CombinedRepoView> }`;
    - `CombinedRepoView { repo: String, commits: Vec<String>, files: Vec<CombinedFileView>, no_net_change: Vec<String>, subjects: BTreeMap<String, String>, error: Option<String> }`;
    - `CombinedFileView { path, from: Option<String>, status: &'static str /* "added"|"modified"|"deleted"|"renamed" */, shas, last, before_ref: Option<String>, before_path, included, mode: &'static str /* "fast"|"merge"|"fallback" */, reason: Option<&'static str> /* "conflict"|"binary"|"tooLarge" */ }`;
    - `#[serde(tag = "kind", content = "value", rename_all = "camelCase")] pub enum CombinedLeftView { Text(String), Fallback(&'static str) }`.
  - `impl GitLog { pub fn combined(&self, window: &str, query_id: u64, items: &[CommitRef]) -> Result<CombinedView, String>; pub fn combined_left(&self, window: &str, query_id: u64, repo: &str, last: &str, path: &str, without: &[String]) -> Result<CombinedLeftView, String> }`
    - Both use `Tag { window: channel(window, "combined"), query: query_id }`.
    - `combined` calls `set_current` (false gives `Err(STALE)`). `combined_left` only checks `is_current` (false gives `Err(STALE)`), so opening a file never supersedes the tree.
    - Validation, before any git:
      - more than `COMBINED_LIMIT` distinct items gives `Err("Combined Changes shows up to 100 commits")`;
      - a sha or a `without` entry that fails `is_sha` gives `Err("Not a commit id")`;
      - a path that fails `tree_path_ok` gives `Err("Not a path in the repository")`;
      - an unknown repo gives the existing `"Unknown repository; refresh the list"`.
    - Items are grouped by repo; repos run in parallel (`par_map`) and come back in the order of first appearance in `items`.
    - One repo's `Failed` fills that repo's `error` (the first stderr line), and the others still answer. Any `Cancelled` gives `Err(STALE)`.
  - `struct TaggedGit<'a> { runner: &'a Runner, tag: &'a Tag, cwd: &'a Path }` implements `GitRead`. `read` maps `RunError::Cancelled` to `ReadError::Cancelled` and anything else to `Failed`. `blob` runs `cat-file -s` then `cat-file blob`: a failed `-s` is `Missing`, and over `max` is `TooLarge(size)`.
  - Commands:
    - `#[tauri::command] gitlog_combined(query_id: u64, items: Vec<CommitRef>, window: tauri::Window, state) -> Result<CombinedView, String>`;
    - `#[tauri::command] gitlog_combined_left(query_id: u64, repo: String, last: String, path: String, without: Vec<String>, window, state) -> Result<CombinedLeftView, String>`.
  - Both run through `crate::commands::blocking`.
- **Produces** in `gitfix`: `pub fn commit_as(dir: &Path, name: &str, files: &[(&str, Option<&[u8]>)], msg: &str, t: i64) -> String`. This is `commit` with `GIT_AUTHOR_NAME`/`GIT_COMMITTER_NAME` set to `name` and email `{name}@example.com`.

- [ ] **Step 1: Write the failing tests.** The fixture follows CC §6, in temp dirs:
  - **acme-web** starts at `p`, with `src/header.ts` (5 lines) and `src/header.css` (3 lines). Then:
    - `a` by dana: header.ts line 3, header.css line 2, adds `src/debug.ts`;
    - `x` by rin: header.ts line 1, header.css line 2 (the same line as `a`);
    - `c` by dana: header.ts line 5, header.css line 3, deletes debug.ts.
  - **acme-api** gets `d1` (dana, `api/a.go`), `r1` (rin, `api/b.go`) and `d2` (dana, `api/a.go`).
  - Select `a, c, d1, d2`.
  - Tests:
    - `combined_fixture_tree`:
      - acme-web lists header.css (`mode "fallback"`, `reason "conflict"`, `included == [x]`, `status "modified"`) and header.ts (`mode "merge"`, `included == [x]`);
      - `no_net_change == ["src/debug.ts"]`;
      - `subjects[x]` is rin's subject;
      - acme-api lists `api/a.go` with `mode "fast"` and `shas == [d1, d2]`;
      - repos come back in request order.
    - `combined_left_takes_selection_out`:
      - `combined_left(.., c, "src/header.ts", [c, a])` gives `Text` whose line 1 is rin's and whose lines 3 and 5 are `p`'s;
      - for header.css it gives `Fallback("conflict")`;
      - `rev_file(web, c, "src/header.ts")` is `c`'s text.
    - `combined_writes_nothing`:
      - before and after `combined` plus `combined_left` for every file, these are equal: the recursive file count and the total byte size under `.git/objects`, the `git status --porcelain` output, and `.git/index`'s mtime;
      - no file named `*.lock` exists under `.git`.
    - `combined_merges_and_branches`: in svc-lib, `main` has `a`; branch `feat` has `b` (same file, a different line) and `f2` (another file); `m` merges feat into main. Select `a` and `m`:
      - `m`'s files are its first-parent diff: `f2`'s file is listed with `shas == [m]`;
      - the shared file is not `"fast"`;
      - the call succeeds.
      - Then select `a` and `b` (two branches): every file has a mode, and none is `"fast"` unless every selected commit is an ancestor of its `last`.
    - `combined_hostile_names`: files `"a\nb.ts"`, `"-rf.ts"`, `"x\x1ey.ts"` and `"sp ace:colon.ts"`, each modified by two selected commits. All four are listed with exact paths, and `combined_left` for each returns `Text`.
    - `combined_binary_and_huge`: `logo.png` (bytes with NUL) and `gen.txt` (25,000 lines) are each changed by `s1`, unselected `u`, and `s2`. They fall back with reasons `"binary"` and `"tooLarge"`; neither is an error.
    - `combined_superseded_is_stale_and_killed`:
      - with `slow_git` from `gitlog_cmd.rs` (copy it), query 1 on the slow repo blocks;
      - query 2 (a good repo) returns;
      - query 1 returns `Err(STALE)`;
      - every pid query 1 had (`runner.pids(&Tag { window: channel("main", "combined"), query: 1 })`) is dead within 2 s.
    - `combined_channel_leaves_the_log_alone`: a log page query on window `"main"` stuck on the slow repo is **not** killed when a combined query starts on the same window: its pids are still alive after `combined` returns, then cleaned up with `forget_window("main")`.
    - `combined_rejects_bad_input`: 101 items, a non-sha, a non-sha in `without`, and a path `"../x"` each give the exact error strings above.
  - In `gitlog.rs` `mod tests`, `forget_window_kills_every_channel`: register a dummy tag on `"w"` and `"w#combined"` (with `set_current`). After `forget_window("w")`, `is_current` is false for both, while `"wx#combined"` stays current.
- [ ] **Step 2: Run `cargo test -p claudegate-desktop --test gitlog_combined_cmd`.** Expected: FAIL to compile.
- [ ] **Step 3: Implement** the items above.
- [ ] **Step 4: Run it, then `cargo test --workspace`.** Expected: PASS, with `gitlog_cmd` and `no_writes` still green.
- [ ] **Step 5: Commit.**
```bash
git add src-tauri/src/gitlog_combined.rs src-tauri/src/gitlog.rs src-tauri/src/lib.rs src-tauri/tests/gitfix/mod.rs src-tauri/tests/gitlog_combined_cmd.rs
git commit -m "feat(gitlog): combined changes commands on their own cancellation channel"
```

### Task 5: The combined Changes tree and its diff (UI)

**Ports:** CC §3 (the Changes view with several commits); behaviour of `changesTree.ts` combined mode.

**Files:**
- Create `ui/src/gitlog/combined.ts`, test `ui/src/gitlog/combined.test.ts`, and `ui/src/gitlog/CombinedChanges.svelte`.
- Modify `ui/src/gitlog/GitLogView.svelte`, `ui/src/lib/api.ts` (`gitlogCombined`, `gitlogCombinedLeft`, `gitlogRevFile`) and `ui/src/lib/mock.ts` (a combined answer covering fast, merge, every fallback reason, no-net-change and one repo error).

**Interfaces:**
- **Consumes:** the Task 4 commands; `Selection` and `picked` (Task 1); `queryGate`, `isStale`, `shortSha` (log.ts); `buildTree`, `visibleNodes`, `nodeId`, `diffPane` (changes.ts and lib/tree).
- **Produces** in `combined.ts`:
  - The types `NetStatus`, `FallbackReason = "conflict" | "binary" | "tooLarge"`, `CombinedFile`, `CombinedRepo`, `CombinedView` and `CombinedLeft`, mirroring the Task 4 views field for field (camelCase).
  - `export function isCombined(picked: RowView[]): boolean`: `picked.length >= 2`.
  - `export function itemsOf(picked: RowView[]): { repo: string; sha: string }[]`.
  - `export function combinedTitle(commits: number, repos: number): string`.
  - `export function shaList(shas: string[]): string`: short shas joined by `" · "`.
  - `export function warnText(f: CombinedFile, subjects: Record<string, string>): string | null`: null unless `mode === "fallback"`.
  - `export function noNetChangeText(paths: string[]): string | null`.
  - `export function orderRepos(repos: CombinedRepo[], repoOrder: string[]): CombinedRepo[]`: Repo List order (the discovery order of `repos` in GitLogView), unknown ones last.
  - `export function combinedFirst(repos: CombinedRepo[]): { repo: string; file: CombinedFile } | null`: the first file in tree order of the first repo with files.
  - `export function combinedPlan(f: CombinedFile): { left: "rev" | "merge" | "missing"; right: "rev" | "missing" }`.
  - `export type CombinedState = { loading: boolean; view: CombinedView | null; error: string | null }`.
  - `export class CombinedLoad { constructor(fetch: (id: number, items: { repo: string; sha: string }[]) => Promise<CombinedView>, emit: (s: CombinedState) => void, gate?: QueryGate); start(items): Promise<void>; clear(): void; get id(): number }`. It gates every answer on its id, as `LogQuery` does.
- **Produces** `CombinedChanges.svelte`, with props:
  - `title: string`;
  - `repos: CombinedRepo[]`;
  - `repoName: (id: string) => string`;
  - `repoColor: (id: string) => number`;
  - `loading: boolean`;
  - `error: string | null`;
  - `active: { repo: string; path: string } | null`;
  - `theme: IconTheme`;
  - `onopenfile: (repo: string, f: CombinedFile) => void`;
  - `onclear: () => void`;
  - `onescape: () => void`;
  - `onhistory: (repo: string, path: string) => void` (wired in Task 8; until then a no-op).

  It renders repo root rows (chip colour, name, tooltip listing the repo's selected commits by short sha and subject), then one folder tree per repo. Each file row has:
  - the net status letter (`A M D R`) with the existing badge colours;
  - `shaList(f.shas)`;
  - a ⚠ icon with `title={warnText(..)}` when it falls back.

  A message row closes each repo with no-net-change files; an error row stands for a repo whose `error` is set. The keyboard and roving tab stop work as in `Changes.svelte`.

- [ ] **Step 1: Write the failing tests** (`combined.test.ts`):
  - `it("titles the combined tree")`: `combinedTitle(4, 2)` is `"4 commits · 2 repositories"`; `combinedTitle(2, 1)` is `"2 commits · 1 repository"`.
  - `it("lists the shas that touched a file, oldest first")`: `shaList([a40, c40])` is `` `${a7} · ${c7}` ``.
  - `it("explains a fallback")`:
    - conflict with one included gives `"Includes changes from 1 unselected commit: 9c4e1a8 header: wider title"`;
    - two included give `"Includes changes from 2 unselected commits: 9c4e1a8 header: wider title; 1b2c3d4 fix: x"`;
    - conflict with none included gives `"The selected changes can't be shown apart here"`;
    - binary gives `"Binary file: shown as one diff across the selection"`;
    - tooLarge gives `"Too large to take apart: shown as one diff across the selection"`;
    - `mode "merge"` gives `null`.
  - `it("names the files with no net change")`: `["src/debug.ts"]` gives `"1 file with no net change: src/debug.ts"`; two paths give `"2 files with no net change: src/debug.ts, src/nav.ts"`; `[]` gives `null`.
  - `it("plans each side's read")`:
    - fast gives `{ left: "rev", right: "rev" }`;
    - fast with `status "added"` or `beforeRef null` gives `left "missing"`;
    - `status "deleted"` gives `right "missing"`;
    - merge gives `left "merge"`;
    - fallback gives `left "rev"`.
  - `it("orders repositories as the Repo List does")`.
  - `it("opens the first file of the first repository with files")`.
  - `it("a superseded combined answer is ignored")`:
    - `start(A)`, then `start(B)`; B resolves, then A resolves late. The emitted `view` is B's.
    - A rejecting with `"stale query"` after B leaves no error.
    - The current query failing gives `error` set and `loading false`.
- [ ] **Step 2: Run `npm --prefix ui test -- combined`.** Expected: FAIL.
- [ ] **Step 3: Implement `combined.ts`, `CombinedChanges.svelte` and the GitLogView wiring.**
  - When `isCombined(picked)`, the Changes column shows `CombinedChanges` with `title = combinedTitle(picked.length, distinct repos)`, and `combinedLoad.start(itemsOf(picked))` runs, debounced with the existing `leadingDebounce(…, CHANGES_DEBOUNCE_MS)`. Otherwise the existing single-commit path runs, untouched.
  - The **Clear selection** button sits in the Changes header in combined mode, with text `Clear selection` and `title="Clear selection: back to the focused commit"`. It sets the list's selection to `single(focus)`.
  - Opening a file reads both sides per `combinedPlan`:
    - `rev` uses `gitlogRevFile(repo, beforeRef, beforePath)` for the left and `gitlogRevFile(repo, last, path)` for the right;
    - `merge` uses `gitlogCombinedLeft(combinedLoad.id, repo, last, path, [...shas].reverse())`. A `fallback` answer re-reads the left as `rev` and keeps that reason for Task 6.
    - The pane comes from `diffPane`. A `fileGate` drops stale answers, as for one commit.
  - Enter with several commits selected opens `combinedFirst`.
  - "Open in editor" opens the working-tree file of `path` in the main checkout (`gitlogWorktreeFile(repo, path)`). `diffRow.key` is `` `${last}:${path}` ``.
  - Loading copy: `Reading ${n} commits…`. A repo error row reads `` `Couldn't read ${name}: ${error}` ``.
- [ ] **Step 4: Run `npm --prefix ui test` and `npm --prefix ui run check`.** Expected: PASS, 0 errors.
- [ ] **Step 5: Commit.**
```bash
git add ui/src/gitlog/combined.ts ui/src/gitlog/combined.test.ts ui/src/gitlog/CombinedChanges.svelte ui/src/gitlog/GitLogView.svelte ui/src/lib/api.ts ui/src/lib/mock.ts
git commit -m "feat(ui): combined Changes tree: each file once, net status, fallback warning"
```

### Task 6: The diff header notice (UI)

**Ports:** CC §4.3 and §4.6 (the diff title), moved into the diff header bar (spec adaptation "Labels").

**Files:**
- Modify `ui/src/components/DiffView.svelte`: `external.banner`.
- Modify `ui/src/gitlog/combined.ts` and `combined.test.ts` (`combinedNotice`), and `ui/src/gitlog/GitLogView.svelte` (passes it).

**Interfaces:**
- **Consumes:** `CombinedFile`, `CombinedLeft`, `shaList` (Task 5).
- **Produces:**
  - `export type Banner = { tone: "info" | "warn"; text: string };` in `combined.ts`.
  - `export function combinedNotice(f: CombinedFile, subjects: Record<string, string>, left: CombinedLeft | null): Banner | null`. `left` is the `gitlogCombinedLeft` answer for a `merge` file (null otherwise); a fallback there overrides `mode`.
  - In `DiffView.svelte`, `external` gains `banner?: { tone: "info" | "warn"; text: string } | null`. It is drawn directly under `.diff-head` as `<div class="banner {tone}" role="note">`, with a ⚠ glyph for `warn`. `warn` uses `--warning-bg`/`--warning-ink`, and `info` uses the header's muted text colour. Long text wraps; it never truncates. The review (no `external`) never shows it.

- [ ] **Step 1: Write the failing tests** (`combined.test.ts`). In these cases `a7`/`c7` are short shas, `x` is an included sha with subject `header: wider title`, and the strings are exact:
  - `it("says nothing for one commit on the fast path")`: `null`.
  - `it("names the selected commits on the fast path")`: `{ tone: "info", text: "2 selected commits: a3f91c2 · c7d20e5" }`.
  - `it("says unselected edits are context after a clean merge")`: `"2 selected commits: a3f91c2 · c7d20e5 · edits from 1 unselected commit shown as unchanged context"`. With no `included` it is the first part alone. With one sha it is `"1 selected commit: a3f91c2 · …"`.
  - `it("warns when the changes can't be shown apart")`:
    - fallback/conflict gives `{ tone: "warn", text: "a3f91c2^ ↔ c7d20e5 · includes 1 unselected commit: 9c4e1a8 header: wider title" }`;
    - with two included: `"… · includes 2 unselected commits: 9c4e1a8 header: wider title; 1b2c3d4 fix: x"`;
    - with none: `"a3f91c2^ ↔ c7d20e5 · the selected changes can't be shown apart"`.
  - `it("labels binary and too-large fallbacks and still names included commits")`: `"a3f91c2^ ↔ c7d20e5 · binary"`, `"a3f91c2^ ↔ c7d20e5 · too large"`, and binary with one included: `"a3f91c2^ ↔ c7d20e5 · binary · includes 1 unselected commit: 9c4e1a8 header: wider title"`. (My call: unlike Polylog's title, which drops them, a binary or too-large fallback still names unselected commits, per the anti-goal.)
  - `it("a fallback from the left-side read overrides a merge")`: `mode "merge"` with `left = { kind: "fallback", value: "conflict" }` gives the warn text.
- [ ] **Step 2: Run `npm --prefix ui test -- combined`.** Expected: FAIL.
- [ ] **Step 3: Implement** `combinedNotice`, the `banner` rendering, and the GitLogView pass-through (`external.banner`).
- [ ] **Step 4: Run tests and check.** Expected: PASS, 0 errors.
- [ ] **Step 5: Browser preview.**
  - Run the UI against the mock (`#gitlog`). Select two commits so the combined tree shows a fast, a merge and a fallback file, and open each.
  - Screenshot narrow and wide, in dark and light. The banner wraps without pushing the header buttons off.
  - Stop the dev server by its PID, never with `pkill -f`. Move the screenshots out of the repo.
- [ ] **Step 6: Commit.**
```bash
git add ui/src/components/DiffView.svelte ui/src/gitlog/combined.ts ui/src/gitlog/combined.test.ts ui/src/gitlog/GitLogView.svelte
git commit -m "feat(ui): the diff header says what a combined diff includes"
```

## File History

### Task 7: History query (core and src-tauri)

**Ports:** Polylog `src/gitLog.ts` (`HISTORY_FORMAT`, `parseHistory`), `src/filterModel.ts` (`historyArgs`, `historyPathsArgs`, `parseHistoryPaths`) and `src/logQuery.ts` (the history branch of `fetchPage`), with their tests. Polylog's shipped code is the reference over the spec's wording: the first page runs **one** unfiltered `git log --follow` to learn every name the file has had, then pages with `--skip` over all those names **without** `--follow` (`--follow` cannot page with `--skip`). This is my call, as phase 1's paging decision was.

**Files:**
- Create `core/src/gitlog/history.rs`; register and re-export it in `mod.rs`.
- Modify `src-tauri/src/gitlog.rs`: `HistoryReq`, the `page(..)` history mode, `RowView.file`, and `gitlog_page`'s new argument.
- Test: `core/tests/gitlog_history.rs`, and additions to `src-tauri/tests/gitlog_cmd.rs`. Every existing `page(..)` call in `gitlog_cmd.rs` and `gitlog_perf.rs` gains a trailing `None`. The Tauri argument is an `Option`, so the UI, which does not send `history` until Task 8, keeps working.

**Interfaces:**
- **Produces** in `history.rs`:
  - `pub const HISTORY_FORMAT: &str = "--format=%x1e%H%x00%ct%x00%an%x00%ae%x00%s%x00%P";`
  - `pub fn history_paths_args(path: &str, heads: &[String]) -> Vec<String>`, which gives `["log", "--follow", "--name-status", "-z", "-M", "--format=%x1e", ("--end-of-options", ..heads when non-empty), "--", ":(literal){path}"]`.
  - `pub fn parse_history_paths(out: &str, path: &str) -> Vec<String>`: the current path first, then each other name newest first, no repeats.
  - `pub fn history_args(heads: &[String], page: usize, skip: usize, paths: &[String]) -> Vec<String>`, which gives `["log", HISTORY_FORMAT, "--max-count={page}", ("--skip={skip}" when > 0), "--name-status", "-z", "-M", ("--end-of-options", ..heads), "--", ":(literal){p}"…]`. It takes no `Filter`: search, authors, dates and the path filter are set aside by construction.
  - `pub struct HistoryFile { pub path: String, pub from: Option<String>, pub status: Option<FileStatus> }`
  - `pub fn parse_history(out: &str, current: &str) -> (Vec<(Commit, HistoryFile)>, usize)`. The `usize` counts records git emitted (headers with all six fields read, parseable or not), which feeds `--skip`. It is positional on NUL tokens:
    - a record starts at a token beginning `\x1e`, and the next five tokens are fields, taken verbatim;
    - then an optional status token (after stripping leading `\n`, `^[AMDRCT]\d*$`) and its one or two path tokens;
    - a record with no status keeps the previous commit's `from` or `path`, else `current`;
    - a record cut off mid-fields is not counted.
- **Produces** in `gitlog.rs`:
  - `#[derive(Deserialize)] #[serde(rename_all = "camelCase")] pub struct HistoryReq { pub repo: String, pub path: String }`.
  - `pub fn page(&self, window: &str, query_id: u64, repos: &[String], filter: &FilterView, more: bool, history: Option<&HistoryReq>) -> Result<PageView, String>`. With `history`:
    - only `history.repo` is queried, whatever `repos` says;
    - the filter is reduced to its branch;
    - `tree_path_ok(path)`, else `Err("Not a path in the repository")`;
    - the first page reads the names once with `history_paths_args(path, heads)` and keeps them in `QueryState` (`history_paths: Option<Vec<String>>`); a failure becomes that repo's `Failure` row;
    - each page runs `history_args(heads, page, skip, names)` and `parse_history`.
  - `RowView` gains `#[serde(skip_serializing_if = "Option::is_none")] pub file: Option<HistoryFileView>`, where `HistoryFileView { path: String, from: Option<String>, status: Option<&'static str> }` uses the same status words as `ChangedFileView`.
  - `gitlog_page(query_id, repos, filter, more, history: Option<HistoryReq>, window, state)`.

- [ ] **Step 1: Write the failing tests.**
  - `core/tests/gitlog_history.rs`:
    - `history_args_shape`: `history_args(&["HEAD"], 200, 5, &["src/client.ts", "src/old.ts"])` contains `"--skip=5"` and no `"--follow"`, and ends `["--name-status", "-z", "-M", "--end-of-options", "HEAD", "--", ":(literal)src/client.ts", ":(literal)src/old.ts"]`;
    - `a_path_is_never_an_option`: the paths `["--output=/tmp/x"]` end `["--", ":(literal)--output=/tmp/x"]`, and `"app/[id]/page.tsx"` stays literal;
    - `history_paths_args_shape`: Polylog's `origin/prod` case, with heads `["origin/prod"]`;
    - `parse_history_paths_names`: Polylog's two cases;
    - `history_ignores_log_filters`: no argument starts with `--grep`, `--author`, `--since` or `--until`;
    - `parse_history_follows_renames`: Polylog's three-record fixture, plus its cut-off record and its merge-without-status cases, with `HistoryFile` values;
    - `parse_history_hostile_names`: subject `"a\x1eb\x1fc"`; a rename from `"a\nR100\nb.ts"` to `"x\x1ey.ts"`; then a record modifying `"-rf.ts"`. Exact paths and subject come back, and the record count is 2.
  - `src-tauri/tests/gitlog_cmd.rs`:
    - `history_follows_rename`: add `old.ts` (t1), modify it (t2), rename it to `new.ts` (t3), modify `new.ts` (t4), and commit an unrelated `other.ts` (t5). History of `new.ts` gives 4 rows, newest first. Their `file`s are `{new.ts, modified}`, `{new.ts, from old.ts, renamed}`, `{old.ts, modified}` and `{old.ts, added}`.
    - `history_sets_filters_aside`: the same, with `FilterView { search: "zzz", authors: ["nobody"], within: 1, path: "other.ts" }`, still gives 4 rows.
    - `history_pages_with_skip`: `with_page_size(2)` gives 2 rows then 2 more with `done == true`, and no row repeats.
    - `history_only_its_repo`: `repos = [a, b]` with history in `b` gives only `b` rows.
    - `history_keeps_branch`: branch filter `feat`, where `feat` has one extra commit to the file, gives that commit too.
    - `history_rejects_bad_path`: `"../x"` gives `Err("Not a path in the repository")`.
- [ ] **Step 2: Run `cargo test -p claudegate-core --test gitlog_history` and `cargo test -p claudegate-desktop --test gitlog_cmd`.** Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run `cargo test --workspace`.** Expected: PASS.
- [ ] **Step 5: Commit.**
```bash
git add core/src/gitlog/history.rs core/src/gitlog/mod.rs core/tests/gitlog_history.rs src-tauri/src/gitlog.rs src-tauri/tests/gitlog_cmd.rs src-tauri/tests/gitlog_perf.rs
git commit -m "feat(gitlog): file history across renames, paged like the log"
```

### Task 8: File History in the view (UI)

**Files:**
- Modify:
  - `ui/src/gitlog/log.ts` and `log.test.ts`;
  - `ui/src/gitlog/LogList.svelte`;
  - `ui/src/gitlog/GitLogView.svelte`;
  - `ui/src/gitlog/Changes.svelte`, which gains a context menu with the existing `ContextMenu` component and the `h` key;
  - `ui/src/gitlog/CombinedChanges.svelte`, which gains the context menu;
  - `ui/src/gitlog/Filters.svelte`, which gains a `disabled` prop;
  - `ui/src/lib/api.ts` and `ui/src/lib/mock.ts`.

**Interfaces:**
- **Consumes:** `gitlog_page(.., history)` and `RowView.file` (Task 7); `clickGesture(.., multi)` (Task 1).
- **Produces** in `log.ts`:
  - `export type HistoryReq = { repo: string; path: string };`
  - `RowView` gains `file?: { path: string; from: string | null; status: FileStatus | null } | null`.
  - `Request` becomes `{ repos: string[]; filter: FilterView; history: HistoryReq | null }`.
  - `PageFetcher` and `gitlogPage(queryId, repos, filter, more, history)` pass it.
  - `export function historyRequest(f: FilterState, h: HistoryReq): Request`.
  - `export function historyBanner(path: string): string`.
  - `export const HISTORY_KEY = "h";`
- **Produces** in GitLogView:
  - `let history = $state<HistoryReq | null>(null)`.
  - `openHistory(repo, path)` sets it, which restarts the list with `historyRequest(filter, history)`.
  - `closeHistory()` clears it, and the list restarts with the untouched `filter`.
  - While history is open:
    - the banner sits above the list: the text `historyBanner(path)` (the path in `<code>`), then a ✕ button with `aria-label="Close File History"` and `title="Close File History"`;
    - `Filters` gets `disabled` (inert, `title="Set aside while File History is open"`);
    - `LogList` gets `multi={false}`;
    - the diff follows the selection: `pickFile(files, row.file?.path ?? null)`.
- **Produces** a menu item `{ id: "history", label: "File History" }` on file rows in `Changes.svelte` (and the `h` key on a focused file row) and in `CombinedChanges.svelte`. It calls `onhistory(repo, path)`.

- [ ] **Step 1: Write the failing tests** (`log.test.ts`):
  - `it("history sets search, authors, Me, path and dates aside and keeps the branch")`:
    - `historyRequest({ ...DEFAULT_FILTER, search: "fix", authors: ["dana"], mine: true, path: "src", range: "7d", branch: "release-1.4" }, { repo: R, path: "a.ts" })` equals `{ repos: [R], filter: { branch: "release-1.4" }, history: { repo: R, path: "a.ts" } }`;
    - with an empty branch, `filter` is `{}`.
  - `it("names the file in the banner")`: `historyBanner("src/new.ts")` is `"History of src/new.ts"`.
  - `it("the diff follows the file's path at each commit")`: `pickFile(files, row.file.path)` picks `old.ts` for a row whose `file.path` is `old.ts`, even when `new.ts` was open.
  - The existing `LogQuery` tests pass `history: null` in every `Request`.
- [ ] **Step 2: Run `npm --prefix ui test -- log`.** Expected: FAIL.
- [ ] **Step 3: Implement** the items above.
- [ ] **Step 4: Run tests and check.** Expected: PASS, 0 errors.
- [ ] **Step 5: Commit.**
```bash
git add ui/src/gitlog/log.ts ui/src/gitlog/log.test.ts ui/src/gitlog/LogList.svelte ui/src/gitlog/GitLogView.svelte ui/src/gitlog/Changes.svelte ui/src/gitlog/CombinedChanges.svelte ui/src/gitlog/Filters.svelte ui/src/lib/api.ts ui/src/lib/mock.ts
git commit -m "feat(ui): File History from a changed file, with the filters set aside"
```

## Blame

### Task 9: Blame on the cursor line (core, src-tauri, UI)

**Ports:** Polylog `src/blameModel.ts` (`parseBlame`, `formatBlame`) and `src/blameView.ts` (what is drawn and when). The desktop reads **one line** with `git blame --porcelain -L n,n <rev> -- <path>`, debounced (spec).

**Files:**
- Create `core/src/gitlog/blame.rs` (registered in `mod.rs`), with test `core/tests/gitlog_blame.rs`.
- Create `ui/src/gitlog/blame.ts`, with test `ui/src/gitlog/blame.test.ts`.
- Modify:
  - `src-tauri/src/gitlog.rs` (`GitLog::blame` and its command) and `src-tauri/src/lib.rs`;
  - `src-tauri/src/config.rs` (`blame` setting) and `src-tauri/tests/gitlog_config.rs`;
  - `src-tauri/tests/gitlog_cmd.rs`;
  - `ui/src/components/DiffView.svelte` (`external.blame`) and `ui/src/gitlog/GitLogView.svelte`;
  - `ui/src/gitlog/repos.ts` (`GitLogConfig.blame`);
  - `ui/src/lib/api.ts` and `ui/src/lib/mock.ts`.

**Interfaces:**
- **Produces** in `blame.rs`:
  - `pub fn blame_args(rev: &str, path: &str, line: u32) -> Vec<String>`, which gives `["blame", "--porcelain", "-L{line},{line}", rev, "--", path]`.
  - `pub struct BlameLine { pub sha: String, pub author: String, pub email: String, pub time: i64, pub summary: String }`.
  - `pub fn parse_blame_porcelain(out: &str) -> Option<BlameLine>`. The first line is `<sha> <orig> <final> <n>` (40 or 64 hex). Then come the `author`, `author-mail` (`<>` stripped), `author-time` and `summary` keys. `filename` and `boundary` are ignored.
- **Produces** in `gitlog.rs`:
  - `pub fn blame(&self, window: &str, query_id: u64, repo: &str, rev: &str, path: &str, line: u32) -> Result<Option<BlameLineView>, String>`:
    - channel `"blame"`, with `set_current`;
    - `is_sha(rev)`, `tree_path_ok(path)` and `line >= 1`, else the existing error strings;
    - `RunError::Failed` gives `Ok(None)` (past the end, a deleted side), and `Cancelled` gives `Err(STALE)`.
  - `#[tauri::command] gitlog_blame(query_id: u64, repo: String, rev: String, path: String, line: u32, window, state) -> Result<Option<BlameLineView>, String>`. `BlameLineView` is camelCase.
- **Produces** in `config.rs`:
  - `GitLogConfig.blame: bool`, default `true` when absent or not a bool;
  - `pub fn gitlog_set_blame(path: &Path, on: bool, guard: Option<&Path>) -> Result<GitLogConfig, String>`, which sets `gitLog.blame` only;
  - `#[tauri::command] gitlog_blame_setting(on: bool) -> Result<GitLogConfig, String>`.
- **Produces** in `blame.ts`:
  - `export const BLAME_DEBOUNCE_MS = 150;`
  - `export type BlameLine = { sha: string; author: string; email: string; time: number; summary: string };`
  - `export type BlameRef = { repo: string; rev: string; path: string };`
  - `export type BlameSides = { original: BlameRef | null; modified: BlameRef | null };`
  - `export function blameText(b: BlameLine, now: number): string`, giving `` `${subject}, ${author} (${commitAge(time, now)})` ``. A subject over 50 characters is its first 49 plus `…`.
  - `export function escapeMarkdown(s: string): string`, which backslash-escapes `` \ ` * _ { } [ ] ( ) # + - . ! | < > ``.
  - `export function blameHover(b: BlameLine, now: number): string`, giving `` `${author} <${email}>, ${age} (${commitDate(time)})\n\n${summary}\n\n\`${shortSha(sha)}\`` ``, with every interpolated value passed through `escapeMarkdown`.
  - `export function commitBlameSides(row: RowView, f: ChangedFile): BlameSides`. The original side is `(row.parents[0], beforeFrom(f) ?? f.path)`, or null for an added file or a root commit. The modified side is `(row.sha, f.path)`, or null for a deleted file.
  - `export function combinedBlameSides(repo: string, f: CombinedFile, leftMerged: boolean): BlameSides`. The original side is null when the left is a merged text or `beforeRef` is null, else `(beforeRef, beforePath)`. The modified side is `(last, path)`, or null for deleted.
- **Produces** in `DiffView.svelte`: `external.blame?: { on: boolean; at: (side: "original" | "modified", line: number) => Promise<BlameLine | null>; toggle: (on: boolean) => void } | null`.
  - With `on`, a cursor the user placed (the existing `onDidChangeCursorPosition` mouse/keyboard guard) is debounced by `BLAME_DEBOUNCE_MS`. Then `at(side, line)` runs, and if the cursor is still on that line, one `createDecorationsCollection` per side gets a single decoration at the line's end: `after: { content: blameText(..), inlineClassName: "cg-blame" }` and `hoverMessage: { value: blameHover(..) }`.
  - A new file, the other side gaining the cursor, or `on` turning false clears the decoration.
  - A header toggle button, shown only when `external.blame` is set, has `aria-pressed={on}`, `title="Blame: who last changed the cursor's line"` and the label `Blame`.
  - `cg-blame` uses the muted text token.
- GitLogView sets `at` to call `gitlogBlame(queryGate.begin(), ref.repo, ref.rev, ref.path, line)`, where `ref` comes from the matching side of `commitBlameSides` or `combinedBlameSides` (null side gives null). `toggle` calls `gitlogBlameSetting(on)` and takes the returned config. My call: the setting lives on the diff header, where its effect shows, rather than in the Settings screen, which does not hold `gitLog` settings today.

- [ ] **Step 1: Write the failing tests.**
  - `core/tests/gitlog_blame.rs`:
    - `blame_args_shape`: the path `"-x.ts"` comes after `"--"`;
    - `parses_porcelain`: a sample with `boundary` and `filename "x\ny.txt"` gives the exact `BlameLine`;
    - `empty_is_none`;
    - `sha256_header`.
  - `src-tauri/tests/gitlog_cmd.rs`:
    - `blame_names_the_line_author`: ann commits two lines (t1), then dana changes line 2 (t2). At t2, line 1 is ann and line 2 is dana; at t1, line 2 is ann.
    - `blame_past_the_end_is_none`;
    - `blame_deleted_side_is_none`: the rev where the file is gone gives `Ok(None)`.
  - `src-tauri/tests/gitlog_config.rs`, `blame_setting_is_one_key`:
    - the default is `true`;
    - `gitlog_set_blame(false)` leaves `gitLog.blame == false` and every other key in config.json byte-identical;
    - a non-bool `blame` reads as `true`.
  - `ui/src/gitlog/blame.test.ts`:
    - `it("formats the end-of-line text")`, including the 50-character cut;
    - `it("escapes markdown in the hover")`: a subject `` "*bold* [x](javascript:alert(1))" `` contains no unescaped `[`, `(` or `*`;
    - `it("blames each side at its own revision")`: the `commitBlameSides` cases for modified, added, deleted, renamed and root-commit files;
    - `it("does not blame a merged left side")`: `combinedBlameSides(.., leftMerged=true).original === null`.
- [ ] **Step 2: Run the four test targets.** Expected: FAIL.
- [ ] **Step 3: Implement** the items above.
- [ ] **Step 4: Run `cargo test --workspace`, `npm --prefix ui test` and `npm --prefix ui run check`.** Expected: PASS, 0 errors.
- [ ] **Step 5: Browser preview.** Against the mock, place the cursor on a line on each side. The end-of-line text appears and the hover shows the details; the toggle hides them. Screenshot dark and light. Stop the server by PID.
- [ ] **Step 6: Commit.**
```bash
git add core/src/gitlog/blame.rs core/src/gitlog/mod.rs core/tests/gitlog_blame.rs src-tauri/src/gitlog.rs src-tauri/src/lib.rs src-tauri/src/config.rs src-tauri/tests/gitlog_cmd.rs src-tauri/tests/gitlog_config.rs ui/src/gitlog/blame.ts ui/src/gitlog/blame.test.ts ui/src/components/DiffView.svelte ui/src/gitlog/GitLogView.svelte ui/src/gitlog/repos.ts ui/src/lib/api.ts ui/src/lib/mock.ts
git commit -m "feat(gitlog): blame the cursor's line in Git log diffs"
```

## Compare Branches

### Task 10: Compare reads (core and src-tauri)

**Ports:** Polylog `src/compareModel.ts` (the shipped arg builders and parsers: `revParseArgs` with trees, `sideCountArgs` with `--no-merges`, `filesArgs` with `--raw --numstat -z -M`, `bothPaths`, `tipPaths`, `pairDuplicates`, `mirror`, `sidePage`) and `src/compareStore.ts` (`readOne`, `readFiles`, `readCommits`, `readBranches`), with `compareModel.test.ts`. CB §4. The shipped code is the reference where it differs from CB §4.2. Two examples: the same tree means identical, and merges are not counted.

**Files:**
- Create `core/src/gitlog/compare.rs` (registered in `mod.rs`), with test `core/tests/gitlog_compare.rs`.
- Create `src-tauri/src/gitlog_compare.rs`, registered in `lib.rs`.
- Modify `src-tauri/src/gitlog.rs`: add `Runner::run_code`, and make `run` a wrapper over it.
- Test: `src-tauri/tests/gitlog_compare_cmd.rs`.

**Interfaces:**
- **Produces** in `compare.rs`:
  - `pub struct Pair { pub left: String, pub right: String }` and `pub fn valid_pair(p: &Pair) -> bool`, which is `is_valid_ref` on both.
  - `pub fn rev_parse_args(p: &Pair) -> Vec<String>` and `pub fn parse_rev_parse(out: &str) -> Option<(String, String, bool)>`, returning `(left, right, same_tree)`.
  - `pub fn merge_base_args(l: &str, r: &str) -> Vec<String>`.
  - `pub enum Side { Left, Right }`.
  - `pub fn side_count_args(side: Side, l: &str, r: &str) -> Vec<String>` and `pub fn parse_side_count(out: &str) -> (u32, u32)`, returning `(only, same)`.
  - `pub fn files_args(base: &str, tip: &str) -> Vec<String>`.
  - `pub fn parse_diff_z(out: &str) -> Vec<ChangedFile>`. This is a positional parse of the `--raw` records (status, one or two paths) followed by the `--numstat -z` records (`a\td\tpath\0`, or `a\td\t\0old\0new\0` for a rename). Paths are verbatim. A numstat of `-` means binary (`None`).
  - `pub fn both_paths(a: &[ChangedFile], b: &[ChangedFile]) -> Vec<String>`, sorted.
  - `pub fn tip_paths(side: Side, path: &str, from: Option<&str>) -> (String, String)`, returning `(left_path, right_path)`.
  - `pub const SIDE_FORMAT: &str = "--format=%m%x00%H%x00%P%x00%ct%x00%aN%x00%s%x00";`. My call: NUL fields counted, as `LOG_FORMAT` is, instead of Polylog's US/RS, which a subject can forge.
  - `pub fn side_log_args(side: Side, l: &str, r: &str, max: usize) -> Vec<String>`.
  - `pub struct SideCommit { pub mark: char /* '+'|'=' */, pub sha: String, pub parents: Vec<String>, pub time: i64, pub author: String, pub subject: String }`, and `pub fn parse_side_log(out: &str) -> Vec<SideCommit>`.
  - `pub struct Duplicate { pub subject: String, pub left: Option<String>, pub right: Option<String> }`, and `pub fn pair_duplicates(left: &[SideCommit], right: &[SideCommit]) -> Vec<Duplicate>`.
  - `pub fn branch_refs_args() -> Vec<String>`, which gives `["for-each-ref", "--format=%(refname)", "refs/heads", "refs/remotes"]`. `pub fn parse_branch_refs(out: &str) -> (Vec<String>, Vec<String>)` returns `(names without refs/heads/ or refs/remotes/, remote names)` and drops `refs/remotes/*/HEAD`.
  - `pub const COMMIT_PAGE: usize = 500;`
- **Produces** in `gitlog.rs`: `pub fn run_code(&self, tag: Option<&Tag>, cwd: &Path, args: &[&str]) -> Result<(i32, Vec<u8>), RunError>`. A non-zero exit is not an error here. A child killed by supersede is `Cancelled`.
- **Produces** in `gitlog_compare.rs` (channel `"compare"`; every call validates `valid_pair` or `is_sha`):
  - `#[serde(tag = "kind", rename_all = "camelCase")] pub enum RepoCompareView { Missing, Identical, Nobase, Error { reason: String }, Differs { left_sha: String, right_sha: String, base: String, left: u32, right: u32, same_left: u32, same_right: u32 } }`, with fields camelCase. It is wrapped as `RepoCompareRow { repo: String, #[serde(flatten)] result: RepoCompareView }`.
  - `pub struct CompareView { pub query_id: u64, pub rows: Vec<RepoCompareRow> }`.
  - `impl GitLog`:
    - `compare(window, query_id, repos: &[String], left: &str, right: &str) -> Result<CompareView, String>`. It uses `set_current`, then per repo, in parallel:
      - `rev_parse_args`: a failure or an unparseable answer is `Missing`, and the same tree is `Identical`;
      - then merge-base (via `run_code`; exit 1 is `Nobase`) and both side counts, concurrently;
      - any other failure is `Error`.
      - Rows come back in `repos` order.
    - `compare_files(window, query_id, repo, left_sha, right_sha, base) -> Result<CompareFiles, String>`, where `CompareFiles { left: Vec<ChangedFileView>, right: Vec<ChangedFileView>, both: Vec<String> }`. It checks `is_current` only.
    - `compare_commits(window, query_id, repo, side: &str /* "left"|"right" */, left_sha, right_sha, max: usize) -> Result<Vec<SideCommitView>, String>`. `max` is clamped to `1..=10_001`. It checks `is_current` only.
    - `branches(repos: &[String]) -> Result<Vec<BranchNameView>, String>`, where `BranchNameView { name: String, count: usize, remote: bool }`. It is untagged and runs one `for-each-ref` per repo. It is sorted by count descending, then name.
  - Commands: `gitlog_compare`, `gitlog_compare_files`, `gitlog_compare_commits`, `gitlog_branches`, with the same arguments plus `window` and `state`. Diffs reuse `gitlog_rev_file` (Task 4); a commit's files reuse `gitlog_changes` and `gitlog_file`.

- [ ] **Step 1: Write the failing tests.**
  - `core/tests/gitlog_compare.rs`, porting every `compareModel.test.ts` case that `compare.rs` covers, with the same values:
    - `a_pair_is_two_branch_names`;
    - `rev_parse_same_tree_is_identical`;
    - `side_counts`;
    - `files_parse_status_rename_counts`: Polylog's `raw` fixture, as `ChangedFile`;
    - `both_matches_new_path`;
    - `tip_paths_follow_a_rename`;
    - `side_log_marks_duplicates`, rewritten for `SIDE_FORMAT`;
    - `duplicates_pair_by_subject`;
    - `branch_refs_skip_head`: `refs/remotes/origin/HEAD` is dropped, and the remotes are `["origin"]`.
  - Hostile input:
    - `parse_diff_z_hostile_names`: a rename from `"a\nb.go"` to `"c\x1ed.go"`, plus `"-x.go"` and `"1\t2\tz.go"` as a path, parse to exact paths and counts;
    - `side_log_hostile_subject`: the subject `"x\x1fy\x1ez"` comes back intact.
  - `src-tauri/tests/gitlog_compare_cmd.rs`. Fixture branches `main` and `release-1.4`:
    - **acme-api** has left-only, right-only, and one commit cherry-picked onto both;
    - **acme-web** has left-only commits;
    - **svc-lib** is identical;
    - **acme-old** has no `release-1.4`;
    - **acme-docs**' `release-1.4` is an orphan branch (no common history).

    Tests:
    - `compare_counts_and_kinds`: the per-repo kinds, and acme-api's `left`/`right`/`same_left`/`same_right`;
    - `compare_files_both_tag`: a file changed on both sides is in `both`;
    - `compare_commits_marks_duplicates`;
    - `compare_superseded_is_stale`: slow_git, as in Task 4;
    - `compare_writes_nothing`: `.git/objects` and `git status --porcelain` are unchanged;
    - `branches_counted_across_repos`: `main` has count 5, and `origin/main` (from `git update-ref refs/remotes/origin/main …`, with `refs/remotes/origin/HEAD` set) is `remote: true` and listed once;
    - `run_code_reports_exit`: merge-base of unrelated commits gives `(1, _)`.
- [ ] **Step 2: Run `cargo test -p claudegate-core --test gitlog_compare` and `cargo test -p claudegate-desktop --test gitlog_compare_cmd`.** Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run `cargo test --workspace`.** Expected: PASS.
- [ ] **Step 5: Commit.**
```bash
git add core/src/gitlog/compare.rs core/src/gitlog/mod.rs core/tests/gitlog_compare.rs src-tauri/src/gitlog_compare.rs src-tauri/src/gitlog.rs src-tauri/src/lib.rs src-tauri/tests/gitlog_compare_cmd.rs
git commit -m "feat(gitlog): compare two branches across repositories"
```

### Task 11: Branch picker and its storage (src-tauri config, UI)

**Ports:** Polylog `src/compareModel.ts` (`pushRecent`, `pickerGroups`, `tabTitle`) and `src/webview/compare/picker.ts`. CB §3.2.

**Files:**
- Modify `src-tauri/src/config.rs` (four writers, five commands registered in `lib.rs`) and `src-tauri/tests/gitlog_config.rs`.
- Create `ui/src/gitlog/compare.ts`, `ui/src/gitlog/compare.test.ts` and `ui/src/gitlog/BranchPicker.svelte`.
- Modify `ui/src/gitlog/repos.ts` (`GitLogConfig` fields), `ui/src/lib/api.ts` and `ui/src/lib/mock.ts`.

**Interfaces:**
- **Produces** in `config.rs`. Every writer is one change under `CONFIG_LOCK` via `update_gitlog`. Branch names must pass `is_valid_ref`, else `Err("Not a branch name.")`.
  - `GitLogConfig` gains:
    - `compare_pair: Option<PairView>`;
    - `compare_recent: Vec<PairView>`;
    - `compare_favorites: Vec<String>`;
    - `compare_mode: String`, `"files"` by default (`"commits"` otherwise).

    `PairView { left: String, right: String }` is camelCase. Malformed entries read as absent.
  - `pub fn gitlog_set_compare_pair(path, left: &str, right: &str, guard) -> Result<GitLogConfig, String>` sets `gitLog.comparePair`.
  - `pub fn gitlog_push_recent_pair(path, left: &str, right: &str, guard) -> Result<GitLogConfig, String>` puts the pair first in `gitLog.compareRecent`, drops its earlier copy and keeps 5. It reads the stored list under the lock; it never takes a list from the UI.
  - `pub fn gitlog_favorite_branch(path, name: &str, guard)` is add-only, and `pub fn gitlog_unfavorite_branch(path, name: &str, guard)` is remove-only, both on `gitLog.compareFavorites`.
  - `pub fn gitlog_set_compare_mode(path, mode: &str, guard)` takes `"files"` or `"commits"`, else `Err("Unknown mode.")`.
  - The commands are `gitlog_compare_pair(left, right)`, `gitlog_compare_recent(left, right)`, `gitlog_favorite(name)`, `gitlog_unfavorite(name)` and `gitlog_compare_mode(mode)`.
- **Produces** in `compare.ts`:
  - `export type Pair = { left: string; right: string };`
  - `export type BranchName = { name: string; count: number; remote: boolean };`
  - `export type PickerItem = { kind: "pair"; pair: Pair } | { kind: "name"; name: string; count: number; favorite: boolean };`
  - `export type PickerGroup = { title: string; items: PickerItem[] };`
  - `export function pickerGroups(names: BranchName[], favorites: string[], recent: Pair[], query: string): PickerGroup[]`. The groups are `Recent pairs` (hidden while searching), `Favorites`, `Local` and `Remote`. Local versus Remote comes from `remote`, which is my call: it replaces Polylog's module-level remote list. Matching is a case-insensitive substring, and items sort by count, then name.
  - `export function highlight(name: string, query: string): { pre: string; hit: string; post: string }`: the first match only.
  - `export function inReposText(n: number): string`, giving `"in 1 repository"` or `"in 3 repositories"`.
  - `export function noMatchText(q: string): string`, giving `` `No branch named "${q}" in any repository.` ``.
  - `export function tabTitle(p: Pair | null): string`.
- **Produces** `BranchPicker.svelte`, with props `names: BranchName[] | null`, `favorites: string[]`, `recent: Pair[]`, `anchor: HTMLElement`, `onpick: (name: string) => void`, `onpair: (p: Pair) => void`, `onfavorite: (name: string, on: boolean) => void` and `onclose: () => void`.
  - The search box is focused on open. ↑/↓ move, Enter picks, and Esc or a click outside closes and returns focus to the anchor.
  - A ★ toggles a favourite. The matched part of a name is marked with `<mark>`. Nothing is read from git while typing.

- [ ] **Step 1: Write the failing tests.**
  - `gitlog_config.rs`:
    - `compare_pair_is_one_key`: the other keys are byte-identical, and a second set replaces only the pair;
    - `recent_pairs_front_dedupe_cap5`: Polylog's `pushRecent` sequence gives `[p1, p2]`, then a cap of 5 after 9 more;
    - `favorites_add_and_remove_only`: adding twice keeps one; removing a missing name is a no-op that writes nothing;
    - `compare_mode_validates`;
    - `invalid_branch_names_refused`: `"-x"`, `"a..b"` and `"x y"` each give `Err("Not a branch name.")`.
  - `compare.test.ts`:
    - `it("groups and filters branch names")`: Polylog's `names`/`groups`/`searched`/`zzz` case, with `remote` flags `[true, false, true, false]` for `[origin/main, main, origin/release-1.4, feat/billing]`;
    - `it("marks the matched part")`: `highlight("origin/release-1.4", "REL")` gives `{ pre: "origin/", hit: "rel", post: "ease-1.4" }`;
    - `it("counts repositories")`;
    - `it("says when no branch matches")`;
    - `it("titles the pair without origin/")`: Polylog's three `tabTitle` cases.
- [ ] **Step 2: Run the targets.** Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run `cargo test --workspace`, `npm --prefix ui test` and `npm --prefix ui run check`.** Expected: PASS.
- [ ] **Step 5: Commit.**
```bash
git add src-tauri/src/config.rs src-tauri/src/lib.rs src-tauri/tests/gitlog_config.rs ui/src/gitlog/compare.ts ui/src/gitlog/compare.test.ts ui/src/gitlog/BranchPicker.svelte ui/src/gitlog/repos.ts ui/src/lib/api.ts ui/src/lib/mock.ts
git commit -m "feat(gitlog): branch picker with recent pairs and favourites"
```

### Task 12: Compare mode in the Git log view (UI)

**Ports:** CB §3.1 and §3.3–3.6 (toolbar, repo list, columns, On both strip, messages), and Polylog `compareModel.ts` (`summaryLabel`, `repoCounts`, `mirror`, `sidePage`).

**Files:**
- Create `ui/src/gitlog/ComparePane.svelte` and `ui/src/gitlog/CompareColumns.svelte`.
- Modify `ui/src/gitlog/compare.ts` and `compare.test.ts`, `ui/src/gitlog/blame.ts` (`compareBlameSides`), `ui/src/gitlog/GitLogView.svelte`, `ui/src/lib/api.ts` and `ui/src/lib/mock.ts` (a compare answer with every row kind).

**Interfaces:**
- **Consumes:** Task 10's commands, Task 11's picker and writers, `gitlogRevFile` (Task 4), `external.banner` (Task 6) and `external.blame` (Task 9).
- **Produces** in `compare.ts`:
  - `export type RepoCompare` (the Task 10 view, `kind` tagged) and `export type CompareRow = { repo: string } & RepoCompare`.
  - `export function summaryLabel(rows: RepoCompare[], ticked: number): string`.
  - `export function repoCounts(c: RepoCompare): string`.
  - `export function mirror(c: RepoCompare): RepoCompare`: Swap mirrors the result it has and does not read again.
  - `export function missingText(p: Pair, n: number): string`.
  - `export function columnHeader(side: "left" | "right", mode: "files" | "commits", n: number, p: Pair): string`.
  - `export function emptyColumnText(side: "left" | "right", p: Pair): string`.
  - `export function bothStripText(n: number): string`.
  - `export function compareMessage(p: Pair | null, ticked: number, rows: RepoCompare[] | null): string | null`.
  - `export function sidePage<T extends { mark: "+" | "=" }>(commits: T[], limit: number): { rows: T[]; more: boolean }`.
  - `export function compareBanner(side: "left" | "right", p: Pair): { tone: "info"; text: string }`.
  - `export class CompareLoad`, gated like `CombinedLoad`.
- **Produces** in `blame.ts`: `export function compareBlameSides(repo: string, base: string, tip: string, path: string, from: string | null): BlameSides`. The original side is `(base, from ?? path)` and the modified side is `(tip, path)`.
- **Produces** in GitLogView, a segmented `[ Log │ Compare ]` control above the filter bar (`role="group"`, `aria-pressed`). Compare is a third mode of the view, not a new top-level view.
  - In Compare mode, the filter bar and commit list give way to `ComparePane`. The Repos pane stays, and its ticked repos are the scope: a tick change starts a new `CompareLoad` with a new id. The diff row on top stays.
  - Nothing compares until Compare mode is shown. It opens on `cfg.comparePair`; with none, the picker opens on the left box.
- **Produces** `ComparePane.svelte`:
  - the toolbar `◀ left ▾`, `⇄`, `▶ right ▾`, `Files | Commits` (mode stored with `gitlogCompareMode`), `↻`, and the summary;
  - a list of only the differing repos, with chip and `repoCounts`, plus the rows that say why they can't be compared (`no common history`, `git error` with the reason in the tooltip), as a listbox with ↑/↓;
  - the Missing footer, a `<details>` listing the names;
  - picking a pair calls `gitlogComparePair` and `gitlogCompareRecent`.
- **Produces** `CompareColumns.svelte`:
  - two columns with sticky headers. Files mode shows a folder tree per side with `+a −d` or `binary` and a `both` tag whose tooltip is `Changed on both sides since the split: look at it before merging`. Commits mode shows each side's commits, each expanding to its files through `gitlogChanges`.
  - the On both strip.
  - A file opens the diff from `gitlogRevFile(repo, base, from ?? path)` to `gitlogRevFile(repo, tip, path)` (tip paths per `tip_paths`), with `external.banner = compareBanner(side, pair)` and blame from `compareBlameSides`. A commit's file opens as the single-commit diff does.
  - "Show 500 more" refetches with `max = shown + COMMIT_PAGE + 1`.

- [ ] **Step 1: Write the failing tests** (`compare.test.ts`; exact strings from CB §3 and Polylog's tests):
  - `it("summarises the comparison without commit numbers")`: Polylog's three `summaryLabel` cases.
  - `it("counts each side and the duplicates, zeros left out")`. The format is the desktop spec's `◀n ▶n =n`, not Polylog's shipped `3 ◀ · 1 ▶`:
    - left 3, right 1, same 1 gives `"◀3 ▶1 =1"`;
    - left only gives `"◀3"`, right only `"▶2"`, and duplicates only `"=3"`;
    - `nobase` gives `"no common history"`, and `error` gives `"git error"`.
  - `it("mirrors a result on swap")`.
  - `it("names the repositories missing a branch")`: `missingText({ left: "origin/release-1.4", right: "origin/main" }, 12)` is `"origin/release-1.4 or origin/main is missing in 12 repositories"`, and `1` gives `"… is missing in 1 repository"`.
  - `it("heads each column")`:
    - files: `"3 files · since the split"` (and `"1 file · since the split"`);
    - left commits: `"2 commits · the merge brings these in"`;
    - right commits: `"2 commits · missing from origin/release-1.4"`.
  - `it("says when a side has nothing")`: left gives `"Nothing here: origin/main already has every commit of origin/release-1.4."`, and right the mirror.
  - `it("explains the On both strip")`: `bothStripText(1)` is `"= 1 on both — the same change committed on each side (cherry-picked): the merge does not repeat it, but history lists it twice"`.
  - `it("shows the right message for each state")`:
    - the same name gives ``"Pick two different branches. Both sides are `main`."``;
    - an empty side gives `"Pick the branch to merge from (◀) and the branch it goes into (▶)."`;
    - nothing differing gives `"origin/release-1.4 and origin/main are the same in every repository."`;
    - zero ticked gives `"No repositories are ticked in the Repo List."`;
    - a pair with rows that differ gives `null`;
    - `rows === null` (reading) gives `` `Reading ${ticked} repositories…` ``.
  - `it("offers more commits whenever git returned more than the page")`: Polylog's two `sidePage` cases.
  - `it("labels a compare diff")`: `compareBanner("left", { left: "origin/release-1.4", right: "origin/main" })` gives `{ tone: "info", text: "merge base ↔ release-1.4" }`.
  - `it("a superseded compare answer is ignored")`, as in Task 5.
  - `blame.test.ts`: `it("blames a compare diff at the base and the tip")`.
- [ ] **Step 2: Run `npm --prefix ui test`.** Expected: FAIL.
- [ ] **Step 3: Implement** the items above.
- [ ] **Step 4: Run tests and check.** Expected: PASS, 0 errors.
- [ ] **Step 5: Browser preview.** Against the mock, show Compare with differing, identical, missing, no-common-history and error rows. Check Files and Commits, swap, the picker, and the both tag. Screenshot narrow and wide, dark and light. Stop the server by PID.
- [ ] **Step 6: Run everything:** `cargo test --workspace`, `npm --prefix ui test`, `npm --prefix ui run check` and `scripts/decide-e2e.sh`. Expected: all green.
- [ ] **Step 7: Commit.**
```bash
git add ui/src/gitlog/ComparePane.svelte ui/src/gitlog/CompareColumns.svelte ui/src/gitlog/compare.ts ui/src/gitlog/compare.test.ts ui/src/gitlog/blame.ts ui/src/gitlog/blame.test.ts ui/src/gitlog/GitLogView.svelte ui/src/lib/api.ts ui/src/lib/mock.ts
git commit -m "feat(ui): Compare mode in the Git log view"
```

---

## Decisions the plan makes (the spec left them open)

1. **Six combined tasks, not four or five.** The read path is two reviewable units: a pure model with a fake git (Task 3), and processes plus cancellation (Task 4). A reviewer can approve one and reject the other.
2. **Cancellation channels** (`<window>#combined`, `#blame`, `#compare`) on the one `Runner`. A combined selection must kill its own reads without killing the log's paging (`combined_channel_leaves_the_log_alone`).
3. **`exclude_for` uses `<base>^@`** instead of parent ids from the UI. The backend never trusts the webview's parents, and a root commit needs no special case.
4. **Positional NUL parsing** for every new format (range log, history, compare side log, `diff -z`). A subject or path containing `\x1e` or `\x1f` cannot break a record, as phase 1 found for the log.
5. **File History pages like Polylog's shipped code:** one `--follow` walk for the names, then `--skip` paging over those names without `--follow`. The branch filter is kept; search, authors, Me, the path filter and dates are set aside. Multi-select is off in File History, as in Polylog's non-goals.
6. **A binary or too-large fallback still names the unselected commits it includes**, unlike Polylog's title. This follows the anti-goal.
7. **Text that is not UTF-8 counts as binary** for the merge, as `file_view_of` already decides.
8. **The blame toggle sits on the diff header** (`gitLog.blame`, set-one-key) rather than in the Settings screen.
9. **Compare answers in one response per call, not row by row as each repo answers.** The summary reads "Reading N repositories…" until then. There is no ref-change watch (nothing runs in the background); ↻ re-reads.
10. **Compare has no repo right-click menu.** Polylog's carries Pull, which is phase 3.
11. **Compare repo rows read `◀n ▶n =n`**, as the desktop spec says, rather than Polylog's shipped `3 ◀ · 1 ▶ · =1`.
12. **Combined rows show no `+/−` counts:** the range log has none, and the net change is not the sum of the commits' counts.
