// Icon lookup over Material Icon Theme's generated manifest. Named imports let Vite drop the
// parts we never read (iconDefinitions, languageIds, highContrast) from the bundle. The SVGs
// themselves are copied to `public/icons/<id>.svg` by `scripts/gen-icons.mjs`.
import {
  fileExtensions,
  fileNames,
  folderNames,
  folderNamesExpanded,
  light,
} from "material-icon-theme/dist/material-icons.json";

type Table = Record<string, string>;
type Tables = { fileNames: Table; fileExtensions: Table; folderNames: Table; folderNamesExpanded: Table };

const DARK: Tables = { fileNames, fileExtensions, folderNames, folderNamesExpanded };
const LIGHT = light as Partial<Tables>;

export type IconTheme = "dark" | "light";

function lookup(theme: IconTheme, table: keyof Tables, key: string): string | undefined {
  return (theme === "light" ? LIGHT[table]?.[key] : undefined) ?? DARK[table][key];
}

function fileIcon(name: string, theme: IconTheme): string {
  const lower = name.toLowerCase();
  const exact = lookup(theme, "fileNames", lower);
  if (exact) return exact;
  // Longest suffix first: `a.test.ts` tries `test.ts` before `ts`.
  let dot = lower.indexOf(".");
  while (dot !== -1) {
    const hit = lookup(theme, "fileExtensions", lower.slice(dot + 1));
    if (hit) return hit;
    dot = lower.indexOf(".", dot + 1);
  }
  return "file";
}

function folderIcon(name: string, open: boolean, theme: IconTheme): string {
  const last = name.replace(/\\/g, "/").split("/").filter(Boolean).pop() ?? name;
  const hit = lookup(theme, open ? "folderNamesExpanded" : "folderNames", last.toLowerCase());
  return hit ?? (open ? "folder-open" : "folder");
}

/** Asset URL of the icon for a file or folder row. A compacted folder (`a/b`) uses its last segment. */
export function iconFor(
  name: string,
  kind: "file" | "folder",
  open: boolean,
  theme: IconTheme = "dark",
): string {
  const id = kind === "folder" ? folderIcon(name, open, theme) : fileIcon(name, theme);
  return `${import.meta.env.BASE_URL}icons/${id}.svg`;
}
