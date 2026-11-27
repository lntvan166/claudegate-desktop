// Compare Branches' pure helpers: the branch picker's groups, search highlight and titles.
// A branch is keyed on (name, remote): a local `origin/x` and the remote-tracking `origin/x` are two
// refs, and the compare resolves exactly the one that was picked.

import type { ChangedFile } from "./changes";
import { queryGate, isStale, type QueryGate } from "./log";
import type { BranchKey, ComparePair } from "./repos";

export type Pair = ComparePair;
/** gitlog_compare.rs `BranchNameView`: how many of the asked repositories have this branch. */
export type BranchName = { name: string; count: number; remote: boolean };
export type PickerItem =
  | { kind: "pair"; pair: Pair }
  | { kind: "name"; name: string; remote: boolean; count: number; favorite: boolean };
export type PickerGroup = { title: string; items: PickerItem[] };

export const RECENT_MAX = 5;

export const sameKey = (a: BranchKey, b: BranchKey) => a.name === b.name && a.remote === b.remote;
export const samePair = (a: Pair, b: Pair) => a.left === b.left && a.right === b.right && a.leftRemote === b.leftRemote && a.rightRemote === b.rightRemote;

/** The pair first, its earlier copy dropped, at most `cap` kept (the backend's rule; the mock uses it too). */
export function pushRecent(recent: readonly Pair[], p: Pair, cap = RECENT_MAX): Pair[] {
  return [p, ...recent.filter((x) => !samePair(x, p))].slice(0, cap);
}

/**
 * Recent pairs (not while searching), Favorites, Local, Remote. A branch may be in Favorites and in
 * its own group. Matching is a case-insensitive substring; items sort by count, then name.
 */
export function pickerGroups(names: readonly BranchName[], favorites: readonly BranchKey[], recent: readonly Pair[], query: string): PickerGroup[] {
  const q = query.trim().toLowerCase();
  const sorted = names
    .filter((n) => q === "" || n.name.toLowerCase().includes(q))
    .sort((a, b) => b.count - a.count || (a.name < b.name ? -1 : a.name > b.name ? 1 : 0) || Number(a.remote) - Number(b.remote));
  const item = (n: BranchName): PickerItem => ({ kind: "name", name: n.name, remote: n.remote, count: n.count, favorite: favorites.some((f) => sameKey(f, n)) });
  const groups: PickerGroup[] = [];
  if (q === "" && recent.length > 0) groups.push({ title: "Recent pairs", items: recent.map((pair) => ({ kind: "pair", pair })) });
  const add = (title: string, list: BranchName[]) => {
    if (list.length > 0) groups.push({ title, items: list.map(item) });
  };
  add("Favorites", sorted.filter((n) => favorites.some((f) => sameKey(f, n))));
  add("Local", sorted.filter((n) => !n.remote));
  add("Remote", sorted.filter((n) => n.remote));
  return groups;
}

/** The first match of `query` in `name`, split around it (the match keeps the name's own case). */
export function highlight(name: string, query: string): { pre: string; hit: string; post: string } {
  const q = query.trim();
  const i = q === "" ? -1 : name.toLowerCase().indexOf(q.toLowerCase());
  if (i < 0) return { pre: name, hit: "", post: "" };
  return { pre: name.slice(0, i), hit: name.slice(i, i + q.length), post: name.slice(i + q.length) };
}

export function inReposText(n: number): string {
  return `in ${n} ${n === 1 ? "repository" : "repositories"}`;
}

export function noMatchText(q: string): string {
  return `No branch named "${q}" in any repository.`;
}

/**
 * The picker's active row: the row keyed `activeKey` (`<group>\u0000<item>`), else the same item
 * in another group (its Favorites copy went), else the position last chosen.
 */
export function activeIndex(keys: string[], activeKey: string | null, lastIndex: number): number {
  if (activeKey !== null) {
    const at = keys.indexOf(activeKey);
    if (at >= 0) return at;
    const item = activeKey.slice(activeKey.indexOf("\u0000"));
    const same = keys.findIndex((k) => k.endsWith(item));
    if (same >= 0) return same;
  }
  return Math.max(0, Math.min(lastIndex, keys.length - 1));
}

// ---- Compare mode (gitlog_compare.rs) ----------------------------------------------------------

/** gitlog_compare.rs `RepoCompareView`: one repository's answer, tagged by `kind`. */
export type RepoCompare =
  | { kind: "missing" }
  | { kind: "identical" }
  | { kind: "nobase" }
  | { kind: "error"; reason: string }
  | { kind: "differs"; leftSha: string; rightSha: string; base: string; left: number; right: number; sameLeft: number; sameRight: number };
/** gitlog_compare.rs `RepoCompareRow`. */
export type CompareRow = { repo: string } & RepoCompare;
/** gitlog_compare.rs `CompareView`: every asked repository, in request order. */
export type CompareView = { queryId: number; rows: CompareRow[] };
/** gitlog_compare.rs `CompareFiles`: each side's changes since the split, and the paths both changed. */
export type CompareFiles = { left: ChangedFile[]; right: ChangedFile[]; both: string[] };
/** gitlog_compare.rs `SideCommitView`. `=`: the other side has the same change. */
export type SideCommit = { mark: "+" | "="; sha: string; parents: string[]; time: number; author: string; subject: string };
export type Duplicate = { subject: string; left?: string; right?: string };
export type CompareSide = "left" | "right";
export type CompareMode = "files" | "commits";

/** A side's commits per page (the backend's `COMMIT_PAGE`). */
export const COMMIT_PAGE = 500;
export const BOTH_TOOLTIP = "Changed on both sides since the split: look at it before merging";

const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

/** Rows the repo list shows: the ones that differ, and the ones that say why they can't be compared. */
export const listed = (c: RepoCompare) => c.kind === "differs" || c.kind === "nobase" || c.kind === "error";

/** The list's summary. No commit numbers: they mislead when the same change travels by cherry-pick. */
export function summaryLabel(rows: readonly RepoCompare[], ticked: number): string {
  let differ = 0;
  let identical = 0;
  let missing = 0;
  for (const c of rows) {
    if (c.kind === "identical") identical++;
    else if (c.kind === "missing") missing++;
    else differ++;
  }
  const parts = [`${differ} ${differ === 1 ? "repository differs" : "repositories differ"}`];
  if (identical > 0) parts.push(`${identical} identical`);
  if (missing > 0) parts.push(`${missing} missing a branch`);
  parts.push(`in ${plural(ticked, "repository", "repositories")}`);
  return parts.join(" · ");
}

/** A repo row's counts, `◀n ▶n =n` with zeros left out; a row that can't be compared says why. */
export function repoCounts(c: RepoCompare): string {
  if (c.kind === "nobase") return "no common history";
  if (c.kind === "error") return "git error";
  if (c.kind !== "differs") return "";
  return [c.left > 0 ? `◀${c.left}` : "", c.right > 0 ? `▶${c.right}` : "", c.sameLeft > 0 ? `=${c.sameLeft}` : ""].filter(Boolean).join(" ");
}

/** Swap mirrors the result it has and does not read again. */
export function mirror(c: RepoCompare): RepoCompare {
  if (c.kind !== "differs") return c;
  return { kind: "differs", leftSha: c.rightSha, rightSha: c.leftSha, base: c.base, left: c.right, right: c.left, sameLeft: c.sameRight, sameRight: c.sameLeft };
}

export const swapPair = (p: Pair): Pair => ({ left: p.right, leftRemote: p.rightRemote, right: p.left, rightRemote: p.leftRemote });

export function missingText(p: Pair, n: number): string {
  return `${p.left} or ${p.right} is missing in ${plural(n, "repository", "repositories")}`;
}

/** The line under a column's title. */
export function columnHeader(side: CompareSide, mode: CompareMode, n: number, p: Pair): string {
  if (mode === "files") return `${plural(n, "file")} · since the split`;
  return `${plural(n, "commit")} · ${side === "left" ? "the merge brings these in" : `missing from ${p.left}`}`;
}

export function emptyColumnText(side: CompareSide, p: Pair): string {
  const [has, of] = side === "left" ? [p.right, p.left] : [p.left, p.right];
  return `Nothing here: ${has} already has every commit of ${of}.`;
}

export function bothStripText(n: number): string {
  return `= ${n} on both — the same change committed on each side (cherry-picked): the merge does not repeat it, but history lists it twice`;
}

/** What the body says instead of the list; null when the list has something to show. `rows` null: being read. */
export function compareMessage(p: Pair | null, ticked: number, rows: readonly RepoCompare[] | null): string | null {
  if (!p || !p.left || !p.right) return "Pick the branch to merge from (◀) and the branch it goes into (▶).";
  if (sameKey({ name: p.left, remote: p.leftRemote }, { name: p.right, remote: p.rightRemote })) return `Pick two different branches. Both sides are \`${p.left}\`.`;
  if (ticked === 0) return "No repositories are ticked in the Repo List.";
  if (rows === null) return `Reading ${plural(ticked, "repository", "repositories")}…`;
  if (rows.some(listed)) return null;
  if (rows.length > 0 && rows.every((r) => r.kind === "missing")) return `${missingText(p, rows.length)}.`;
  return `${p.left} and ${p.right} are the same in every repository.`;
}

/** A side's page of commits (git returned up to limit + 1, duplicates included): its own rows, and whether more exist. */
export function sidePage<T extends { mark: "+" | "=" }>(commits: T[], limit: number): { rows: T[]; more: boolean } {
  return { rows: commits.slice(0, limit).filter((c) => c.mark === "+"), more: commits.length > limit };
}

/** Duplicates for display: a left and a right one with the same subject share a row. Counts come from git. */
export function pairDuplicates(left: readonly SideCommit[], right: readonly SideCommit[]): Duplicate[] {
  const rights = [...right];
  const out: Duplicate[] = [];
  for (const l of left) {
    const i = rights.findIndex((r) => r.subject === l.subject);
    if (i >= 0) {
      out.push({ subject: l.subject, left: l.sha, right: rights[i].sha });
      rights.splice(i, 1);
    } else out.push({ subject: l.subject, left: l.sha });
  }
  for (const r of rights) out.push({ subject: r.subject, right: r.sha });
  return out;
}

/**
 * The note on a compare diff: the merge base against that side's tip, by the branch's full name.
 * A remote side keeps its `origin/`: local `main` and `origin/main` are different tips.
 */
export function compareBanner(side: CompareSide, p: Pair): { tone: "info"; text: string } {
  return { tone: "info", text: `merge base ↔ ${side === "left" ? p.left : p.right}` };
}

/** `pair` is the pair `view` answers (it trails the toolbar while a newer read is in flight). */
export type CompareState = { loading: boolean; view: CompareView | null; pair: Pair | null; error: string | null };
export const EMPTY_COMPARE: CompareState = { loading: false, view: null, pair: null, error: null };
export type CompareFetcher = (queryId: number, repos: string[], pair: Pair) => Promise<CompareView>;

/**
 * One compare at a time, gated like `CombinedLoad`: a new `start` supersedes the one before it, and
 * an answer is kept only when it is for the query asked last and the backend echoes that id.
 */
export class CompareLoad {
  private current = 0;
  state: CompareState = EMPTY_COMPARE;

  constructor(
    private readonly fetch: CompareFetcher,
    private readonly emit: (s: CompareState) => void,
    private readonly gate: QueryGate = queryGate,
  ) {}

  /** The query the shown rows answer (what Files and Commits reads are tied to). */
  get id(): number {
    return this.current;
  }

  private set(s: CompareState) {
    this.state = s;
    this.emit(s);
  }

  async start(repos: string[], pair: Pair): Promise<void> {
    const id = this.gate.begin();
    this.current = id;
    // The old rows stay (dimmed, with the pair they answer) until the new ones land.
    this.set({ ...this.state, loading: true, error: null });
    try {
      const view = await this.fetch(id, repos, pair);
      if (id !== this.current || view.queryId !== id) return;
      this.set({ loading: false, view, pair: { ...pair }, error: null });
    } catch (e) {
      if (id !== this.current) return;
      this.set({ loading: false, view: null, pair: null, error: isStale(e) ? "The comparison changed while it was being read" : String(e) });
    }
  }

  /** Swap: the rows shown, each side's numbers exchanged. Nothing is read. */
  mirror() {
    const v = this.state.view;
    if (!v) return;
    this.set({ ...this.state, view: { queryId: v.queryId, rows: v.rows.map((r) => ({ repo: r.repo, ...mirror(r) })) }, pair: this.state.pair && swapPair(this.state.pair) });
  }

  /** Nothing to compare: whatever is in flight is ignored. */
  clear() {
    this.current = 0;
    this.set(EMPTY_COMPARE);
  }
}

/** A file opened from Compare: one side's change since the split, or one commit's change. */
export type CompareOpen =
  | { kind: "side"; repo: string; side: CompareSide; file: ChangedFile; base: string; tip: string; pair: Pair }
  | { kind: "commit"; repo: string; commit: SideCommit; file: ChangedFile };

export function compareOpenKey(o: CompareOpen): string {
  return o.kind === "side" ? `s\u0000${o.repo}\u0000${o.side}\u0000${o.tip}\u0000${o.file.path}` : `c\u0000${o.repo}\u0000${o.commit.sha}\u0000${o.file.path}`;
}

/** A compare row and its pair as one key: the same key is the same read. */
export function compareKey(p: Pair | null, repos: readonly string[]): string {
  return JSON.stringify([p?.left ?? "", p?.leftRemote ?? false, p?.right ?? "", p?.rightRemote ?? false, repos]);
}

/** Both names given, and two different refs. */
export function comparable(p: Pair | null): p is Pair {
  return !!p && !!p.left && !!p.right && !sameKey({ name: p.left, remote: p.leftRemote }, { name: p.right, remote: p.rightRemote });
}

/** A repo row's counts in words, for a screen reader: "3 only on X, 1 only on Y, 1 on both". */
export function repoCountsLabel(c: RepoCompare, p: Pair): string {
  if (c.kind !== "differs") return repoCounts(c);
  const parts = [c.left > 0 ? `${c.left} only on ${p.left}` : "", c.right > 0 ? `${c.right} only on ${p.right}` : "", c.sameLeft > 0 ? `${c.sameLeft} on both` : ""].filter(Boolean);
  return parts.length ? parts.join(", ") : "no commits on either side";
}

export type ReadState<T> = { data: T | null; loading: boolean; error: string | null };
export const EMPTY_READ: ReadState<never> = { data: null, loading: false, error: null };

/**
 * One column read (Files, or one side's commits) under a compare query. A newer `run`, `hit` or
 * `reset` supersedes the one in flight. A STALE answer to the current run ends it with no data and
 * no error, and remembers the query it was cut short under: `neededFor` is then true again for any
 * other query (the next compare, even with the same tips), never for that one (no re-read loop).
 */
export class GatedRead<T> {
  private seq = 0;
  private staleQuery: number | null = null;
  state: ReadState<T> = EMPTY_READ;

  constructor(private readonly emit: (s: ReadState<T>) => void) {}

  private set(s: ReadState<T>) {
    this.state = s;
    this.emit(s);
  }

  /** Nothing here, nothing in flight, nothing failed, and `query` did not already cut it short: a read is due. */
  neededFor(query: number): boolean {
    return this.state.data === null && !this.state.loading && this.state.error === null && this.staleQuery !== query;
  }

  reset() {
    this.seq++;
    this.staleQuery = null;
    this.set(EMPTY_READ);
  }

  /** An answer already in hand (a cache hit). */
  hit(v: T) {
    this.seq++;
    this.set({ data: v, loading: false, error: null });
  }

  /** Reads under `query`; what is shown stays (dimmed) until the answer lands. */
  async run(query: number, fetch: () => Promise<T>): Promise<void> {
    const seq = ++this.seq;
    this.staleQuery = null;
    this.set({ data: this.state.data, loading: true, error: null });
    try {
      const v = await fetch();
      if (seq !== this.seq) return;
      this.set({ data: v, loading: false, error: null });
    } catch (e) {
      if (seq !== this.seq) return;
      if (isStale(e)) this.staleQuery = query;
      this.set({ data: this.state.data, loading: false, error: isStale(e) ? null : String(e) });
    }
  }
}
