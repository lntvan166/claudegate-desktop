// Dev-only fixtures for previewing the UI in a plain browser (`npm run dev`, no Tauri).
// Loaded through a DEV-guarded dynamic import in api.ts, so it never ships.
import type {
  Api,
  CaptureView,
  ChangedPayload,
  HookView,
  DecideAction,
  DecideReport,
  DecideTarget,
  DiffKind,
  DiffPayload,
  EditorConfig,
  PendingItem,
  ProjectSummary,
  ProjectView,
  RecordItem,
  SettingKey,
  SettingsView,
} from "./api";
import type { FilterView, PageView, RowView } from "../gitlog/log";
import type { ChangedFile, FileView } from "../gitlog/changes";
import { DEFAULT_GITLOG_CONFIG, SCAN_DEPTH_MAX, type GitLogConfig, type RepoView } from "../gitlog/repos";
import { DEFAULT_PROTECTED, globMatcher, type ExcludeOwner, type Resolved } from "./settings";

const SHOP = "/home/dev/src/shop-api";
const WT = `${SHOP}/.worktrees/checkout-v2`;
const HERDR = "/home/dev/src/herdr-pokemon";
const BILLING = "/home/dev/src/legacy-billing";
const NOTES = "/home/dev/src/field-notes";
/** An umbrella repo with a registered repo nested inside it, shown as one of its scopes. */
const UMBRELLA = "/home/dev/src/umbrella";
const UMB_NESTED = `${UMBRELLA}/umbrella-orders`;
/** Known projects outside the workspace with pending work: the rail's elsewhere line counts them. */
const LAB = "/home/dev/src/scratch-lab";
const LEDGER = "/home/dev/src/ledger-sync";
/** Added in the app (a Zed-only project), so it is marked and removable. */
const TIDE = "/home/dev/src/tide-sim";

const ago = (min: number) => new Date(Date.now() - min * 60_000).toISOString();

function pending(scope: string, rel: string, added: number, removed: number, newFile = false): PendingItem {
  const path = `${scope}/${rel}`;
  // scripts/seed.py has a null baseline from an older hook: a reject leaves it on disk.
  const rejectOutcome = newFile ? "delete" : rel.endsWith("seed.py") ? "leave" : "restore";
  // Two Claude sessions: this morning's (older captures) and the one running now.
  const sessionId = added > 20 ? "sess-morning" : "sess-now";
  return { path, rel: path.slice(SHOP.length + 1), scope, newFile, capturedAt: ago(added), sessionId, added, removed, waiting: false, rejectOutcome };
}

function pendingIn(root: string, rel: string, added: number, removed: number): PendingItem {
  const path = `${root}/${rel}`;
  return { path, rel, scope: root, newFile: false, capturedAt: ago(added), added, removed, waiting: false, rejectOutcome: "restore" };
}

function record(root: string, rel: string, min: number, added: number, removed: number): RecordItem {
  const decidedAt = ago(min);
  const path = `${root}/${rel}`;
  return { id: `${decidedAt}::${path}`, path, rel, scope: root, decidedAt, added, removed };
}

const views: Record<string, ProjectView> = {
  [SHOP]: {
    root: SHOP,
    missing: false,
    scopes: [
      { root: SHOP, isWorktree: false, shapeUnknown: [], error: null },
      { root: WT, isWorktree: true, shapeUnknown: [], error: null },
    ],
    pending: [
      pending(SHOP, "README.md", 12, 3),
      pending(SHOP, "package.json", 2, 1),
      pending(SHOP, "src/api/client.ts", 41, 17),
      pending(SHOP, "src/api/routes/orders.ts", 64, 8),
      pending(SHOP, "src/utils.ts", 9, 4),
      pending(SHOP, "src/db/migrations/0007_refunds.sql", 23, 0, true),
      pending(SHOP, "docker-compose.yml", 5, 2),
      pending(SHOP, "internal/refunds/refund.go", 19, 6),
      pending(SHOP, "scripts/seed.py", 11, 3),
      // A secret: the protected globs match it, so accepting it asks first.
      pending(SHOP, ".env.local", 2, 1),
      // Captured four days ago by a hook with no session id; it has sat in the backlog since.
      { ...pending(SHOP, "docs/runbook.md", 6, 2), capturedAt: ago(4 * 1440 + 30), sessionId: null },
      // Captured, but Claude's write has not landed: bulk actions skip it.
      { ...pending(SHOP, "src/api/types.ts", 0, 0), capturedAt: ago(0), waiting: true },
      pending(WT, "src/checkout/cart.svelte", 88, 31),
      pending(WT, "src/checkout/stripe.test.ts", 54, 0, true),
      pending(WT, ".github/workflows/ci.yml", 7, 3),
    ],
    // src/index.ts accepted twice: the log is append-only, so one path can repeat.
    accepted: [record(SHOP, "src/index.ts", 95, 6, 2), record(SHOP, "tsconfig.json", 240, 1, 1), record(SHOP, "src/index.ts", 300, 3, 1)],
    rejected: [record(SHOP, "src/legacy/session.js", 180, 0, 112)],
    nextRecheckAt: null,
  },
  [HERDR]: {
    root: HERDR,
    missing: false,
    scopes: [{ root: HERDR, isWorktree: false, shapeUnknown: [], error: null }],
    pending: [],
    accepted: [
      record(HERDR, "crates/battle/src/damage.rs", 30, 22, 9),
      record(HERDR, "crates/battle/src/types.rs", 31, 4, 0),
      record(HERDR, "Cargo.toml", 33, 1, 1),
      record(HERDR, "assets/sprites/pikachu.png", 1440, 0, 0),
    ],
    rejected: [record(HERDR, "crates/dex/src/lib.rs", 50, 140, 2), record(HERDR, "Makefile", 2900, 3, 3)],
    nextRecheckAt: null,
  },
  // Written by a newer ClaudeGate: shown, never decided.
  [NOTES]: {
    root: NOTES,
    missing: false,
    scopes: [{ root: NOTES, isWorktree: false, shapeUnknown: ["reviewNotes"], error: null }],
    pending: [pendingIn(NOTES, "src/sync.ts", 14, 5), pendingIn(NOTES, "README.md", 3, 0)],
    accepted: [],
    rejected: [],
    nextRecheckAt: null,
  },
  [UMBRELLA]: {
    root: UMBRELLA,
    missing: false,
    scopes: [
      { root: UMBRELLA, isWorktree: false, shapeUnknown: [], error: null },
      { root: UMB_NESTED, isWorktree: false, nested: true, shapeUnknown: [], error: null },
    ],
    pending: [
      pendingIn(UMBRELLA, "docs/architecture.md", 8, 2),
      pendingIn(UMB_NESTED, "internal/order/sync.go", 37, 12),
      pendingIn(UMB_NESTED, "internal/order/sync_test.go", 52, 0),
      pendingIn(UMB_NESTED, "go.mod", 1, 1),
    ],
    accepted: [],
    rejected: [],
    nextRecheckAt: null,
  },
  [LAB]: {
    root: LAB,
    missing: false,
    scopes: [{ root: LAB, isWorktree: false, shapeUnknown: [], error: null }],
    pending: [
      { ...pendingIn(LAB, "notebooks/fit.py", 26, 4), capturedAt: ago(3 * 1440) },
      { ...pendingIn(LAB, "data/README.md", 5, 0), capturedAt: ago(3 * 1440 + 20) },
      { ...pendingIn(LAB, "pyproject.toml", 2, 1), capturedAt: ago(3 * 1440 + 25) },
    ],
    accepted: [],
    rejected: [],
    nextRecheckAt: null,
  },
  [LEDGER]: {
    root: LEDGER,
    missing: false,
    scopes: [{ root: LEDGER, isWorktree: false, shapeUnknown: [], error: null }],
    pending: [
      pendingIn(LEDGER, "src/reconcile.ts", 18, 6),
      pendingIn(LEDGER, "src/reconcile.test.ts", 33, 0),
      pendingIn(LEDGER, "README.md", 4, 1),
      pendingIn(LEDGER, "package.json", 1, 1),
      // Not landed yet: counted on its own rail row, never in the elsewhere line.
      { ...pendingIn(LEDGER, "src/types.ts", 0, 0), capturedAt: ago(0), waiting: true },
    ],
    accepted: [],
    rejected: [],
    nextRecheckAt: null,
  },
  [BILLING]: { root: BILLING, missing: true, scopes: [], pending: [], accepted: [], rejected: [], nextRecheckAt: null },
  [TIDE]: {
    root: TIDE,
    missing: false,
    scopes: [{ root: TIDE, isWorktree: false, shapeUnknown: [], error: null }],
    // Cargo.lock is in the default exclude: it shows only once the project's list drops it.
    pending: [pendingIn(TIDE, "src/ocean.rs", 31, 12), pendingIn(TIDE, "Cargo.lock", 40, 22)],
    accepted: [],
    rejected: [],
    nextRecheckAt: null,
  },
};

/** workspace-roots.json, in order. */
const rootsList = [SHOP, UMBRELLA, NOTES, BILLING, HERDR, TIDE, LAB, LEDGER];

const emptyView = (root: string): ProjectView => ({
  root,
  missing: false,
  scopes: [{ root, isWorktree: false, shapeUnknown: [], error: null }],
  pending: [],
  accepted: [],
  rejected: [],
  nextRecheckAt: null,
});

/** The nesting guard and messages of core/src/roots.rs, close enough for the preview. */
function mockAdd(raw: string): { root: string; already: boolean } {
  const path = raw.trim();
  if (!path.startsWith("/")) throw "Give the full path of the folder, starting from /.";
  const root = path.replace(/\/+$/, "") || "/";
  if (root.endsWith(".txt")) throw "That is not a folder that exists.";
  const inside = (a: string, b: string) => a.startsWith(b + "/");
  if (rootsList.includes(root)) return { root, already: true };
  for (const r of rootsList) {
    if (inside(root, r)) {
      if (root.includes("/.worktrees/"))
        throw `This folder is a git worktree inside ${r}. It needs no project of its own: worktrees inside a project are found automatically.`;
      throw `This folder is inside ${r}, which is already a project. Its edits are captured there.`;
    }
    if (inside(r, root)) throw `This folder contains ${r}, which is already a project. Add a folder that does not overlap it.`;
  }
  rootsList.push(root);
  views[root] ??= emptyView(root);
  config = { ...config, appRoots: [...(config.appRoots ?? []), root] };
  return { root, already: false };
}

/** config.rs `add_to_workspace`: add-only, at the end. `?workspace=empty` starts with none. */
function joinWorkspace(root: string) {
  const ws = config.workspace ?? [];
  if (!ws.includes(root)) config = { ...config, workspace: [...ws, root] };
}

// Per-file before/after text, so the diff preview shows real-looking changes in several languages.
const CLIENT_BEFORE = `import type { Order } from "./types";

const BASE = "/api";

export async function fetchOrders(userId: string) {
  const res = await fetch(\`\${BASE}/orders?user=\${userId}\`);
  return res.json();
}

export async function fetchOrder(id: string) {
  const res = await fetch(\`\${BASE}/orders/\${id}\`);
  return res.json();
}

export function formatTotal(cents: number) {
  return "$" + (cents / 100).toFixed(2);
}
`;
const CLIENT_AFTER = `import type { Order } from "./types";

const BASE = "/api";

export class HttpError extends Error {
  constructor(readonly status: number, what: string) {
    super(\`\${what}: \${status}\`);
  }
}

export async function fetchOrders(userId: string, signal?: AbortSignal): Promise<Order[]> {
  const res = await fetch(\`\${BASE}/orders?user=\${encodeURIComponent(userId)}\`, { signal });
  if (!res.ok) throw new HttpError(res.status, "orders");
  return (await res.json()) as Order[];
}

export async function fetchOrder(id: string, signal?: AbortSignal): Promise<Order> {
  const res = await fetch(\`\${BASE}/orders/\${encodeURIComponent(id)}\`, { signal });
  if (!res.ok) throw new HttpError(res.status, "order");
  return (await res.json()) as Order;
}

export function formatTotal(cents: number, currency = "USD") {
  return new Intl.NumberFormat("en-US", { style: "currency", currency }).format(cents / 100);
}
`;
const SQL = `-- Refunds are recorded against the original order line, never the order.
CREATE TABLE refunds (
  id            BIGSERIAL PRIMARY KEY,
  order_line_id BIGINT      NOT NULL REFERENCES order_lines (id),
  amount_cents  INTEGER     NOT NULL CHECK (amount_cents > 0),
  reason        TEXT        NOT NULL,
  created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX refunds_order_line_idx ON refunds (order_line_id);
`;
const README_BEFORE = `# shop-api

The order and checkout service.

## Running

    npm install
    npm run dev
`;
const README_AFTER = `# shop-api

The order, checkout and refund service.

## Running

    npm install
    docker compose up -d db
    npm run dev

## Refunds

Refunds are stored per order line. See \`src/db/migrations/0007_refunds.sql\`.
`;
const PKG_BEFORE = `{
  "name": "shop-api",
  "version": "2.3.0",
  "scripts": {
    "dev": "tsx watch src/index.ts",
    "test": "vitest run"
  }
}
`;
const PKG_AFTER = PKG_BEFORE.replace('"2.3.0"', '"2.4.0"').replace(
  '"test": "vitest run"',
  '"test": "vitest run",\n    "migrate": "node scripts/migrate.mjs"',
);
const RUST_BEFORE = `pub fn damage(attack: u32, defense: u32, power: u32) -> u32 {
    (attack * power) / defense.max(1)
}
`;
const RUST_AFTER = `/// Gen-V damage formula, without the random roll.
pub fn damage(level: u32, attack: u32, defense: u32, power: u32, stab: bool) -> u32 {
    let base = ((2 * level / 5 + 2) * power * attack / defense.max(1)) / 50 + 2;
    if stab { base * 3 / 2 } else { base }
}
`;

const GO_BEFORE = `package refunds

import "errors"

// Refund returns money for one order line.
func Refund(lineID int64, cents int) error {
	if cents <= 0 {
		return errors.New("amount must be positive")
	}
	return nil
}
`;
const GO_AFTER = `package refunds

import (
	"context"
	"fmt"
)

// Store persists refunds.
type Store interface {
	Insert(ctx context.Context, lineID int64, cents int, reason string) error
}

// Refund returns money for one order line and records why.
func Refund(ctx context.Context, store Store, lineID int64, cents int, reason string) error {
	if cents <= 0 {
		return fmt.Errorf("refund %d: amount must be positive, got %d", lineID, cents)
	}
	return store.Insert(ctx, lineID, cents, reason)
}
`;
const PY_BEFORE = `import json


def seed(path):
    with open(path) as f:
        return json.load(f)
`;
const PY_AFTER = `import json
from pathlib import Path


def seed(path: Path, *, limit: int = 100) -> list[dict]:
    """Load fixture orders, at most \`limit\` of them."""
    with path.open(encoding="utf-8") as f:
        return json.load(f)[:limit]


if __name__ == "__main__":
    print(len(seed(Path("fixtures/orders.json"))))
`;
const YAML_BEFORE = `services:
  api:
    build: .
    ports:
      - "3000:3000"
`;
const YAML_AFTER = `services:
  api:
    build: .
    ports:
      - "3000:3000"
    depends_on: [db]
    environment:
      DATABASE_URL: postgres://shop:shop@db:5432/shop
  db:
    image: postgres:16
    restart: unless-stopped
`;

function sample(path: string, isNew: boolean): { before: string | null; after: string } {
  const name = path.split("/").pop() ?? path;
  if (name.endsWith(".go")) return { before: GO_BEFORE, after: GO_AFTER };
  if (name.endsWith(".py")) return { before: PY_BEFORE, after: PY_AFTER };
  if (name.endsWith(".yml")) return { before: YAML_BEFORE, after: YAML_AFTER };
  if (name.endsWith(".sql")) return { before: isNew ? null : SQL, after: SQL };
  if (name === "README.md") return { before: README_BEFORE, after: README_AFTER };
  if (name === "package.json") return { before: PKG_BEFORE, after: PKG_AFTER };
  if (name.endsWith(".rs")) return { before: RUST_BEFORE, after: RUST_AFTER };
  if (isNew) return { before: null, after: CLIENT_AFTER };
  return { before: CLIENT_BEFORE, after: CLIENT_AFTER };
}

let config: EditorConfig = {
  editor: "zed {path}:{line}:{col}",
  customEditor: "nvim +{line} {path}",
  readOnly: false,
  appRoots: [TIDE],
  // herdr-pokemon, scratch-lab and ledger-sync are known projects outside it.
  workspace: new URLSearchParams(location.search).get("workspace") === "empty" ? [] : [SHOP, UMBRELLA, NOTES, BILLING, TIDE],
};

/** What the extension would supply: a VS Code user setting and a project exclude list. */
const ext: Record<string, Resolved<unknown>> = {
  autoAdvance: { value: true, source: "default" },
  groupBySession: { value: false, source: "default" },
  pendingAgeMinDays: { value: 2, source: "vsCode" },
  maxAttached: { value: 256, source: "default" },
  protected: { value: DEFAULT_PROTECTED, source: "default" },
};
const appSettings: Partial<Record<SettingKey, unknown>> = {};
let hookLog = false;
const extraHide: Record<string, string[]> = { [SHOP]: ["**/*.snap"] };
const DEFAULT_EXCLUDE = ["**/package-lock.json", "**/yarn.lock", "**/Cargo.lock", "**/go.sum", "**/*.min.js", "**/*.map", "**/node_modules/**"];

/** The app's own exclude list of the projects it owns (`projects[root].exclude`). */
const projectExclude: Record<string, string[]> = {};
/** VS Code's own list for an app project it took over. */
const takenOver: Record<string, string[]> = {};

function excludeOwner(root: string | null): ExcludeOwner {
  if (!root || !(config.appRoots ?? []).includes(root)) return "vsCode";
  return root in takenOver ? "managedByVsCode" : "owned";
}

function mockExclude(root: string | null): Resolved<string[]> {
  const owner = excludeOwner(root);
  if (owner === "owned") {
    return root! in projectExclude ? { value: projectExclude[root!], source: "app" } : { value: DEFAULT_EXCLUDE, source: "default" };
  }
  if (owner === "managedByVsCode") return { value: takenOver[root!], source: "vsCode" };
  return root === SHOP
    ? { value: [...DEFAULT_EXCLUDE, "dist/**", "coverage/**"], source: "vsCode" }
    : { value: DEFAULT_EXCLUDE, source: "default" };
}

/** The view as the core filters it: an owned project's own exclude drops rows. */
function filtered(v: ProjectView): ProjectView {
  if (excludeOwner(v.root) !== "owned") return v;
  const out = globMatcher(v.root, mockExclude(v.root).value);
  return { ...v, pending: v.pending.filter((p) => !out(p.path)), accepted: v.accepted.filter((r) => !out(r.path)), rejected: v.rejected.filter((r) => !out(r.path)) };
}

function mockSettings(root: string | null): SettingsView {
  const r = <T>(k: SettingKey) =>
    (k in appSettings ? { value: appSettings[k], source: "app" } : ext[k]) as Resolved<T>;
  return {
    exclude: mockExclude(root),
    protected: r<string[]>("protected"),
    autoAdvance: r<boolean>("autoAdvance"),
    groupBySession: r<boolean>("groupBySession"),
    pendingAgeMinDays: r<number>("pendingAgeMinDays"),
    maxAttached: r<number>("maxAttached"),
    hookLog: { value: hookLog, source: hookLog ? "app" : "default" },
    extraHide: (root && extraHide[root]) || [],
    nestedExtraHide: root && views[root] ? views[root].scopes.filter((s) => s.nested).map((s) => ({ root: s.root, patterns: extraHide[s.root] ?? [] })) : [],
    excludeOwner: excludeOwner(root),
    copyRefPathStyle: appSettings.copyRefPathStyle === "absolute" ? "absolute" : "relative",
    protectedWorkspace: [],
  };
}

const delay = <T>(v: T) => new Promise<T>((r) => setTimeout(() => r(structuredClone(v)), 120));

const handlers = new Set<(p: ChangedPayload) => void>();
/** Dev preview only: how many times `__mock.rewrite` has written each path again. */
const rewrites: Record<string, number> = {};
/** Dev preview only: pending files `__mock.deleteOnDisk` removed. */
const deleted = new Set<string>();

/**
 * Dev preview only: `__mock.drop("client.ts")` removes a pending row and fires the change event,
 * the path a real accept in VS Code takes, so a selected row disappears under the diff pane.
 */
(globalThis as Record<string, unknown>).__mock = {
  /** What the next "Choose folder" returns (null: cancelled). */
  nextPick: "/home/dev/src/kelp-forest" as string | null,
  /** Roots "open in VS Code": Remove only forgets them. */
  inVsCode: [] as string[],
  /** The next decide fails as a store error after "changing" the files. */
  failStore: false,
  /** Marks a project's primary session unreadable, as a permission error would. */
  breakScope(name: string) {
    for (const v of Object.values(views)) {
      if (!v.root.endsWith(`/${name}`) || !v.scopes[0]) continue;
      v.scopes[0] = { ...v.scopes[0], error: "cannot read session file: Permission denied (os error 13)" };
      for (const h of handlers) h({ roots: [v.root] });
    }
  },
  /** A VS Code window writes its own exclude for an app project: the app stops owning it. */
  takeOver(name: string, globs: string[] = ["**/node_modules/**", "target/**"]) {
    const root = rootsList.find((r) => r.endsWith(`/${name}`));
    if (!root) return;
    takenOver[root] = globs;
    for (const h of handlers) h({ roots: [] });
  },
  /** The pending file ending in `suffix` is deleted on disk (Claude ran `rm`). */
  deleteOnDisk(suffix: string) {
    for (const v of Object.values(views)) {
      const hit = v.pending.find((p) => p.path.endsWith(suffix));
      if (!hit) continue;
      deleted.add(hit.path);
      for (const h of handlers) h({ roots: [v.root] });
    }
  },
  /** Claude writes the pending file ending in `suffix` again: one more line at its end. */
  rewrite(suffix: string) {
    for (const v of Object.values(views)) {
      const hit = v.pending.find((p) => p.path.endsWith(suffix));
      if (!hit) continue;
      rewrites[hit.path] = (rewrites[hit.path] ?? 0) + 1;
      for (const h of handlers) h({ roots: [v.root] });
    }
  },
  drop(suffix: string) {
    for (const v of Object.values(views)) {
      const before = v.pending.length;
      v.pending = v.pending.filter((p) => !p.path.endsWith(suffix));
      if (v.pending.length !== before) for (const h of handlers) h({ roots: [v.root] });
    }
  },
};

// ---- Git log -------------------------------------------------------------------------------
// The workspace's repositories as gitlog.rs `gitlog_repos` reports them: one per git common dir,
// a worktree folded into its repository, ahead/behind from the last fetch (null: no upstream).
const head = (n: number) => n.toString(16).padStart(40, "0");
function mockRepo(path: string, color: number, ahead: number | null, behind: number | null, worktrees: { path: string; branch: string | null }[] = []): RepoView {
  return {
    id: `${path}/.git`,
    name: path.split("/").pop()!,
    color,
    checkouts: [
      { path, head: head(color + 1), branch: "main", isMain: true },
      ...worktrees.map((w, i) => ({ path: w.path, head: head(100 + i), branch: w.branch, isMain: false })),
    ],
    ahead,
    behind,
  };
}
const gitRepos: RepoView[] = [
  mockRepo(SHOP, 0, 1, 2, [{ path: WT, branch: "checkout-v2" }]),
  mockRepo(UMBRELLA, 1, 0, 0),
  mockRepo(UMB_NESTED, 2, 0, 5),
  mockRepo(`${UMBRELLA}/umbrella-payments`, 3, 3, 0),
  mockRepo(`${UMBRELLA}/umbrella-web`, 4, null, null, [
    { path: `${UMBRELLA}/umbrella-web/.worktrees/dark-mode`, branch: "dark-mode" },
    { path: `${UMBRELLA}/umbrella-web/.worktrees/hotfix`, branch: null },
  ]),
  mockRepo(`${UMBRELLA}/svc-lib`, 5, 0, 0),
  mockRepo(NOTES, 6, 0, 0),
  mockRepo(BILLING, 7, null, null),
  mockRepo(TIDE, 8, 2, 1),
];
/** `gitLog` in config.json. `?gitlog=profiles` starts with a couple of profiles, one naming a gone repo. */
let gitLog: GitLogConfig = structuredClone(DEFAULT_GITLOG_CONFIG);
if (new URLSearchParams(location.search).get("gitlog") === "profiles") {
  gitLog.profiles = {
    Checkout: [`${SHOP}/.git`, `${UMB_NESTED}/.git`, `${UMBRELLA}/umbrella-payments/.git`],
    "Front end": [`${UMBRELLA}/umbrella-web/.git`, "/home/dev/src/retired-site/.git"],
  };
  gitLog.ticked = gitLog.profiles.Checkout.slice();
}
const gl = () => delay(gitLog);
function glTicked(): string[] {
  return gitLog.ticked ?? gitRepos.map((r) => r.id);
}
function glName(n: string): string {
  const t = n.trim();
  if (!t) throw "A profile needs a name.";
  return t;
}

// The merged log of the mock: a few hundred deterministic commits per repository.
const GL_PAGE = 100;
const GL_AUTHORS = [
  ["Rin", "rin@acme.test"],
  ["Sam", "sam@acme.test"],
  ["Ada", "ada@acme.test"],
] as const;
const GL_SUBJECTS = [
  "fix: keep the retry count across reconnects",
  "feat: add the audit export",
  "refactor: split the order totals out of the handler",
  "chore: bump dependencies",
  "test: cover the empty cart",
  "docs: explain the release steps",
  "fix: stop logging the session token",
  "perf: cache the price lookup",
];
const glSubject = (sha: string) => GL_SUBJECTS[parseInt(sha.slice(-4), 16) % GL_SUBJECTS.length];
let glNewest = 0;
let glQuery: { id: number; rows: RowView[]; at: number; failures: { repo: string; reason: string }[]; branchUse: PageView["branchUse"] } | null = null;
const glAll: RowView[] = (() => {
  const now = Date.now() / 1000;
  const out: RowView[] = [];
  gitRepos.forEach((r, ri) => {
    const wt = r.checkouts.filter((c) => !c.isMain);
    for (let k = 0; k < 400; k++) {
      const sha = ((((ri + 1) * 2_654_435_761 + k * 40_503) >>> 0).toString(16).padStart(8, "0")).repeat(5);
      const [author, email] = GL_AUTHORS[(k + ri) % 3];
      // Every 9th commit of a repository with worktrees lives only on one of them.
      const only = wt.length && k % 9 === 0 ? wt[k % wt.length] : null;
      out.push({
        repo: r.id,
        sha,
        time: Math.floor(now - ri * 600 - k * (1500 + ri * 700)),
        author,
        email,
        subject: GL_SUBJECTS[(k * 3 + ri) % GL_SUBJECTS.length] + (k % 5 === 0 ? ` (#${1000 + k})` : ""),
        parents: [],
        source: only ? (only.branch ? `refs/heads/${only.branch}` : only.head) : "HEAD",
      });
    }
  });
  return out.sort((a, b) => b.time - a.time);
})();
function glStart(id: number, repos: string[], f: FilterView) {
  const now = Date.now() / 1000;
  const since = f.since ?? (f.within ? now - f.within : null);
  const search = f.search?.toLowerCase();
  const authors = (f.authors ?? []).map((a) => a.toLowerCase());
  const branch = f.branch?.trim();
  const has = (id2: string) => !!branch && !!gitRepos.find((r) => r.id === id2)?.checkouts.some((c) => c.branch === branch);
  const rows = glAll
    .filter((r) => repos.includes(r.repo))
    .filter((r) => (since === null ? true : r.time >= since) && (f.until === undefined || r.time <= f.until))
    .filter((r) => !search || r.subject.toLowerCase().includes(search) || r.sha.startsWith(search))
    .filter((r) => !f.mine || r.author === "Rin")
    .filter((r) => !authors.length || authors.some((a) => r.author.toLowerCase().includes(a) || r.email.includes(a)))
    .map((r) => (branch ? { ...r, source: has(r.repo) ? branch : "HEAD" } : r));
  const failing = new URLSearchParams(location.search).get("gitlog") === "fail" ? gitRepos.find((r) => r.name === "legacy-billing") : undefined;
  return {
    id,
    rows: failing ? rows.filter((r) => r.repo !== failing.id) : rows,
    at: 0,
    failures: failing && repos.includes(failing.id) ? [{ repo: failing.id, reason: "fatal: detected dubious ownership in repository" }] : [],
    branchUse: branch ? { branch, found: repos.filter(has).length, fallback: repos.filter((x) => !has(x)).length } : null,
  };
}

// What a commit changed: a deterministic set per commit id, from a one-file edit to a rename with a
// binary, a deleted file and a file too large to show (the five states the pane has to draw).
const ch = (path: string, status: ChangedFile["status"], added: number | null, removed: number | null, from: string | null = null): ChangedFile => ({ path, status, from, added, removed });
const GL_CHANGES: ChangedFile[][] = [
  [ch("internal/refunds/refund.go", "modified", 12, 3), ch("internal/refunds/refund_test.go", "added", 41, 0), ch("README.md", "modified", 2, 1)],
  [
    ch("web/src/client.ts", "renamed", 6, 2, "web/src/api.ts"),
    ch("web/src/components/Cart.ts", "modified", 18, 7),
    ch("web/src/components/Totals.ts", "added", 54, 0),
    ch("web/public/logo.png", "modified", null, null),
    ch("docs/old-notes.md", "deleted", 0, 31),
    ch("package.json", "modified", 1, 1),
  ],
  [ch("service/billing.py", "modified", 5, 5)],
  [ch("db/seed.sql", "modified", 9000, 8800), ch("config/deploy.yml", "modified", 3, 0), ch("scripts/export.rs", "deleted", 0, 12)],
  [],
];
const glFiles = (sha: string) => GL_CHANGES[parseInt(sha.slice(-4), 16) % GL_CHANGES.length];

const gitlogMock: Pick<
  Api,
  | "gitlogRepos"
  | "gitlogConfig"
  | "gitlogTick"
  | "gitlogUntick"
  | "gitlogShowAll"
  | "gitlogShowOnly"
  | "gitlogProfileApply"
  | "gitlogProfileSave"
  | "gitlogProfileRename"
  | "gitlogProfileDelete"
  | "gitlogScanDepth"
  | "gitlogExclude"
  | "gitlogInclude"
  | "gitlogPane"
  | "gitlogLastFilter"
  | "gitlogPage"
  | "gitlogMessage"
  | "gitlogChanges"
  | "gitlogFile"
  | "gitlogWorktreeFile"
  | "gitlogOpenRemote"
> = {
  gitlogRepos: async () => {
    await new Promise((r) => setTimeout(r, 300));
    return structuredClone(gitRepos.filter((r) => !gitLog.excludeRepos.includes(r.name)));
  },
  gitlogConfig: gl,
  gitlogTick: (ids) => {
    if (gitLog.ticked) gitLog = { ...gitLog, ticked: [...gitLog.ticked, ...ids.filter((i) => !gitLog.ticked!.includes(i))] };
    return gl();
  },
  gitlogUntick: (ids) => {
    gitLog = { ...gitLog, ticked: glTicked().filter((i) => !ids.includes(i)) };
    return gl();
  },
  gitlogShowAll: () => {
    gitLog = { ...gitLog, ticked: null };
    return gl();
  },
  gitlogShowOnly: (ids) => {
    gitLog = { ...gitLog, ticked: [...ids] };
    return gl();
  },
  gitlogProfileApply: async (name) => {
    const p = gitLog.profiles[name];
    if (!p) throw `There is no profile named ${name}.`;
    gitLog = { ...gitLog, ticked: [...p] };
    return gl();
  },
  gitlogProfileSave: async (name) => {
    gitLog = { ...gitLog, profiles: { ...gitLog.profiles, [glName(name)]: glTicked() } };
    return gl();
  },
  gitlogProfileRename: async (from, to) => {
    const t = glName(to);
    if (t !== from && gitLog.profiles[t]) throw `A profile named ${t} already exists.`;
    const { [from]: ids, ...rest } = gitLog.profiles;
    if (!ids) throw `There is no profile named ${from}.`;
    gitLog = { ...gitLog, profiles: { ...rest, [t]: ids } };
    return gl();
  },
  gitlogProfileDelete: (name) => {
    const { [name]: _gone, ...rest } = gitLog.profiles;
    gitLog = { ...gitLog, profiles: rest };
    return gl();
  },
  gitlogScanDepth: async (depth) => {
    if (depth < 0 || depth > SCAN_DEPTH_MAX) throw `The scan depth must be 0 to ${SCAN_DEPTH_MAX}.`;
    gitLog = { ...gitLog, scanDepth: depth };
    return gl();
  },
  gitlogExclude: (glob) => {
    if (!gitLog.excludeRepos.includes(glob)) gitLog = { ...gitLog, excludeRepos: [...gitLog.excludeRepos, glob] };
    return gl();
  },
  gitlogInclude: (glob) => {
    gitLog = { ...gitLog, excludeRepos: gitLog.excludeRepos.filter((g) => g !== glob) };
    return gl();
  },
  gitlogPane: (pane, width) => {
    gitLog = { ...gitLog, panes: { ...gitLog.panes, [pane]: Math.round(width) } };
    return gl();
  },
  gitlogLastFilter: (filter) => {
    gitLog = { ...gitLog, lastFilter: filter };
    return gl();
  },
  gitlogPage: async (queryId, repos, filter, more) => {
    await new Promise((r) => setTimeout(r, 120));
    if (queryId < glNewest) throw "stale query";
    if (more) {
      if (!glQuery || glQuery.id !== queryId) throw "stale query";
    } else {
      glNewest = queryId;
      glQuery = glStart(queryId, repos, filter);
    }
    const q = glQuery!;
    const rows = q.rows.slice(q.at, q.at + GL_PAGE);
    q.at += rows.length;
    return { queryId, rows, failures: q.failures, branchUse: q.branchUse, done: q.at >= q.rows.length };
  },
  gitlogMessage: async (_repo, sha) => `${glSubject(sha)}\n\nThe longer explanation of this change goes here, wrapped the way git stores it.\n\nCo-authored-by: Sam <sam@acme.test>`,
  gitlogChanges: async (_repo, sha) => {
    await new Promise((r) => setTimeout(r, 40));
    return structuredClone(glFiles(sha));
  },
  gitlogFile: async (_repo, sha, path, side): Promise<FileView> => {
    await new Promise((r) => setTimeout(r, 40));
    const f = glFiles(sha).find((x) => x.path === path);
    if (!f) throw "Not a path in the repository";
    if (f.added === null) return { kind: "binary" };
    if (path.endsWith(".sql")) return { kind: "tooLarge", value: 3_400_000 };
    if ((side === "before" && f.status === "added") || (side === "after" && f.status === "deleted")) return { kind: "missing" };
    const { before, after } = sample(f.from ?? path, f.status === "added");
    const text = side === "before" ? before : after;
    return text === null ? { kind: "missing" } : { kind: "text", value: text };
  },
  gitlogWorktreeFile: async (repo, path) => {
    if (path.startsWith("docs/old") || path.startsWith("scripts/")) throw `${path} is no longer in the working tree`;
    return `${repo.replace(/\/\.git\/?$/, "")}/${path}`;
  },
  gitlogOpenRemote: async (repo, sha) => {
    if (repo.includes("legacy-billing")) return false;
    console.info("open", `https://github.com/acme/${repo.split("/").slice(-2, -1)[0]}/commit/${sha}`);
    return true;
  },
};

export const mockApi: Api = {
  ...gitlogMock,
  listProjects: () =>
    delay<ProjectSummary[]>(
      rootsList.map((root) => {
        // The sidebar count leaves out what the app-only hide patterns hide, like the tabs do.
        const hidden = globMatcher(root, extraHide[root] ?? []);
        const counted = filtered(views[root]).pending.filter((p) => !hidden(p.path));
        return {
          root,
          name: root.split("/").pop()!,
          missing: views[root].missing,
          pending: counted.length,
          ready: counted.filter((p) => !p.waiting).length,
          nextRecheckAt: null,
          nested: views[root].scopes.filter((s) => s.nested).map((s) => s.root),
        };
      }),
    ),
  getProject: (root) =>
    delay(views[root] ? filtered(views[root]) : { root, missing: true, scopes: [], pending: [], accepted: [], rejected: [], nextRecheckAt: null }),
  getDiff: (_root, _scope, kind: DiffKind, key) => {
    const path = key.includes("::") ? key.split("::")[1] : key;
    if (path.endsWith(".png")) return delay<DiffPayload>({ path, before: { kind: "binary" }, after: { kind: "binary" } });
    const isNew = kind === "pending" && Object.values(views).some((v) => v.pending.some((p) => p.path === key && p.newFile));
    const { before, after } = sample(path, isNew);
    if (kind === "pending" && deleted.has(path)) {
      return delay<DiffPayload>({ path, before: before === null ? { kind: "missing" } : { kind: "text", text: before }, after: { kind: "missing" } });
    }
    return delay<DiffPayload>({
      path,
      before: before === null ? { kind: "missing" } : { kind: "text", text: before },
      after: { kind: "text", text: after + "\n// rewritten\n".repeat(rewrites[path] ?? 0) },
    });
  },
  onChanged: async (handler) => {
    handlers.add(handler);
    return () => void handlers.delete(handler);
  },
  onLaunch: async () => () => {},
  launchPath: async () => [],
  getConfig: () => delay(config),
  setEditor: (template) => {
    config = { ...config, editor: template };
    return delay(config);
  },
  openInEditor: async (path, line, col, template) => {
    const t = template ?? config.editor;
    console.info("[mock] open", t, path, line, col);
    (globalThis as Record<string, unknown>).__lastOpen = { path, line, col, template: t };
    if (t.startsWith("nvim")) {
      throw "Couldn't find nvim (`nvim`). Install its command-line tool, or pick another editor.";
    }
  },
  setPendingCount: async () => {},
  decide: async (root, action, target) => {
    await new Promise((r) => setTimeout(r, 220));
    const r = mockDecide(root, action, target);
    for (const h of handlers) h({ roots: [root] });
    return r;
  },
  setReadOnly: (on) => {
    config = { ...config, readOnly: on };
    return delay(config);
  },
  addToWorkspace: (root) => {
    joinWorkspace(root);
    return delay(config);
  },
  removeFromWorkspace: async (root, nested, stopCapture) => {
    // commands.rs: the Remove flow first; when it fails the workspace is left alone.
    const removed = stopCapture ? await mockApi.removeProject(root) : null;
    const gone = [root, ...nested];
    config = { ...config, workspace: (config.workspace ?? []).filter((r) => !gone.includes(r)) };
    return { config, removed };
  },
  getSettings: (root) => delay(mockSettings(root)),
  setSetting: (key, value, root) => {
    if (value === null) delete appSettings[key];
    else appSettings[key] = value;
    return delay(mockSettings(root));
  },
  setHookLog: (on, root) => {
    if (config.readOnly) throw "Read-only mode is on. Turn it off in Settings to turn the hook log on or off.";
    hookLog = on;
    return delay(mockSettings(root));
  },
  setExtraHide: (root, patterns, viewRoot) => {
    extraHide[root] = patterns;
    return delay(mockSettings(viewRoot ?? root));
  },
  setProjectExclude: async (root, globs) => {
    await delay(null);
    if (config.readOnly) throw "Read-only mode is on. Turn it off in Settings to change a project's exclude list.";
    const owner = excludeOwner(root);
    if (owner === "vsCode")
      throw "This project comes from VS Code, which sets its exclude list. Change it there, or hide more files below.";
    if (owner === "managedByVsCode")
      throw "Managed by VS Code: a VS Code window wrote its own exclude list for this project, so the app no longer changes it.";
    if (globs === null) delete projectExclude[root];
    else {
      const clean = [...new Set(globs.map((g) => g.trim()))];
      if (clean.some((g) => g === "")) throw "A pattern is empty.";
      projectExclude[root] = clean;
    }
    return mockSettings(root);
  },
  takeBackExclude: async (root) => {
    await delay(null);
    if (config.readOnly) throw "Read-only mode is on. Turn it off in Settings to take a project's exclude list back.";
    if (excludeOwner(root) === "vsCode")
      throw "This project comes from VS Code, which sets its exclude list. Change it there, or hide more files below.";
    delete takenOver[root];
    return mockSettings(root);
  },
  addProject: async (path) => {
    await delay(null);
    if (config.readOnly) throw "Read-only mode is on. Turn it off in Settings to add or remove projects.";
    const r = mockAdd(path);
    joinWorkspace(r.root);
    return r;
  },
  removeProject: async (root) => {
    await delay(null);
    if (!(config.appRoots ?? []).includes(root))
      throw "This project comes from VS Code. Remove it there; the app only removes projects it added.";
    if (config.readOnly) throw "Read-only mode is on. Turn it off in Settings to add or remove projects.";
    const vs = mockState().inVsCode.includes(root);
    if (!vs) rootsList.splice(rootsList.indexOf(root), 1);
    config = {
      ...config,
      appRoots: (config.appRoots ?? []).filter((r) => r !== root),
    };
    return { root, unregistered: !vs };
  },
  projectInVsCode: (root) => delay(mockState().inVsCode.includes(root)),
  pickFolder: () => delay(mockState().nextPick),
  captureStatus: (recheck) => {
    // `?capture=fail`: the status call fails, until Check again.
    if (captureFails && !recheck) return Promise.reject("couldn't read ~/.claude/settings.json: permission denied");
    if (captureFails) {
      captureFails = false;
      capture = { ...capture, status: "not-set-up" };
    }
    // "Check again" finds python3 in the preview, so the state can be walked through.
    if (recheck && capture.status === "python-missing") capture = { ...capture, status: "not-set-up" };
    if (recheck) capture = { ...capture, pythonMissing: false };
    return delay({ ...capture });
  },
  setUpCapture: async () => {
    await delay(null);
    if (config.readOnly) throw "Read-only mode is on. Turn it off in Settings to set up capture.";
    if (capture.status !== "not-set-up") throw "Capture is already set up; nothing was changed.";
    if (!capture.setupSupported) throw "Setting up capture is not supported on this platform yet.";
    capture = {
      status: "app",
      version: MOCK_BUNDLED,
      note: null,
      pythonMissing: false,
      takenOver: false,
      setupSupported: true,
      hook: { state: "upToDate", installed: MOCK_BUNDLED, bundled: MOCK_BUNDLED },
    };
    return {
      settings: "/home/dev/.claude/settings.json",
      backup: "/home/dev/.claude/settings.json.claudegate-2026-11-18T10-20-30.123Z.bak",
      created: ["/home/dev/.claudegate/hook.py", "/home/dev/.claudegate/hook.sh"],
      version: "v1.16.1",
      note: new URLSearchParams(location.search).get("captureNote"),
      adopted: false,
      capture: { ...capture },
    };
  },
  updateCaptureHook: async () => {
    await delay(null);
    if (config.readOnly) throw "ClaudeGate Desktop is in read-only mode. Turn it off to update the hook.";
    const from = capture.hook;
    if (!from || (capture.status !== "vscode" && capture.status !== "app"))
      throw "Capture is not set up (no ClaudeGate hook in ~/.claude/settings.json), so hook.py was left alone.";
    if (from.state === "upToDate") throw "hook.py is already the bundled version; nothing was changed.";
    if (from.state === "newer") throw `hook.py is ${from.installed}, newer than the version bundled in this app, so it was left as is.`;
    const appOwnsNow = capture.takenOver;
    if (appOwnsNow) capture = { ...capture, status: "app", version: "v1.16.1", takenOver: false };
    capture = { ...capture, hook: { state: "upToDate", installed: MOCK_BUNDLED, bundled: MOCK_BUNDLED } };
    return {
      hook: "/home/dev/.claudegate/hook.py",
      backup: from.state === "missing" ? null : "/home/dev/.claudegate-desktop/backups/hook/hook-20261126T101530.123Z.py",
      from,
      to: MOCK_BUNDLED,
      appOwnsNow,
      capture: { ...capture },
    };
  },
  // `?update=deb|app`: a newer release is out; otherwise this is the latest.
  checkUpdate: () => {
    const kind = new URLSearchParams(location.search).get("update");
    if (!kind) return delay(null);
    const deb = kind === "deb";
    return delay({
      version: "0.3.0",
      current: "0.2.0",
      notes: "Faster diff loading.",
      canInstall: !deb,
      command: deb
        ? "curl -fL -o ~/Downloads/ClaudeGate_0.3.0_amd64.deb https://github.com/lntvan166/claudegate-desktop/releases/download/v0.3.0/ClaudeGate_0.3.0_amd64.deb && pkexec apt install -y ~/Downloads/ClaudeGate_0.3.0_amd64.deb"
        : null,
    });
  },
  installUpdate: () => delay(undefined),
};

/**
 * The capture state the preview starts in: `?capture=vscode|app|not-set-up|settings-unreadable|python-missing`,
 * plus `&capturePython=missing` (the warning next to a capturing state), `&captureTakenOver=1`,
 * `&unsupported` (core says setup isn't supported on this platform), and
 * `&captureHook=upToDate|older|newer|different|modified|missing`. A capturing preview starts with an outdated hook
 * (the extension's unstamped copy under VS Code, an older stamp under this app), so Update hook… shows.
 */
const MOCK_BUNDLED = "v1.17.0";
function mockHook(status: CaptureView["status"], q: URLSearchParams): HookView | null {
  if (status !== "vscode" && status !== "app") return null;
  const want = q.get("captureHook") ?? (status === "vscode" ? "different" : "older");
  const state = (want === "modified" ? "different" : ["upToDate", "older", "newer", "different", "missing"].includes(want) ? want : "different") as HookView["state"];
  const installed =
    state === "older" ? "v1.16.1" : state === "newer" ? "v1.18.0" : want === "modified" || (state === "upToDate" && status === "app") ? MOCK_BUNDLED : null;
  return { state, installed, bundled: MOCK_BUNDLED };
}
let captureFails = typeof location !== "undefined" && new URLSearchParams(location.search).get("capture") === "fail";
let capture: CaptureView = (() => {
  const q = new URLSearchParams(typeof location === "undefined" ? "" : location.search);
  const s = q.get("capture") ?? "not-set-up";
  const status = (["vscode", "app", "not-set-up", "settings-unreadable", "python-missing"].includes(s) ? s : "not-set-up") as CaptureView["status"];
  return {
    status,
    version: status === "app" ? "v1.16.1" : null,
    note: status === "settings-unreadable" ? "not valid JSON: expected value at line 4 column 3" : null,
    pythonMissing: (status === "vscode" || status === "app") && q.get("capturePython") === "missing",
    takenOver: status === "vscode" && q.get("captureTakenOver") === "1",
    setupSupported: !q.has("unsupported"),
    hook: mockHook(status, q),
  };
})();

function mockState(): { nextPick: string | null; inVsCode: string[] } {
  return (globalThis as Record<string, unknown>).__mock as { nextPick: string | null; inVsCode: string[] };
}

const UNKNOWN = "This project was written by a newer ClaudeGate. Update the app to make decisions here.";
const fail = (kind: string, message: string) => JSON.stringify({ kind, message });

/**
 * An in-memory `decide` over the fixtures, close enough to core/src/decide.rs to exercise every
 * control: bulk and folder actions skip waiting rows; a reject of `scripts/seed.py` leaves it on
 * disk and one of `refund.go` fails, so the bulk summary and its Details have something to show.
 */
function mockDecide(root: string, action: DecideAction, target: DecideTarget): DecideReport {
  const v = views[root];
  if (!v) throw fail("notFound", "nothing to decide: the file or record is no longer there");
  if (config.readOnly) throw fail("readOnly", "ClaudeGate Desktop is in read-only mode. Turn it off to make decisions.");
  if (v.scopes.some((s) => s.shapeUnknown.length > 0)) throw fail("readOnly", UNKNOWN);
  const m = (globalThis as unknown as Record<string, { failStore?: boolean }>).__mock;
  const report: DecideReport = { done: 0, left: [], errors: [] };
  const now = new Date().toISOString();
  // Confirmed keys narrow folder and all targets; one that is no longer there is reported.
  const confirmed = target.kind === "file" ? undefined : target.keys;
  const seen = new Set<string>();
  const take = (key: string) => {
    if (!confirmed) return true;
    if (!confirmed.includes(key)) return false;
    seen.add(key);
    return true;
  };
  const inScope = (scope: string, path: string) => {
    if (target.kind === "all") return true;
    if (target.scope !== scope) return false;
    return target.kind === "folder" ? path.startsWith(`${target.prefix}/`) : true;
  };
  // A store failure leaves the log as it was, so Retry has the same work to do.
  const snapshot = { pending: v.pending, accepted: v.accepted, rejected: v.rejected };
  const changed: string[] = [];
  switch (action) {
    case "accept":
    case "reject": {
      const picked = v.pending.filter(
        (p) => inScope(p.scope, p.path) && (target.kind === "file" ? p.path === target.key : !p.waiting && take(p.path)),
      );
      if (picked.length === 0) throw fail("notFound", "nothing to decide: the file or record is no longer there");
      for (const p of picked) {
        if (action === "reject" && p.path.endsWith("refund.go")) {
          report.errors.push([p.path, "permission denied (os error 13)"]);
          continue;
        }
        if (action === "reject" && p.path.endsWith("seed.py")) report.left.push(p.path);
        v.pending = v.pending.filter((x) => x !== p);
        const rec: RecordItem = {
          id: `${now}::${p.path}`,
          path: p.path,
          rel: p.rel,
          scope: p.scope,
          decidedAt: now,
          added: p.added,
          removed: p.removed,
        };
        if (action === "accept") v.accepted = [rec, ...v.accepted];
        else v.rejected = [rec, ...v.rejected.filter((x) => x.path !== p.path)];
        changed.push(p.path);
        report.done++;
      }
      break;
    }
    case "revert":
    case "reapply": {
      const list = action === "revert" ? v.accepted : v.rejected;
      const picked = list.filter(
        (r) =>
          inScope(r.scope, r.path) &&
          (target.kind !== "file" ? take(action === "revert" ? r.id : r.path) : (action === "revert" ? r.id : r.path) === target.key),
      );
      if (picked.length === 0) throw fail("notFound", "nothing to decide: the file or record is no longer there");
      for (const r of picked) {
        if (action === "reapply" && v.pending.some((p) => p.path === r.path)) {
          report.errors.push([r.path, "Claude has edited this file again — review the new change first"]);
          continue;
        }
        if (action === "revert") v.accepted = v.accepted.filter((x) => x !== r);
        else v.rejected = v.rejected.filter((x) => x !== r);
        if (!v.pending.some((p) => p.path === r.path)) {
          v.pending.push({ path: r.path, rel: r.rel, scope: r.scope, newFile: false, capturedAt: now, added: r.added, removed: r.removed, waiting: false, rejectOutcome: "restore" });
        }
        changed.push(r.path);
        report.done++;
      }
      break;
    }
    case "clearAccepted":
      report.done = v.accepted.length;
      v.accepted = [];
      break;
    case "clearRejected":
      report.done = v.rejected.length;
      v.rejected = [];
      break;
  }
  for (const k of confirmed ?? []) {
    if (!seen.has(k)) report.errors.push([k.includes("::") ? k.slice(k.indexOf("::") + 2) : k, "Changed since you confirmed, so it was left alone"]);
  }
  if (m?.failStore) {
    m.failStore = false;
    Object.assign(v, snapshot);
    throw JSON.stringify({
      kind: "store",
      message: "cannot write the session file: No space left on device (os error 28)",
      changed,
      partial: { done: 0, left: [], errors: [] },
      snapshot: changed.length ? "/home/you/.claudegate-desktop/backups/3f2a9c/files/20261116T120000.000Z" : null,
    });
  }
  return report;
}
