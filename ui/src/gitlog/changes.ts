// The selected commit's Changes tree and the diff behind it: what gitlog.rs `gitlog_changes` and
// `gitlog_file` return, turned into rows and pane states. Pure; Changes.svelte and GitLogView.svelte
// draw from it.
import type { DiffPayload, TextSide } from "../lib/api";
import { buildTree, type TreeNode } from "../lib/tree";

export type FileStatus = "added" | "modified" | "deleted" | "renamed" | "copied" | "typeChanged";

/** gitlog.rs `ChangedFileView`. `added`/`removed` are null for a binary file; `from` is a rename's old path. */
export type ChangedFile = {
  path: string;
  status: FileStatus;
  from?: string | null;
  added: number | null;
  removed: number | null;
};

export type FileSide = "before" | "after";

/** gitlog.rs `FileView`: one side of a changed file. */
export type FileView = { kind: "text"; value: string } | { kind: "binary" } | { kind: "tooLarge"; value: number } | { kind: "missing" };

export const STATUS_LETTER: Record<FileStatus, string> = {
  added: "A",
  modified: "M",
  deleted: "D",
  renamed: "R",
  copied: "C",
  typeChanged: "T",
};

export const STATUS_WORD: Record<FileStatus, string> = {
  added: "Added",
  modified: "Modified",
  deleted: "Deleted",
  renamed: "Renamed",
  copied: "Copied",
  typeChanged: "Type changed",
};

export function statusLetter(f: ChangedFile): string {
  return STATUS_LETTER[f.status] ?? "M";
}

/** "Renamed from a/b.ts", or just the status word. */
export function statusText(f: ChangedFile): string {
  const word = STATUS_WORD[f.status] ?? "Modified";
  return f.from ? `${word} from ${f.from}` : word;
}

/** `acme-api · 3f9a2c1 · 2 files` */
export function changesTitle(repoName: string, sha: string, files: number): string {
  return `${repoName} · ${sha.slice(0, 7)} · ${files} ${files === 1 ? "file" : "files"}`;
}

/** The +/- counts as text; a binary file has none. */
export function countsText(f: ChangedFile): string {
  return f.added === null || f.removed === null ? "binary" : `+${f.added} −${f.removed}`;
}

// ---- the tree ---------------------------------------------------------------------------------

export type ChangeItem = { rel: string; file: ChangedFile };
export type ChangeNode = TreeNode<ChangeItem>;

/** Folders first, then files, each alphabetical; a one-child folder chain is one row. */
export function changeTree(files: ChangedFile[]): ChangeNode[] {
  return buildTree(files.map((file) => ({ rel: file.path, file })));
}

export type VisibleNode<T = ChangeItem> = { node: TreeNode<T>; depth: number };

/** The rows on screen, in order, with the folders in `collapsed` (by `rel`) closed. */
export function visibleNodes<T = ChangeItem>(nodes: TreeNode<T>[], collapsed: ReadonlySet<string>, depth = 0): VisibleNode<T>[] {
  const out: VisibleNode<T>[] = [];
  for (const node of nodes) {
    out.push({ node, depth });
    if (node.kind === "folder" && !collapsed.has(node.rel)) out.push(...visibleNodes(node.children ?? [], collapsed, depth + 1));
  }
  return out;
}

/** The files in the order the tree lists them (what "the first changed file" means to the eye). */
export function treeOrder(files: ChangedFile[]): ChangedFile[] {
  const out: ChangedFile[] = [];
  const walk = (ns: ChangeNode[]) => {
    for (const n of ns) {
      if (n.kind === "folder") walk(n.children ?? []);
      else if (n.item) out.push(n.item.file);
    }
  };
  walk(changeTree(files));
  return out;
}

/** The file to show for a new commit: the one that was open when it is also in this commit, else the first. */
export function pickFile(files: ChangedFile[], previous: string | null): ChangedFile | null {
  if (files.length === 0) return null;
  return (previous && files.find((f) => f.path === previous)) || treeOrder(files)[0];
}

/** The path to pass as `from` for the before side: a file that kept its path needs no rename lookup. */
export function beforeFrom(f: ChangedFile): string | undefined {
  if (f.status === "modified" || f.status === "deleted" || f.status === "typeChanged") return f.path;
  return f.from ?? undefined;
}

/** The folders `path` sits in, outermost first, as the tree's `rel` keys (compacted chains included by the caller). */
export function foldersOf(path: string): string[] {
  const parts = path.split("/");
  return parts.slice(0, -1).map((_, i) => parts.slice(0, i + 1).join("/"));
}

// ---- the diff pane ----------------------------------------------------------------------------

export type DiffPane =
  | { kind: "diff"; payload: DiffPayload }
  | { kind: "binary" }
  | { kind: "tooLarge"; bytes: number }
  | { kind: "missing" };

const side = (v: FileView): TextSide => (v.kind === "text" ? { kind: "text", text: v.value } : { kind: "missing" });

/**
 * What the pane shows for a file's two sides. A binary side wins (there is nothing to diff), then a
 * too-large one; two missing sides (a path git no longer has) is "missing". A missing side next to a
 * text one is an added or deleted file: the diff against nothing.
 */
export function diffPane(path: string, before: FileView, after: FileView): DiffPane {
  if (before.kind === "binary" || after.kind === "binary") return { kind: "binary" };
  const big = [before, after].filter((v): v is { kind: "tooLarge"; value: number } => v.kind === "tooLarge");
  if (big.length) return { kind: "tooLarge", bytes: Math.max(...big.map((v) => v.value)) };
  if (before.kind === "missing" && after.kind === "missing") return { kind: "missing" };
  return { kind: "diff", payload: { path, before: side(before), after: side(after) } };
}

export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(n < 10 * 1024 ? 1 : 0)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}

/** The pane's message for the states that show no diff. */
export function paneMessage(p: Exclude<DiffPane, { kind: "diff" }>): { title: string; body: string } {
  switch (p.kind) {
    case "binary":
      return { title: "Binary file, not shown", body: "Git treats this file as binary, so there is no text to compare." };
    case "tooLarge":
      return { title: "File too large to show", body: `This file is ${formatBytes(p.bytes)}. Open it in your editor to read it.` };
    case "missing":
      return { title: "Nothing to show", body: "This commit has no version of this file on either side." };
  }
}

// ---- races ------------------------------------------------------------------------------------

/** A generation counter: a result may land only if nothing newer was asked for since. */
export function createGate() {
  let n = 0;
  return {
    next: () => ++n,
    current: () => n,
    isCurrent: (g: number) => g === n,
  };
}

/** The most recent `max` commits' file lists, so stepping back to one is instant. */
export class ChangesCache {
  private map = new Map<string, ChangedFile[]>();
  constructor(private max = 24) {}
  static key(repo: string, sha: string): string {
    return `${repo}\u0000${sha}`;
  }
  get(repo: string, sha: string): ChangedFile[] | undefined {
    const k = ChangesCache.key(repo, sha);
    const v = this.map.get(k);
    if (v) {
      this.map.delete(k);
      this.map.set(k, v);
    }
    return v;
  }
  set(repo: string, sha: string, files: ChangedFile[]) {
    const k = ChangesCache.key(repo, sha);
    this.map.delete(k);
    this.map.set(k, files);
    while (this.map.size > this.max) this.map.delete(this.map.keys().next().value as string);
  }
}

/** Typing "a/b" in the tree is not a feature; this is the one place the tree's nodes become ids. */
export const nodeId = (n: ChangeNode) => `${n.kind}:${n.rel}`;

/** Repeats within this window of a fetch are coalesced (holding ↓ spawns one call per rest, not per row). */
export const CHANGES_DEBOUNCE_MS = 80;

export type LeadingDebounced = {
  (): void;
  /** Runs a coalesced repeat now, if one is waiting. */
  flush(): void;
  /** Drops a waiting repeat (the cool-down continues). */
  drop(): void;
  hasPending(): boolean;
};

/**
 * Leading edge: the first call after a quiet `ms` runs at once (a click on a commit costs no
 * delay). Calls inside the window coalesce into one trailing run, `ms` after the last of them.
 */
export function leadingDebounce(fn: () => void, ms: number): LeadingDebounced {
  let timer: ReturnType<typeof setTimeout> | null = null;
  let pending = false;
  const cool = () => {
    timer = setTimeout(() => {
      timer = null;
      if (pending) {
        pending = false;
        fn();
        cool();
      }
    }, ms);
  };
  const d = (() => {
    if (!timer) {
      fn();
      cool();
      return;
    }
    pending = true;
    clearTimeout(timer);
    cool();
  }) as LeadingDebounced;
  d.flush = () => {
    if (!pending) return;
    pending = false;
    if (timer) clearTimeout(timer);
    fn();
    cool();
  };
  d.drop = () => {
    pending = false;
  };
  d.hasPending = () => pending;
  return d;
}

/**
 * What Enter on a commit does: focus the tree when its files are here, wait when they are on their
 * way, run the waiting fetch now, or start one (nothing pending or in flight, e.g. after a failure).
 */
export function enterAction(o: { loaded: boolean; inflight: boolean; pending: boolean }): "focus" | "wait" | "flush" | "load" {
  if (o.loaded) return "focus";
  if (o.inflight) return "wait";
  return o.pending ? "flush" : "load";
}
