// Dev-only fixtures for previewing the UI in a plain browser (`npm run dev`, no Tauri).
// Loaded through a DEV-guarded dynamic import in api.ts, so it never ships.
import type {
  Api,
  ChangedPayload,
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

const ago = (min: number) => new Date(Date.now() - min * 60_000).toISOString();

function pending(scope: string, rel: string, added: number, removed: number, newFile = false): PendingItem {
  const path = `${scope}/${rel}`;
  return { path, rel: path.slice(SHOP.length + 1), scope, newFile, capturedAt: ago(added), added, removed, waiting: false };
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

function sample(path: string, isNew: boolean): { before: string | null; after: string } {
  const name = path.split("/").pop() ?? path;
  if (name.endsWith(".sql")) return { before: isNew ? null : SQL, after: SQL };
  if (name === "README.md") return { before: README_BEFORE, after: README_AFTER };
  if (name === "package.json") return { before: PKG_BEFORE, after: PKG_AFTER };
  if (name.endsWith(".rs")) return { before: RUST_BEFORE, after: RUST_AFTER };
  if (isNew) return { before: null, after: CLIENT_AFTER };
  return { before: CLIENT_BEFORE, after: CLIENT_AFTER };
}

let config: EditorConfig = { editor: "zed {path}:{line}", customEditor: "nvim +{line} {path}" };

const delay = <T>(v: T) => new Promise<T>((r) => setTimeout(() => r(structuredClone(v)), 120));

const handlers = new Set<(p: ChangedPayload) => void>();

/**
 * Dev preview only: `__mock.drop("client.ts")` removes a pending row and fires the change event,
 * the path a real accept in VS Code takes, so a selected row disappears under the diff pane.
 */
(globalThis as Record<string, unknown>).__mock = {
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
      [SHOP, BILLING, HERDR].map((root) => ({
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
};
