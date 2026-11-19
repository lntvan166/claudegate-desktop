// `claudegate [PATH]`: which project owns the path the app was launched with.

/** The path from Rust (src-tauri/src/launch.rs); `isDir` because the UI cannot stat. */
export type LaunchTarget = { path: string; isDir: boolean };

/** A project and every root it shows: its own and its worktrees'. */
export type Owner = { root: string; scopes: string[] };

const norm = (p: string) => {
  const s = p.replace(/\\/g, "/").replace(/\/+$/, "");
  return s === "" && p.startsWith("/") ? "/" : s;
};
const within = (root: string, path: string) => path === root || path.startsWith(root === "/" ? "/" : `${root}/`);

/** The project whose root or worktree scope is the longest prefix of `path`; null when none is. */
export function ownerOf(path: string, owners: Owner[]): string | null {
  const p = norm(path);
  let best: string | null = null;
  let bestLen = -1;
  for (const o of owners) {
    for (const r of [o.root, ...o.scopes]) {
      const n = norm(r);
      if (within(n, p) && n.length > bestLen) {
        best = o.root;
        bestLen = n.length;
      }
    }
  }
  return best;
}

/** The pending row for exactly this path (a file launch); null for a folder or an unknown file. */
export function findPendingRow<R extends { path: string }>(path: string, rows: R[]): R | null {
  const p = norm(path);
  return rows.find((r) => norm(r.path) === p) ?? null;
}

/** The folder the Add project flow gets: the folder itself, or the file's folder. */
export function addFolderFor(t: LaunchTarget): string {
  const p = norm(t.path);
  return t.isDir ? p : p.slice(0, Math.max(p.lastIndexOf("/"), 0)) || "/";
}
