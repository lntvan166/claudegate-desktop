// Dev-only fixtures for previewing the UI in a plain browser (`npm run dev`, no Tauri).
// Loaded through a DEV-guarded dynamic import in api.ts, so it never ships.
import type { Api, DiffKind, DiffPayload, PendingItem, ProjectSummary, ProjectView, RecordItem } from "./api";

const SHOP = "/home/dev/src/shop-api";
const WT = `${SHOP}/.worktrees/checkout-v2`;
const HERDR = "/home/dev/src/herdr-pokemon";
const BILLING = "/home/dev/src/legacy-billing";

const ago = (min: number) => new Date(Date.now() - min * 60_000).toISOString();

function pending(scope: string, rel: string, added: number, removed: number, newFile = false): PendingItem {
  const path = `${scope}/${rel}`;
  return { path, rel: path.slice(SHOP.length + 1), scope, newFile, capturedAt: ago(added), added, removed };
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
  },
  [BILLING]: { root: BILLING, missing: true, scopes: [], pending: [], accepted: [], rejected: [] },
};

const BEFORE = `export async function fetchOrders(userId: string) {
  const res = await fetch(\`/api/orders?user=\${userId}\`);
  return res.json();
}
`;
const AFTER = `export async function fetchOrders(userId: string, signal?: AbortSignal) {
  const res = await fetch(\`/api/orders?user=\${encodeURIComponent(userId)}\`, { signal });
  if (!res.ok) throw new Error(\`orders: \${res.status}\`);
  return (await res.json()) as Order[];
}
`;

const delay = <T>(v: T) => new Promise<T>((r) => setTimeout(() => r(structuredClone(v)), 120));

export const mockApi: Api = {
  listProjects: () =>
    delay<ProjectSummary[]>(
      [SHOP, BILLING, HERDR].map((root) => ({
        root,
        name: root.split("/").pop()!,
        missing: views[root].missing,
        pending: views[root].pending.length,
      })),
    ),
  getProject: (root) =>
    delay(views[root] ?? { root, missing: true, scopes: [], pending: [], accepted: [], rejected: [] }),
  getDiff: (_root, _scope, kind: DiffKind, key) => {
    const isNew = kind === "pending" && Object.values(views).some((v) => v.pending.some((p) => p.path === key && p.newFile));
    return delay<DiffPayload>({
      path: key.includes("::") ? key.split("::")[1] : key,
      before: isNew ? { kind: "missing" } : { kind: "text", text: BEFORE },
      after: { kind: "text", text: AFTER },
    });
  },
  onChanged: async () => () => {},
};
