// `claudegate [PATH]` and `claudegate log [PATH]`: which project owns the path the app was launched with.

/** What the launch asks for: the review of the path's project, or the Git log with its repos ticked. */
export type LaunchKind = "review" | "log";

/** The path from Rust (src-tauri/src/launch.rs); `isDir` because the UI cannot stat. */
export type LaunchTarget = { kind: LaunchKind; path: string; exists: boolean; isDir: boolean; canonical: string | null };

/** A project and every root it shows: its own and its worktrees'. */
export type Owner = { root: string; scopes: string[] };

const norm = (p: string) => {
  const s = p.replace(/\\/g, "/").replace(/\/+$/, "");
  return s === "" && p.startsWith("/") ? "/" : s;
};
const within = (root: string, path: string) => path === root || path.startsWith(root === "/" ? "/" : `${root}/`);

/** The project whose root or worktree scope is the longest prefix of `path`; null when none is. */
export function ownerOf(path: string, owners: Owner[], canonical?: string | null): string | null {
  const paths = [norm(path), ...(canonical ? [norm(canonical)] : [])];
  let best: string | null = null;
  let bestLen = -1;
  for (const o of owners) {
    for (const r of [o.root, ...o.scopes]) {
      const n = norm(r);
      if (n.length > bestLen && paths.some((p) => within(n, p))) {
        best = o.root;
        bestLen = n.length;
      }
    }
  }
  return best;
}

/** The pending row for exactly this path (a file launch); null for a folder or an unknown file. */
export function findPendingRow<R extends { path: string; scope?: string }>(path: string, rows: R[]): R | null {
  const p = norm(path);
  // The same path can be listed in two scopes: the most specific (longest) scope owns it.
  const hits = rows.filter((r) => norm(r.path) === p);
  return hits.reduce<R | null>((best, r) => (!best || norm(r.scope ?? "").length > norm(best.scope ?? "").length ? r : best), null);
}

/** The Add project button's label: names the folder when the path is a file. */
export function addLabel(t: LaunchTarget): string {
  return t.isDir ? "Add project" : `Add ${addFolderFor(t)}`;
}

/** The launch notice for a path in a known project outside the workspace. */
export function knownNotice(t: LaunchTarget, name: string): string {
  return `${t.path} is in ${name}, which isn't in your workspace.`;
}

/** What the launch notice says. */
export function launchNotice(t: LaunchTarget): string {
  return t.exists ? `${t.path} isn't a ClaudeGate project.` : `${t.path} doesn't exist.`;
}

/** The folder the Add project flow gets: the folder itself, or the file's folder. */
export function addFolderFor(t: LaunchTarget): string {
  const p = norm(t.path);
  return t.isDir ? p : p.slice(0, Math.max(p.lastIndexOf("/"), 0)) || "/";
}

/**
 * The repositories a `claudegate log PATH` launch ticks: those with a checkout (main or worktree)
 * at or under the path, plus the deepest repository whose checkout contains the path (the path is
 * inside it). Both the path and its symlink-free form are tried. Ids, in the list's order.
 */
export function reposForLaunch(
  repos: { id: string; checkouts: { path: string }[] }[],
  path: string,
  canonical?: string | null,
): string[] {
  const targets = [norm(path), ...(canonical ? [norm(canonical)] : [])];
  const under = new Set<string>();
  let deepest: { id: string; len: number } | null = null;
  for (const r of repos) {
    for (const c of r.checkouts) {
      const cp = norm(c.path);
      if (targets.some((t) => within(t, cp))) under.add(r.id);
      else if (targets.some((t) => within(cp, t)) && (!deepest || cp.length > deepest.len)) deepest = { id: r.id, len: cp.length };
    }
  }
  if (deepest) under.add(deepest.id);
  return repos.filter((r) => under.has(r.id)).map((r) => r.id);
}
