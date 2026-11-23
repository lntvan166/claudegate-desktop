// Small UI choices that survive a restart, kept in localStorage (never in the shared ~/.claudegate).

export type ListMode = "tree" | "list";

export const MODE_KEY = "claudegate.listMode";
export const COLLAPSED_KEY = "claudegate.collapsed";
export const WRAP_KEY = "claudegate.diffWrap";

/** How many collapsed folders and groups are kept; the oldest go first. */
export const COLLAPSED_MAX = 500;

/** The stored tree/list mode; tree when nothing (or something unknown) is stored. */
export function loadMode(raw: string | null): ListMode {
  return raw === "list" ? "list" : "tree";
}

/**
 * The stored collapse keys. They carry the tab and the scope's absolute root (see `visible.ts`),
 * so one list holds every project's folders without mixing them.
 */
export function loadCollapsedKeys(raw: string | null): string[] {
  if (!raw) return [];
  try {
    const v: unknown = JSON.parse(raw);
    return Array.isArray(v) ? v.filter((k): k is string => typeof k === "string").slice(-COLLAPSED_MAX) : [];
  } catch {
    return [];
  }
}

/** The keys to store, newest last, capped so folders that no longer exist cannot pile up forever. */
export function saveCollapsedKeys(keys: Iterable<string>): string {
  return JSON.stringify([...keys].slice(-COLLAPSED_MAX));
}

/** The stored word-wrap choice for the diff; off unless turned on. */
export function loadWrap(raw: string | null): boolean {
  return raw === "on";
}

/** Reads `key` from localStorage; null when storage is unavailable. */
export function readStore(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

/** Writes `key` to localStorage; a full or blocked storage only loses the convenience. */
export function writeStore(key: string, value: string): void {
  try {
    localStorage.setItem(key, value);
  } catch {}
}
