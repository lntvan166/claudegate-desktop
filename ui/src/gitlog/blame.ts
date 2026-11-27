// Blame on the cursor's line in a Git log diff: the text and hover it draws, and which revision
// each side of the diff is blamed at. (Polylog's blameModel/blameView, one line at a time.)

import { beforeFrom, type ChangedFile } from "./changes";
import type { CombinedFile } from "./combined";
import { commitAge, commitDate, shortSha, type RowView } from "./log";

/** How long the cursor rests on a line before git is asked. */
export const BLAME_DEBOUNCE_MS = 150;

/** gitlog.rs `BlameLineView`. */
export type BlameLine = { sha: string; author: string; email: string; time: number; summary: string };

/** A file at a revision: what `git blame` is run on. */
export type BlameRef = { repo: string; rev: string; path: string };

/** The ref to blame for each side of the diff; null when that side has nothing to blame. */
export type BlameSides = { original: BlameRef | null; modified: BlameRef | null };

/** `subject, author (3d ago)`, the subject cut to 50 characters. */
export function blameText(b: BlameLine, now: number): string {
  // By code point: a cut inside a surrogate pair would leave half an emoji.
  const chars = Array.from(b.summary);
  const subject = chars.length > 50 ? `${chars.slice(0, 49).join("")}…` : b.summary;
  return `${subject}, ${b.author} (${commitAge(b.time, now)})`;
}

const MD_SPECIAL = /[\\`*_{}[\]()#+\-.!|<>:~]/g;

/** Backslash-escapes every character Markdown reads as syntax, so a name or subject stays text. */
export function escapeMarkdown(s: string): string {
  return s.replace(MD_SPECIAL, (c) => `\\${c}`);
}

/** The hover: author and email, age and date, the subject, and the short id. */
export function blameHover(b: BlameLine, now: number): string {
  const e = escapeMarkdown;
  return `${e(b.author)} <${e(b.email)}>, ${e(commitAge(b.time, now))} (${e(commitDate(b.time))})\n\n${e(b.summary)}\n\n\`${e(shortSha(b.sha))}\``;
}

/** One commit's file: the parent's version against the commit's. */
export function commitBlameSides(row: RowView, f: ChangedFile): BlameSides {
  const parent = row.parents[0];
  return {
    original: f.status === "added" || !parent ? null : { repo: row.repo, rev: parent, path: beforeFrom(f) ?? f.path },
    modified: f.status === "deleted" ? null : { repo: row.repo, rev: row.sha, path: f.path },
  };
}

/** A combined file: its left side is blamed only when it is a real revision, not merged text. */
export function combinedBlameSides(repo: string, f: CombinedFile, leftMerged: boolean): BlameSides {
  return {
    original: leftMerged || f.beforeRef === null ? null : { repo, rev: f.beforeRef, path: f.beforePath },
    modified: f.status === "deleted" ? null : { repo, rev: f.last, path: f.path },
  };
}

/** A compare file: the merge base (at the path before a rename on that side) against that side's tip. */
export function compareBlameSides(repo: string, base: string, tip: string, path: string, from: string | null): BlameSides {
  return { original: { repo, rev: base, path: from ?? path }, modified: { repo, rev: tip, path } };
}

/**
 * The config after the Blame setting was saved: only `blame` is taken from what the backend
 * returns. The rest stays as shown, since it can hold in-memory state the backend never stored
 * (the ticks of a `claudegate log PATH` launch).
 */
export function withBlame<C extends { blame: boolean }>(cfg: C, saved: { blame: boolean }): C {
  return { ...cfg, blame: saved.blame };
}
