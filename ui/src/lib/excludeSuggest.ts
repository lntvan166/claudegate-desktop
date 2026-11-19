// Turning a row into an exclude glob. Port of the extension's excludeSuggest.ts. Only two shapes:
// anything cleverer guesses at intent, and an exclude that is wrong hides changes silently.
// Paths use `/` or `\`; the globs always use `/`.

const norm = (p: string) => p.replace(/\\/g, "/");

/**
 * Glob for "every file with this extension, anywhere"; undefined with no extension (`**` over an
 * extensionless name would match far more than meant). A dotfile like `.env` has no extension.
 */
export function extensionGlob(filePath: string): string | undefined {
  const name = norm(filePath).split("/").pop() ?? "";
  const dot = name.lastIndexOf(".");
  if (dot <= 0) return undefined;
  const ext = name.slice(dot);
  return ext === "." ? undefined : `**/*${ext}`;
}

/**
 * Glob for "everything under this file's directory", relative to `scopeRoot` when the file is
 * inside it. Undefined for a file at the root (it would hide the whole review).
 */
export function folderGlob(filePath: string, scopeRoot?: string): string | undefined {
  const parts = norm(filePath).split("/");
  parts.pop();
  const dir = parts.join("/");
  if (!dir || dir === ".") return undefined;
  if (scopeRoot) {
    const root = norm(scopeRoot).replace(/\/+$/, "");
    if (dir === root) return undefined;
    if (dir.startsWith(`${root}/`)) return `**/${dir.slice(root.length + 1)}/**`;
  }
  const base = parts[parts.length - 1];
  return base ? `**/${base}/**` : undefined;
}

/** What to call the glob in a prompt: the rule, not a pattern to parse. */
export function describeGlob(glob: string): string {
  const m = /^\*\*\/\*(\.[^/]+)$/.exec(glob);
  if (m) return `every ${m[1]} file`;
  const f = /^\*\*\/(.+)\/\*\*$/.exec(glob);
  if (f) return `everything under ${f[1]}/`;
  return glob;
}

/** The question before hiding: what leaves the list, and that nothing is decided. */
export function hidePrompt(glob: string, count: number): string {
  return `Hide ${describeGlob(glob)} from review? ${count} pending file(s) would leave the list, and future ones would not appear. This does NOT accept or reject anything — the changes stay on disk and the entries stay in the session. Pattern: ${glob}`;
}

export const alreadyExcluded = (glob: string) => `"${glob}" is already excluded.`;
export const excludedNotice = (glob: string) => `Excluded ${glob}. Remove it in Settings to bring those files back.`;
