// Dev-only fixtures for previewing the UI in a plain browser (`npm run dev`, no Tauri).
// Loaded through a DEV-guarded dynamic import in api.ts, so it never ships.
import type {
  Api,
  ChangedPayload,
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
} from "./api";

const SHOP = "/home/dev/src/shop-api";
const WT = `${SHOP}/.worktrees/checkout-v2`;
const HERDR = "/home/dev/src/herdr-pokemon";
const BILLING = "/home/dev/src/legacy-billing";
const NOTES = "/home/dev/src/field-notes";

const ago = (min: number) => new Date(Date.now() - min * 60_000).toISOString();

function pending(scope: string, rel: string, added: number, removed: number, newFile = false): PendingItem {
  const path = `${scope}/${rel}`;
  // scripts/seed.py has a null baseline from an older hook: a reject leaves it on disk.
  const rejectOutcome = newFile ? "delete" : rel.endsWith("seed.py") ? "leave" : "restore";
  return { path, rel: path.slice(SHOP.length + 1), scope, newFile, capturedAt: ago(added), added, removed, waiting: false, rejectOutcome };
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
  [BILLING]: { root: BILLING, missing: true, scopes: [], pending: [], accepted: [], rejected: [], nextRecheckAt: null },
};

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

let config: EditorConfig = { editor: "zed {path}:{line}", customEditor: "nvim +{line} {path}", readOnly: false };

const delay = <T>(v: T) => new Promise<T>((r) => setTimeout(() => r(structuredClone(v)), 120));

const handlers = new Set<(p: ChangedPayload) => void>();

/**
 * Dev preview only: `__mock.drop("client.ts")` removes a pending row and fires the change event,
 * the path a real accept in VS Code takes, so a selected row disappears under the diff pane.
 */
(globalThis as Record<string, unknown>).__mock = {
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
  drop(suffix: string) {
    for (const v of Object.values(views)) {
      const before = v.pending.length;
      v.pending = v.pending.filter((p) => !p.path.endsWith(suffix));
      if (v.pending.length !== before) for (const h of handlers) h({ roots: [v.root] });
    }
  },
};

export const mockApi: Api = {
  listProjects: () =>
    delay<ProjectSummary[]>(
      [SHOP, NOTES, BILLING, HERDR].map((root) => ({
        root,
        name: root.split("/").pop()!,
        missing: views[root].missing,
        pending: views[root].pending.length,
        nextRecheckAt: null,
      })),
    ),
  getProject: (root) =>
    delay(views[root] ?? { root, missing: true, scopes: [], pending: [], accepted: [], rejected: [], nextRecheckAt: null }),
  getDiff: (_root, _scope, kind: DiffKind, key) => {
    const path = key.includes("::") ? key.split("::")[1] : key;
    if (path.endsWith(".png")) return delay<DiffPayload>({ path, before: { kind: "binary" }, after: { kind: "binary" } });
    const isNew = kind === "pending" && Object.values(views).some((v) => v.pending.some((p) => p.path === key && p.newFile));
    const { before, after } = sample(path, isNew);
    return delay<DiffPayload>({
      path,
      before: before === null ? { kind: "missing" } : { kind: "text", text: before },
      after: { kind: "text", text: after },
    });
  },
  onChanged: async (handler) => {
    handlers.add(handler);
    return () => void handlers.delete(handler);
  },
  getConfig: () => delay(config),
  setEditor: (template) => {
    config = { ...config, editor: template };
    return delay(config);
  },
  openInEditor: async (path, line, template) => {
    const t = template ?? config.editor;
    console.info("[mock] open", t, path, line);
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
};

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
