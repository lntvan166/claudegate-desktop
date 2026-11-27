// Compare Branches' pure helpers: the branch picker's groups, search highlight and titles.
// A branch is keyed on (name, remote): a local `origin/x` and the remote-tracking `origin/x` are two
// refs, and the compare resolves exactly the one that was picked.

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

/** The view's description: the pair, without "origin/". */
export function tabTitle(p: Pair | null): string {
  if (!p || !p.left || !p.right) return "";
  const short = (n: string) => n.replace(/^origin\//, "");
  return `${short(p.left)} ↔ ${short(p.right)}`;
}
