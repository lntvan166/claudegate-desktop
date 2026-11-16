import * as fs from "fs";
import * as path from "path";
import {
  acceptEntry,
  mergeFreshCaptures,
  migrateSession,
  rejectEntry,
  type MergeGuards,
} from "../.xcheck-cache/claudegate/src/reviewModel";
import { ExcludeMatcher } from "../.xcheck-cache/claudegate/src/excludeMatcher";

const root = path.resolve(__dirname, "..");
const fixtures = path.join(root, "fixtures");

/** Path of the first difference between two JSON values, or null if equal. */
function firstDiff(a: unknown, b: unknown, at = "$"): string | null {
  if (a === b) return null;
  const isObj = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null;
  if (!isObj(a) || !isObj(b) || Array.isArray(a) !== Array.isArray(b)) {
    return `${at}: ${JSON.stringify(a)} !== ${JSON.stringify(b)}`;
  }
  const keys = new Set([...Object.keys(a), ...Object.keys(b)]);
  for (const k of [...keys].sort()) {
    if (!(k in a)) return `${at}.${k}: missing in extension output`;
    if (!(k in b)) return `${at}.${k}: missing in fixture expected`;
    const d = firstDiff(a[k], b[k], `${at}.${k}`);
    if (d) return d;
  }
  return null;
}

let failures = 0;
const fail = (msg: string) => { failures++; console.error(`FAIL - ${msg}`); };

const sessionDir = path.join(fixtures, "sessions");
const sessionFiles = fs.readdirSync(sessionDir).filter((f) => f.endsWith(".json")).sort();
for (const f of sessionFiles) {
  const fx = JSON.parse(fs.readFileSync(path.join(sessionDir, f), "utf8"));
  const out = JSON.parse(JSON.stringify(migrateSession(JSON.parse(JSON.stringify(fx.input))).session));
  const d = firstDiff(out, fx.expected);
  if (d) fail(`session ${f}: ${d}`);
}

const ex = JSON.parse(fs.readFileSync(path.join(fixtures, "exclude_cases.json"), "utf8"));
for (const c of ex.cases) {
  const m = new ExcludeMatcher();
  m.reload(Object.fromEntries(c.globs.map((g: string) => [g, true])), ex.root);
  const got = m.isExcluded(c.path);
  if (got !== c.excluded) fail(`exclude globs=${JSON.stringify(c.globs)} path=${c.path}: extension=${got}, fixture=${c.excluded}`);
}

// Merge fixtures carry { mine, disk, guards, expected }; decision fixtures add an `op`.
const session = (raw: unknown) => migrateSession(JSON.parse(JSON.stringify(raw))).session;
const mergeDir = path.join(fixtures, "merge");
const mergeFiles = fs.readdirSync(mergeDir).filter((f) => f.endsWith(".json")).sort();
for (const f of mergeFiles) {
  const fx = JSON.parse(fs.readFileSync(path.join(mergeDir, f), "utf8"));
  let got;
  if (fx.op === undefined) {
    const g = fx.guards ?? {};
    const guards: MergeGuards = {
      prunedFiles: g.prunedFiles
        ? new Map(Object.entries<string | null>(g.prunedFiles).map(([k, v]) => [k, v ?? undefined]))
        : undefined,
      droppedAcceptedIds: g.droppedAcceptedIds ? new Set(g.droppedAcceptedIds) : undefined,
      droppedRejectedPaths: g.droppedRejectedPaths ? new Set(g.droppedRejectedPaths) : undefined,
    };
    got = mergeFreshCaptures(session(fx.mine), session(fx.disk), guards);
  } else {
    got = session(fx.session);
    if (fx.op === "accept") acceptEntry(got, fx.path, fx.after, fx.decidedAt);
    else if (fx.op === "reject") rejectEntry(got, fx.path, fx.after, fx.decidedAt);
    else { fail(`merge ${f}: unknown op ${fx.op}`); continue; }
  }
  const d = firstDiff(JSON.parse(JSON.stringify(got)), fx.expected);
  if (d) fail(`merge ${f}: ${d}`);
}

if (failures) {
  console.error(`${failures} mismatch(es)`);
  process.exit(1);
}
console.log(`ok - ${sessionFiles.length} session cases, ${ex.cases.length} exclude cases, ${mergeFiles.length} merge cases`);
