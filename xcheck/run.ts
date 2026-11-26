import * as fs from "fs";
import * as path from "path";
import {
  acceptEntry,
  capAcceptedBytes,
  mergeFreshCaptures,
  migrateSession,
  rejectEntry,
  shouldPruneNoOp,
  type MergeGuards,
} from "../.xcheck-cache/claudegate/src/reviewModel";
import { DEFAULT_EXCLUDES, DEFAULT_PROTECTED, ExcludeMatcher } from "../.xcheck-cache/claudegate/src/excludeMatcher";

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

// Upkeep fixtures: capAcceptedBytes over a migrated session, and shouldPruneNoOp cases.
const upkeepDir = path.join(fixtures, "upkeep");
const upkeepFiles = fs.readdirSync(upkeepDir).filter((f) => f.endsWith(".json")).sort();
let upkeepCases = 0;
for (const f of upkeepFiles) {
  const fx = JSON.parse(fs.readFileSync(path.join(upkeepDir, f), "utf8"));
  if (fx.op === "capAcceptedBytes") {
    const s = session(fx.session);
    const dropped = capAcceptedBytes(s, fx.maxBytes);
    const d = firstDiff({ dropped, acceptedIds: s.accepted.map((r) => r.id) }, fx.expected);
    if (d) fail(`upkeep ${f}: ${d}`);
    upkeepCases++;
  } else if (fx.op === "shouldPruneNoOp") {
    for (const c of fx.cases) {
      const got = shouldPruneNoOp({ reviewStatus: "pending", ...c.entry }, c.disk, c.nowMs);
      if (got !== c.prune) fail(`upkeep ${f} "${c.name}": extension=${got}, fixture=${c.prune}`);
      upkeepCases++;
    }
  } else fail(`upkeep ${f}: unknown op ${fx.op}`);
}

// Upkeep constants: core's copies must equal the extension's source.
{
  const num = (t: string | undefined) => (t === undefined ? undefined : Number(t.replace(/_/g, "")));
  const rsConst = (file: string, name: string) =>
    num(fs.readFileSync(path.join(root, "core", "src", file), "utf8").match(new RegExp(`pub const ${name}: \\w+ = ([\\d_]+);`))?.[1]);
  const tsConst = (file: string, name: string) =>
    num(fs.readFileSync(path.join(root, ".xcheck-cache", "claudegate", "src", file), "utf8").match(new RegExp(`const ${name} = ([\\d_]+);`))?.[1]);
  for (const [rs, ts, name] of [
    ["upkeep.rs", "sessionManager.ts", "SESSION_SIZE_WARN_BYTES"],
    ["upkeep.rs", "sessionManager.ts", "MAX_ACCEPTED_BYTES"],
    ["project.rs", "reviewModel.ts", "NOOP_SETTLE_MS"],
    ["project.rs", "reviewModel.ts", "NEW_FILE_ABSENT_MS"],
    ["model.rs", "reviewModel.ts", "MAX_ACCEPTED_RECORDS"],
  ]) {
    const a = rsConst(rs, name), b = tsConst(ts, name);
    if (a === undefined || a !== b) fail(`upkeep constant ${name}: core=${a}, extension=${b}`);
  }
}

// Settings defaults: the constants copied into core/src/settings.rs must equal the extension's.
{
  const rs = fs.readFileSync(path.join(root, "core", "src", "settings.rs"), "utf8");
  const rsList = (name: string): string[] => {
    const m = rs.match(new RegExp(`pub const ${name}: &\\[&str\\] = &\\[([^\\]]*)\\];`));
    return m ? [...m[1].matchAll(/"([^"]*)"/g)].map((x) => x[1]) : [];
  };
  const rsScalar = (name: string): string | undefined => rs.match(new RegExp(`pub const ${name}: \\w+ = ([^;]+);`))?.[1].trim();
  const extPkg = JSON.parse(fs.readFileSync(path.join(root, ".xcheck-cache", "claudegate", "package.json"), "utf8"));
  const props = extPkg.contributes.configuration.properties;
  const keysOf = (o: Record<string, boolean>) => Object.keys(o).filter((k) => o[k]);
  const same = (name: string, a: unknown, b: unknown) => {
    if (JSON.stringify(a) !== JSON.stringify(b)) fail(`settings default ${name}: core=${JSON.stringify(a)}, extension=${JSON.stringify(b)}`);
  };
  same("DEFAULT_EXCLUDES", rsList("DEFAULT_EXCLUDES"), DEFAULT_EXCLUDES);
  same("DEFAULT_PROTECTED", rsList("DEFAULT_PROTECTED"), DEFAULT_PROTECTED);
  same("package.json claudegate.exclude", keysOf(props["claudegate.exclude"].default), DEFAULT_EXCLUDES);
  same("package.json claudegate.protected", keysOf(props["claudegate.protected"].default), DEFAULT_PROTECTED);
  const scalars: [string, string][] = [
    ["DEFAULT_AUTO_ADVANCE", "claudegate.autoAdvance"],
    ["DEFAULT_GROUP_BY_SESSION", "claudegate.groupBySession"],
    ["DEFAULT_PENDING_AGE_MIN_DAYS", "claudegate.pendingAge.minDays"],
    ["DEFAULT_MAX_ATTACHED", "claudegate.worktrees.maxAttached"],
    ["DEFAULT_HOOK_LOG", "claudegate.hookLog.enabled"],
  ];
  for (const [c, k] of scalars) same(c, rsScalar(c), String(props[k].default));
}

// The bundled hook is committed, not fetched at build time: it must equal the pinned tag's
// hooks/hook.py once the CLAUDEGATE_HOOK_VERSION stamp line is removed.
{
  const stampRe = /^# CLAUDEGATE_HOOK_VERSION=.*\n/m;
  const bundled = fs.readFileSync(path.join(root, "src-tauri", "resources", "hook.py"), "utf8");
  const pinned = fs.readFileSync(path.join(root, ".xcheck-cache", "claudegate", "hooks", "hook.py"), "utf8");
  if (!stampRe.test(bundled)) fail("bundled hook.py has no CLAUDEGATE_HOOK_VERSION stamp");
  else if (bundled.replace(stampRe, "") !== pinned) fail("bundled hook.py differs from the pinned tag's hooks/hook.py (ignoring the stamp line)");
  // audit.py carries no stamp: byte for byte.
  const bundledAudit = fs.readFileSync(path.join(root, "src-tauri", "resources", "audit.py"), "utf8");
  const pinnedAudit = fs.readFileSync(path.join(root, ".xcheck-cache", "claudegate", "hooks", "audit.py"), "utf8");
  if (bundledAudit !== pinnedAudit) fail("bundled audit.py differs from the pinned tag's hooks/audit.py");
}

// Capture setup: the shared fixtures through the extension's own hookInstaller.ts. It imports
// "vscode", so that one name resolves to the extension's own test stub.
let captureCases = 0;
{
  const Module = require("module");
  const ext = path.join(root, ".xcheck-cache", "claudegate");
  const resolve = Module._resolveFilename;
  Module._resolveFilename = function (req: string, ...rest: unknown[]) {
    return req === "vscode" ? path.join(ext, "src", "test-stubs", "vscode.ts") : resolve.call(this, req, ...rest);
  };
  const hi = require(path.join(ext, "src", "hookInstaller.ts"));
  Module._resolveFilename = resolve;
  const cap = (f: string) => JSON.parse(fs.readFileSync(path.join(fixtures, "capture", f), "utf8"));

  const patch = cap("settings_patch.json");
  for (const c of patch.cases) {
    const r = hi.computeSettingsPatch(c.raw, c.hookCommand, c.auditCommand);
    const d = firstDiff({ content: r.content, changed: r.changed, parseError: !!r.parseError }, c.expected);
    if (d) fail(`capture patch "${c.name}": ${d}`);
    captureCases++;
  }
  // Shapes both refuse: parseError + shapeError, the original bytes echoed, nothing changed.
  for (const c of patch.refuse) {
    const r = hi.computeSettingsPatch(c.raw, c.hookCommand, c.auditCommand);
    if (!(r.parseError && r.shapeError && !r.changed && r.content === c.raw)) {
      fail(`capture refuse "${c.name}": the extension does not refuse (${JSON.stringify({ ...r, content: undefined })})`);
    }
    captureCases++;
  }
  for (const c of cap("audit_command_shape.json").cases) {
    const got = hi.AUDIT_COMMAND_SHAPE.test(c.command);
    if (got !== c.ours) fail(`capture audit shape ${JSON.stringify(c.command)}: extension=${got}, fixture=${c.ours}`);
    captureCases++;
  }
  for (const c of cap("backups_to_prune.json").cases) {
    const d = firstDiff(hi.backupsToPrune(c.names, c.baseName, c.keep), c.expected);
    if (d) fail(`capture prune "${c.name}": ${d}`);
    captureCases++;
  }
  for (const c of cap("verify_settings.json").cases) {
    const got = hi.verifySettingsContent(c.after, c.previous, c.hookCommand, c.auditCommand);
    if (got !== c.expected) fail(`capture verify "${c.name}": extension=${got}, fixture=${c.expected}`);
    captureCases++;
  }

  // Constants core copies: the matcher, the keep count, and the POSIX wrapper's text.
  const rs = fs.readFileSync(path.join(root, "core", "src", "capture.rs"), "utf8");
  const rsStr = (name: string) => {
    const m = rs.match(new RegExp(`pub const ${name}: &str = "((?:[^"\\\\]|\\\\.)*)";`));
    return m ? JSON.parse(`"${m[1]}"`) : undefined;
  };
  if (rsStr("PRE_TOOL_MATCHER") !== hi.PRE_TOOL_MATCHER) fail(`capture PRE_TOOL_MATCHER: core=${rsStr("PRE_TOOL_MATCHER")}, extension=${hi.PRE_TOOL_MATCHER}`);
  const keep = rs.match(/pub const SETTINGS_BACKUP_RETENTION: usize = (\d+);/)?.[1];
  if (Number(keep) !== hi.SETTINGS_BACKUP_RETENTION) fail(`capture SETTINGS_BACKUP_RETENTION: core=${keep}, extension=${hi.SETTINGS_BACKUP_RETENTION}`);
  const ts = fs.readFileSync(path.join(ext, "src", "hookInstaller.ts"), "utf8");
  const wrapper = ts.match(/const sh = `([^`]*)`;/)?.[1].replace("${this.pythonCmd}", "python3").replace(/\\n/g, "\n");
  if (rsStr("HOOK_SH_CONTENT") !== wrapper) fail(`capture HOOK_SH_CONTENT: core=${JSON.stringify(rsStr("HOOK_SH_CONTENT"))}, extension=${JSON.stringify(wrapper)}`);
  // The audit wrapper is a concatenation of template literals.
  const auditParts = ts.match(/const auditSh =\s*((?:`[^`]*`\s*\+?\s*)+);/)?.[1].match(/`[^`]*`/g);
  const auditWrapper = auditParts?.map((p) => p.slice(1, -1)).join("").replace("${this.pythonCmd}", "python3").replace(/\\n/g, "\n");
  if (!auditWrapper || rsStr("AUDIT_SH_CONTENT") !== auditWrapper) {
    fail(`capture AUDIT_SH_CONTENT: core=${JSON.stringify(rsStr("AUDIT_SH_CONTENT"))}, extension=${JSON.stringify(auditWrapper)}`);
  }
  if (rsStr("POST_TOOL_MATCHER") !== hi.POST_TOOL_MATCHER) fail(`capture POST_TOOL_MATCHER: core=${rsStr("POST_TOOL_MATCHER")}, extension=${hi.POST_TOOL_MATCHER}`);
  const events = rs.match(/pub const AUDIT_EVENTS: \[&str; \d+\] = \[([^\]]*)\];/)?.[1];
  const rsEvents = events ? JSON.parse(`[${events}]`) : undefined;
  if (JSON.stringify(rsEvents) !== JSON.stringify(hi.AUDIT_EVENTS)) fail(`capture AUDIT_EVENTS: core=${JSON.stringify(rsEvents)}, extension=${JSON.stringify(hi.AUDIT_EVENTS)}`);
}

if (failures) {
  console.error(`${failures} mismatch(es)`);
  process.exit(1);
}
console.log(`ok - ${sessionFiles.length} session cases, ${ex.cases.length} exclude cases, ${mergeFiles.length} merge cases, ${upkeepCases} upkeep cases, ${captureCases} capture cases`);
