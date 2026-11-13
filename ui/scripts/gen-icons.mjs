// Copies the Material Icon Theme SVGs that the manifest's lookup tables reference into
// public/icons/<id>.svg, plus the package LICENSE (MIT requires it to travel with the icons).
// Output is generated and gitignored; it runs before dev, build, check and test.
import { copyFileSync, mkdirSync, readFileSync, rmSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const manifestPath = require.resolve("material-icon-theme/dist/material-icons.json");
const pkgRoot = resolve(dirname(manifestPath), "..");
const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
const out = resolve(dirname(fileURLToPath(import.meta.url)), "..", "public", "icons");

const TABLES = ["fileNames", "fileExtensions", "folderNames", "folderNamesExpanded"];
const ids = new Set(["file", "folder", "folder-open", manifest.file, manifest.folder, manifest.folderExpanded]);
for (const source of [manifest, manifest.light ?? {}]) {
  for (const t of TABLES) {
    for (const id of Object.values(source[t] ?? {})) ids.add(id);
  }
}

rmSync(out, { recursive: true, force: true });
mkdirSync(out, { recursive: true });
for (const id of ids) {
  const def = manifest.iconDefinitions[id];
  if (!def) throw new Error(`manifest references undefined icon "${id}"`);
  copyFileSync(resolve(dirname(manifestPath), def.iconPath), join(out, `${id}.svg`));
}
copyFileSync(join(pkgRoot, "LICENSE"), join(out, "LICENSE"));
console.log(`gen-icons: ${ids.size} icons -> ${out}`);
