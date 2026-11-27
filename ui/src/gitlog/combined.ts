// Combined Changes: several selected commits, each file once. What gitlog_combined.rs returns,
// turned into rows, texts and the race-safe loader. Pure; CombinedChanges.svelte and
// GitLogView.svelte draw from it. A port of Polylog's combined Changes tree.
import { buildTree, type TreeNode } from "../lib/tree";
import { isStale, queryGate, shortSha, type QueryGate, type RowView } from "./log";

export type NetStatus = "added" | "modified" | "deleted" | "renamed";
export type FallbackReason = "conflict" | "binary" | "tooLarge";

/** gitlog_combined.rs `CombinedFileView`. */
export type CombinedFile = {
  path: string;
  /** Renamed within the range: its path before. */
  from: string | null;
  status: NetStatus;
  /** The selected commits that touched it, oldest first. */
  shas: string[];
  last: string;
  beforeRef: string | null;
  beforePath: string;
  /** Unselected commits the diff carries. */
  included: string[];
  mode: "fast" | "merge" | "fallback";
  reason: FallbackReason | null;
};

/** gitlog_combined.rs `CombinedRepoView`. */
export type CombinedRepo = {
  repo: string;
  commits: string[];
  files: CombinedFile[];
  noNetChange: string[];
  /** Subjects of the unselected commits in range, by sha. */
  subjects: Record<string, string>;
  /** The repository could not be read (git's first stderr line). */
  error: string | null;
};

export type CombinedView = { queryId: number; repos: CombinedRepo[] };

/** gitlog_combined.rs `CombinedLeftView`. */
export type CombinedLeft = { kind: "text"; value: string } | { kind: "fallback"; value: FallbackReason };

/** Two or more picked commits make the Changes column a combined one. */
export const isCombined = (picked: RowView[]): boolean => picked.length >= 2;

export const itemsOf = (picked: RowView[]): { repo: string; sha: string }[] => picked.map((r) => ({ repo: r.repo, sha: r.sha }));

/** `4 commits · 2 repositories` */
export function combinedTitle(commits: number, repos: number): string {
  return `${commits} ${commits === 1 ? "commit" : "commits"} · ${repos} ${repos === 1 ? "repository" : "repositories"}`;
}

/** The short ids of the commits that touched a file, as listed (oldest first). */
export const shaList = (shas: string[]): string => shas.map(shortSha).join(" · ");

/** Why a file is shown as one diff across the selection; null for a file that is taken apart. */
export function warnText(f: CombinedFile, subjects: Record<string, string>): string | null {
  if (f.mode !== "fallback") return null;
  if (f.reason === "binary") return "Binary file: shown as one diff across the selection";
  if (f.reason === "tooLarge") return "Too large to take apart: shown as one diff across the selection";
  if (f.included.length === 0) return "The selected changes can't be shown apart here";
  const list = includedList(f.included, subjects);
  return `Includes changes from ${f.included.length} unselected ${f.included.length === 1 ? "commit" : "commits"}: ${list}`;
}

export type Banner = { tone: "info" | "warn"; text: string };

/** The banner only while the diff is settled: not while it is read (a merge file may still fall back), not above an error. */
export const bannerWhenSettled = (b: Banner | null, o: { loading: boolean; error: string | null }): Banner | null =>
  o.loading || o.error ? null : b;

const commitsWord = (n: number) => (n === 1 ? "commit" : "commits");

/** `9c4e1a8 header: wider title; 1b2c3d4 fix: x` */
function includedList(included: string[], subjects: Record<string, string>): string {
  return included
    .map((s) => {
      const subject = subjects[s] ?? subjects[shortSha(s)];
      return subject ? `${shortSha(s)} ${subject}` : shortSha(s);
    })
    .join("; ");
}

/**
 * The note under the diff header: what the combined diff covers. Null for one commit on the fast
 * path. `left` is the `gitlogCombinedLeft` answer of a merge file (null otherwise): a fallback
 * there overrides the mode. A fallback names the unselected commits it carries, whatever the reason.
 */
export function combinedNotice(f: CombinedFile, subjects: Record<string, string>, left: CombinedLeft | null): Banner | null {
  const fellBack = f.mode === "fallback" || left?.kind === "fallback";
  if (!fellBack) {
    const n = f.shas.length;
    if (f.mode === "fast" && n === 1) return null;
    let text = `${n} selected ${commitsWord(n)}: ${shaList(f.shas)}`;
    if (f.mode === "merge" && f.included.length > 0) {
      text += ` · edits from ${f.included.length} unselected ${commitsWord(f.included.length)} shown as unchanged context`;
    }
    return { tone: "info", text };
  }
  const reason: FallbackReason | null = left?.kind === "fallback" ? left.value : f.reason;
  const range = `${shortSha(f.shas[0] ?? f.last)}^ ↔ ${shortSha(f.last)}`;
  const inc = f.included.length > 0 ? `includes ${f.included.length} unselected ${commitsWord(f.included.length)}: ${includedList(f.included, subjects)}` : null;
  const parts = [range];
  if (reason === "binary") parts.push("binary");
  else if (reason === "tooLarge") parts.push("too large");
  if (inc) parts.push(inc);
  else if (reason !== "binary" && reason !== "tooLarge") parts.push("the selected changes can't be shown apart");
  return { tone: "warn", text: parts.join(" · ") };
}

/** `2 files with no net change: a.ts, b.ts`; null when there are none. */
export function noNetChangeText(paths: string[]): string | null {
  if (paths.length === 0) return null;
  return `${paths.length} ${paths.length === 1 ? "file" : "files"} with no net change: ${paths.join(", ")}`;
}

/** The repositories in the Repo List's order (`repoOrder` is its discovery order); unknown ones last. */
export function orderRepos(repos: CombinedRepo[], repoOrder: string[]): CombinedRepo[] {
  const at = (id: string) => {
    const i = repoOrder.indexOf(id);
    return i < 0 ? Number.MAX_SAFE_INTEGER : i;
  };
  return repos
    .map((r, i) => ({ r, i }))
    .sort((a, b) => at(a.r.repo) - at(b.r.repo) || a.i - b.i)
    .map((x) => x.r);
}

export type CombinedItem = { rel: string; file: CombinedFile };
export type CombinedNode = TreeNode<CombinedItem>;

/** Folders first, then files, each alphabetical; a one-child folder chain is one row. */
export const combinedTree = (files: CombinedFile[]): CombinedNode[] => buildTree(files.map((file) => ({ rel: file.path, file })));

/** The files in the order the tree lists them. */
export function combinedTreeOrder(files: CombinedFile[]): CombinedFile[] {
  const out: CombinedFile[] = [];
  const walk = (ns: CombinedNode[]) => {
    for (const n of ns) {
      if (n.kind === "folder") walk(n.children ?? []);
      else if (n.item) out.push(n.item.file);
    }
  };
  walk(combinedTree(files));
  return out;
}

/** The first file, in tree order, of the first repository that has any: what Enter opens. */
export function combinedFirst(repos: CombinedRepo[]): { repo: string; file: CombinedFile } | null {
  for (const r of repos) {
    const file = combinedTreeOrder(r.files)[0];
    if (file) return { repo: r.repo, file };
  }
  return null;
}

/**
 * How each side of a file's diff is read. Fast: both sides are plain revisions (a side the file
 * does not have is missing). Merge: the left side is rebuilt without the selection. Fallback: the
 * left side is the revision before the range, as one diff across it.
 */
export function combinedPlan(f: CombinedFile): { left: "rev" | "merge" | "missing"; right: "rev" | "missing" } {
  const right = f.status === "deleted" ? "missing" : "rev";
  if (f.mode === "merge") return { left: "merge", right };
  return { left: f.status === "added" || f.beforeRef === null ? "missing" : "rev", right };
}

// ---- races of the open file ---------------------------------------------------------------------

/**
 * What a new combined answer does to the file whose diff is open: it is gone (close), changed or
 * its read was cut short by a superseded query (`retry`: read it again), or is as it was (keep).
 */
export function reconcileOpen(cur: CombinedFile, again: CombinedFile | undefined, retry: boolean): "close" | "reread" | "keep" {
  if (!again) return "close";
  return retry || JSON.stringify(again) !== JSON.stringify(cur) ? "reread" : "keep";
}

/**
 * Enter with several commits picked. `open`: the answer is here and current. `flush`: a fetch is
 * waiting out its debounce, run it now (the old tree must not open). `wait`: one is in flight.
 * `start`: nothing is coming (e.g. after a failure).
 */
export function enterStep(o: { hasView: boolean; loading: boolean; pending: boolean }): "open" | "flush" | "wait" | "start" {
  if (o.pending) return "flush";
  if (o.loading) return "wait";
  return o.hasView ? "open" : "start";
}

/**
 * A left-side read refused as "stale query". A combined answer is still on its way: wait for it,
 * it re-reads the file (`cRetry`). No answer is coming (it landed while the read was running, or
 * the backend had not registered it yet): read again, once if the read is still current, then show
 * the error.
 */
export function staleStep(o: { superseded: boolean; loading: boolean; retried: boolean }): "wait" | "retry" | "fail" {
  if (o.loading) return "wait";
  if (o.superseded) return "retry";
  return o.retried ? "fail" : "retry";
}

// ---- the loader --------------------------------------------------------------------------------

export type CombinedState = { loading: boolean; view: CombinedView | null; error: string | null };
export const EMPTY_COMBINED: CombinedState = { loading: false, view: null, error: null };

export type CombinedFetcher = (id: number, items: { repo: string; sha: string }[]) => Promise<CombinedView>;

/**
 * One combined query at a time. A new `start` supersedes the one before it; every answer, good or
 * failed, is checked against the id it was asked for and against the id the backend echoes. A
 * refused request leaves the older selection's answer in flight: its id is old, so it is dropped.
 */
export class CombinedLoad {
  private current = 0;
  state: CombinedState = EMPTY_COMBINED;

  constructor(
    private readonly fetch: CombinedFetcher,
    private readonly emit: (s: CombinedState) => void,
    private readonly gate: QueryGate = queryGate,
    /** Tells the backend to drop the read in flight (kills its git), with a fresh query id. */
    private readonly cancel: (id: number) => void = () => {},
  ) {}

  /** The id of the query the shown tree answers (what a left-side read is tied to). */
  get id(): number {
    return this.current;
  }

  private set(s: CombinedState) {
    this.state = s;
    this.emit(s);
  }

  async start(items: { repo: string; sha: string }[]): Promise<void> {
    const id = this.gate.begin();
    this.current = id;
    // The old tree stays (dimmed) until the new one lands.
    this.set({ loading: true, view: this.state.view, error: null });
    try {
      const view = await this.fetch(id, items);
      if (id !== this.current || view.queryId !== id) return;
      this.set({ loading: false, view, error: null });
    } catch (e) {
      if (id !== this.current) return;
      this.set({ loading: false, view: null, error: isStale(e) ? "The selection changed while it was being read" : String(e) });
    }
  }

  /** Back to one commit: whatever is in flight is ignored, and the backend is told to kill it. */
  clear() {
    if (this.state.loading) this.cancel(this.gate.begin());
    this.current = 0;
    this.set(EMPTY_COMBINED);
  }
}

