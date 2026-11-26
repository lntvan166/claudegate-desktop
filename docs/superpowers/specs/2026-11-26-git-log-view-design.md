# Git log view: Polylog's merged log inside ClaudeGate Desktop (phase 1)

Status: approved direction. This document is the design for review.

## Why

The maintainer used Polylog (`~/Documents/src/personal/polylog`, a VS Code extension, v1.4.0) to read one merged git log across many repositories. They no longer work in VS Code, and opening VS Code just for that is friction. The goal is Polylog's reading workflow without VS Code.

They chose to build it into ClaudeGate Desktop as a separate view rather than as a separate app:
- one window, one install, one updater;
- it reuses the diff, icons, tree and theme;
- the workspace's projects already name the repositories, for example an umbrella root with ~70 nested repos.

## Phases (agreed)

1. **Merged log (this document).** Read-only.
2. Deeper: Combined Changes, File History, blame, Compare Branches. Read-only.
3. Actions: Fetch/Pull All, Uncommitted (stage, unstage, discard, commit). Writes to repos.

Phases 2 and 3 get their own designs later. Phase 1 must not paint them into a corner: keep its modules and data shapes extensible.

## Goals (phase 1)

- **The view.** A "Git log" view next to the existing review, switched from the header: `[ Review │ Git log ]`.
- **The merged log.** One list of commits from every selected repository, newest first by committer date with SHA as the tie-break. Each row shows a colored repo chip, short SHA, subject, relative date and author.
- **Filters, all pushed down into `git log`.** Each is a flag on every repo's `git log`.

  | Filter | How it reaches git |
  |---|---|
  | Message search | literal, case-insensitive: `--grep=<term> -i -F` |
  | Authors | chips, any-of: multiple `--author`; **Me** = each repo's `user.email` |
  | Path | file, folder or glob, relative to each repo root: `-- :(glob)<pattern>` |
  | Branch | applied in every repo that has it (`rev-parse --verify --quiet <b>^{commit}`); the rest fall back to their current branch; a footer shows "found in N · current branch in M" |
  | Date range | last 24 h (default), 7 d, 30 d, all, custom: `--since`/`--until`, with "now" fixed at the first page |

- **Repositories pane.**
  - Tick which repos to show, with fuzzy search and multi-select.
  - **Profiles** are named sets of ticked repos.
  - **↓/↑ counts** come from the last fetch only: `git rev-list --left-right --count @{u}...HEAD`. There is no fetch in phase 1.
  - **Right-click a repo:** show only this, hide, show all, copy path.
- **Changes and diff.** Selecting a commit shows a Changes tree:
  - status badges A/M/D/R, add and delete counts, Material icons;
  - a title such as `acme-api · 3f9a2c1 · 2 files`.

  Clicking a file opens it in the existing Monaco diff: before = the parent's content (empty for a root commit or an added file), after = the commit's content (empty for a deleted file). Renames diff the old path against the new one.
- **Right-click a commit:**
  - Copy Commit ID.
  - Copy Message (the whole message).
  - Open on Remote, built from `origin` for GitHub, GitLab, Bitbucket and Azure DevOps. Credentials in the URL are stripped and never opened.
- **Keyboard first.**
  - ↑/↓, Home/End and PageUp/PageDown move the selection, and the Changes tree follows.
  - Enter opens the first changed file.
  - `/` focuses search.
  - Esc clears search, then returns to the list.
  - Switching between the two views has a shortcut that does not clash with the review's keys.
- **Command line.** `claudegate log [PATH]` opens the Git log view with the repositories under PATH ticked. It goes through the existing single-instance hand-off.

## Non-goals (phase 1)

- A commit graph.
- Combined Changes, File History, blame, Compare (phase 2).
- Fetch, pull, stage, discard, commit (phase 3).
- Editing files from the diff.
- Any write to a repository.

## Anti-goals (would be a failure even if it works)

- **An index, cache or database of commits.** Polylog measured 25 ms for `git log -n 50` across 68 repos (16-way parallel). Spawn on demand and merge in memory, as Polylog does.
- **Filtering in Rust or JS what `git log` can filter.** Every filter goes into git's argv, and the app only merges already-filtered streams.
- **Writing to a repository, or to ClaudeGate's shared files.** Phase 1 writes only the app's own `config.json`: profiles, ticked repos, last filters.
- **Slowing the review view.** Git work runs off the UI thread, and the log never loads until the Git log view is first opened.
- **One broken repo breaking the view.** Failures stay with their repo: a dismissible row names the repo and the reason. Typical causes are a shallow clone, permissions, an interrupted rebase, a path that is no longer a repo, or a repo with no commits.

## Constraints

- **Stack:** Tauri 2 + Rust (`claudegate-core` for the pure logic, `src-tauri` for commands and process spawning) and Svelte 5. Reuse the app's diff, icons, file tree, theme tokens (Dracula/Alucard), settings screen and context menu.
- **Code layout, so phases 2 and 3 or a later split stay cheap:**
  - `core/src/gitlog/`: pure parsing, merge, filter→argv, discovery, remote URLs, with unit tests.
  - `src-tauri/src/gitlog.rs`: commands, the spawn pool, cancellation.
  - `ui/src/gitlog/`: view components and their pure helpers.
  - Shared UI pieces are imported, not copied.
- **Git invocation.**
  - Run `git` as a child process with `--no-pager -c color.ui=never -c core.quotepath=off`.
  - Use NUL- and RS-delimited formats:
    - log: `--format=%H%x00%ct%x00%an%x00%ae%x00%s%x00%P%x1e`
    - changes: `show --numstat -M --format= <sha>` together with `--name-status`
  - Never prompt for credentials: `GIT_TERMINAL_PROMPT=0`.
  - The git binary comes from PATH, with the same GUI-PATH fallback directories the app already uses for python3 and editors.
- **Paging** (see Decisions: `--skip` + horizon supersedes this).
  - Windowed fan-out: each repo returns its newest `pageSize` (default 200) matching commits.
  - Merge them, then "Load more" re-queries with `--until=<committer date of the oldest shown row>`.
  - A concurrency pool of 16.
- **Responsiveness.**
  - Filter input is debounced 250 ms.
  - A new query kills the previous query's child processes.
  - The list is virtualised: thousands of rows stay smooth.
- **Performance target, on the maintainer's umbrella root (~70 repos):** first page visible within 300 ms on a warm cache, and the selected commit's Changes within 100 ms.
- **Repository discovery comes from the workspace (the rail's projects).**
  - For each workspace root, the root itself if it is a repo, plus git repos found by a bounded walk.
  - The walk is `gitlog.scanDepth`, default 2. It stops at a `.git`, and never enters `node_modules`, `vendor` or `target`.
  - The registered nested roots and worktrees the review already knows are included.
  - `gitlog.excludeRepos` removes repos.
  - Repo identity is the resolved path, with a display name and a stable color per repo, derived from the path.
- **Neutral names** in all code, tests, fixtures and docs (`acme-api`, `acme-web`, `svc-lib`): the repo is public. Polylog's §7 "neutral fixtures" rule applies.
- No version bump, release or push without the maintainer saying so.

## Approaches considered

- **Separate app, "Polylog Desktop".** A clean product, but it duplicates the Tauri shell, packaging, updater, signing and CI, and adds another window and dock icon: the friction the maintainer wants gone. Rejected.
- **Inside ClaudeGate as a separate view (chosen).** Reuses everything listed above, gets repos from the workspace, and allows links between the two views later (a pending file's git history, a commit's Claude edits). The code is kept in its own modules so a split remains possible.
- **Wrap Polylog's TypeScript in the webview.** The webview cannot spawn processes, and Polylog is built on the VS Code API, so its core would need rewriting either way. Port the logic to Rust instead: discovery, `logArgs`, merge, parse, remote URLs. Polylog's pure modules (`filterModel`, `mergeStream`, `gitLog` parsing, remote URL building) and their tests are the reference behaviour.

## Decided UI shape

```
┌ ClaudeGate ─────────────── [ Review │ Git log ] ──────── chips ⚙ ┐
│ Repos (profile ▾)│ 🔍 search  👤 authors  📁 path  ⎇ branch  24h ▾│
│ ☑ acme-api   ↓2  │ ● acme-api  3f9a2c1  fix: refund…   2h  An   │
│ ☑ acme-web       │ ● acme-web  b81e0d4  feat: cart…    3h  An   │
│ ☐ svc-lib        │ ● acme-api  9c0d11a  chore: bump…   5h  Bot  │
│                  ├────────────────────────┬─────────────────────┤
│                  │ Changes  3f9a2c1·2 files│  Monaco diff        │
│                  │  M api/refund.go        │                     │
└──────────────────┴────────────────────────┴─────────────────────┘
```

- The three columns resize with the existing splitter and remember their widths.
- At narrow widths the Repos pane collapses, as the projects rail does today.

## Decisions after the builder check (Claude's calls; the maintainer may override)

1. **Paging follows Polylog's shipped code, not its design doc.** This supersedes the "`--until` cursor" wording above.
   - Each repo keeps a `fetched` count, and the next page uses `--skip`.
   - Merged rows are shown only down to the **horizon**: the newest "oldest fetched commit" among repos that still have history. Rows below it are held for the next page, never shown out of order and never dropped.
   - `mergeStream.ts` / `logQuery.ts` (`takeReady`) are the reference, and their tests are ported.
2. **Worktrees: one entry per repository.**
   - Identity is the repo's **git common dir**.
   - Its log is one `git log` over the HEADs of the main checkout and every worktree (`git log <head1> <head2> …`), so git deduplicates shared history itself.
   - Each row shows which checkout or branch it is on when that isn't the main checkout's.
   - Profiles and ticks are keyed by the common dir.
   - The branch filter applies per repository.
3. **Merge commits** use `--diff-merges=first-parent`, as Polylog does. The Changes tree and the diff's before side are against `parents[0]`.
4. **`claudegate log [PATH]`.**
   - The `claudegate` wrapper and `target_from_args` learn a `log` subcommand.
   - `claudegate log` with no PATH means the current directory. A folder literally named `log` opens with `claudegate ./log`.
   - The repos under PATH are ticked for **this launch only**. Nothing is written to `config.json` until the user changes ticks or saves a profile.
   - A PATH whose repos are not in the workspace gets the same "isn't in your workspace — [Add to workspace]" notice the review launch uses. Discovery stays workspace-only.

## Left to the builder

- Exact component split, Rust struct names, and the Tauri command names (prefix `gitlog_`).
- Exact keys for the view switch, provided they don't clash.
- The color palette for repo chips, derived from the theme's accent set.
- Empty, loading and error state copy, following the app's tone.
- How the last filters, ticked repos and profiles are laid out in `config.json` (under one `gitLog` key).
