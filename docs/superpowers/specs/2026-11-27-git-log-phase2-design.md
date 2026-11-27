# Git log view, phase 2: Combined Changes, File History, Blame, Compare Branches

Status: proposed. It follows phase 1 (`2026-11-26-git-log-view-design.md`) and keeps that document's constraints and anti-goals.

## Why

Phase 1 ports Polylog's merged log. While trying it, the maintainer reached for Polylog's **multi-commit review**: select several commits and review each file once. Phase 1 deferred that feature to phase 2.

Phase 2 ports the rest of Polylog's read-only reading tools, so ClaudeGate Desktop can fully replace Polylog for reading:
- Combined Changes;
- File History;
- Blame in diffs;
- Compare Branches.

## The reference behaviour is Polylog's own specs

Polylog's specs are detailed, tested and approved by the same maintainer. They are the behaviour reference, rule for rule, except where this document adapts them to the desktop app.

| Feature | Polylog spec | Polylog code to port |
|---|---|---|
| Combined Changes | `docs/superpowers/specs/2026-10-20-combined-changes-design.md` | `combinedModel.ts`, `combinedRead.ts`, `lineMerge.ts` |
| File History | `2026-09-23-polylog-design.md` and README, "File History" | `gitLog.ts` (history format), `changesModel.ts` |
| Blame in diffs | README, "Blame in diffs" | `blameModel.ts`, `blameView.ts` |
| Compare Branches | `docs/superpowers/specs/2026-10-01-compare-branches-design.md` | `compareModel.ts`, `compareRepos.ts` |

Polylog's pure modules and their tests are ported to Rust (`core/src/gitlog/`). The UI is Svelte (`ui/src/gitlog/`).

## Desktop adaptations (decided)

1. **Combined Changes.**
   - **Selection.** Ctrl-click, Shift-click and Shift+↑/↓ select several commits in the commit list, in any repos. The limit is 100 commits. A plain click is unchanged.
   - **Changes tree.** With 2 or more commits selected, the tree groups by repo. Each file appears once, with the short shas that touched it and its net status. A ⚠ marks a file that falls back.
   - **Diff.** Polylog's §4 semantics: the left side is the newest file with the selected commits' changes taken back out, using a three-way line merge. The fallback is the plain range diff, labelled with the unselected commits it includes.
   - **Merge in Rust.** The line merge (diff3) is a pure Rust module in core. It never uses `git merge-file` or `merge-tree`, because those write objects into `.git`. Inputs are read with `git show <sha>:<path>`. Nothing is cached. The round-trip guard and the 1 MB / 20,000-line / 2,000-edited-line limits all carry over.
   - **Labels.** Polylog shows the message in the diff title. The desktop shows it in the diff header bar instead, which has room for a real notice.
2. **File History.**
   - Right-click a file in Changes, or press a key on it, to choose File History. The commit list then shows the commits that touched that file in its repo, following renames with `git log --follow`.
   - While File History is open, the search, author and date filters are set aside. A banner reads "History of `path` · ✕". Closing it restores the filters.
   - The diff follows the selection, as in Polylog.
3. **Blame.** In the Git log diff, putting the cursor on a line shows who last changed that line, as of the commit shown. It appears as an end-of-line Monaco decoration with a hover for details.
   - It runs one `git blame --porcelain -L n,n <rev> -- <path>`, debounced.
   - There is a setting to turn it off.
4. **Compare Branches.**
   - **Where it lives.** It is a third mode of the Git log view, `[ Log │ Compare ]` inside the view. It is not a new top-level view.
   - **Controls.** Two branch boxes (◀ left, ▶ right) with Polylog's branch picker: recent pairs, favourites, local, remote.
   - **What it shows.** The repos that differ, with `◀n ▶n =n`, then the changed files per side (Files | Commits), then the diff.
   - **Storage.** Favourites and recent pairs are kept in the app's own `config.json` under `gitLog`, with set-one-key and add-only writers, as in phase 1.
   - **Out of scope.** Pull from the menu is not included; that is phase 3.

## Anti-goals (on top of phase 1's)

- Writing anything into a repository, including git objects. This is why the merge is our own code and not `git merge-file`.
- A diff that passes off an unselected commit's change as part of the selection without saying so.
- Changing how a single click and a single commit behave.

## Order and size

Each feature can be reviewed on its own.

1. Combined Changes. The maintainer asked for this first. It is the largest: the selection model, a Rust diff3 with the round-trip guard, the read path, the tree and the diff header.
2. File History.
3. Blame.
4. Compare Branches.

## Left to the builder

- Rust struct and command names. Commands use the prefix `gitlog_`.
- Exact keys, provided they don't clash with phase 1's.
- Copy for the empty, loading and error states, in the app's tone.
