// Re-fetch a project when a "waiting for write" row may settle. The hook records a capture
// before Claude writes, so a fresh entry can show no change yet; core keeps it visible for its
// settle window and reports the earliest deadline as `nextRecheckAt`. No file event marks that
// deadline passing, so the UI asks again then.

/** Past the deadline, so core's strict `age > window` comparison has flipped. */
export const RECHECK_SLACK_MS = 250;

export function recheckDelay(at: number | null | undefined, now: number = Date.now()): number | null {
  if (at == null) return null;
  return Math.max(0, at - now) + RECHECK_SLACK_MS;
}

/** One pending timer per project root; scheduling again replaces it, `null` cancels it. */
export class Rechecks {
  private timers = new Map<string, ReturnType<typeof setTimeout>>();

  constructor(private fire: (root: string) => void) {}

  schedule(root: string, at: number | null | undefined, now: number = Date.now()) {
    const old = this.timers.get(root);
    if (old !== undefined) clearTimeout(old);
    this.timers.delete(root);
    const delay = recheckDelay(at, now);
    if (delay === null) return;
    this.timers.set(
      root,
      setTimeout(() => {
        this.timers.delete(root);
        this.fire(root);
      }, delay),
    );
  }

  clear() {
    for (const t of this.timers.values()) clearTimeout(t);
    this.timers.clear();
  }
}
