<script lang="ts">
  import type { DiffKind } from "../lib/api";

  let {
    tab,
    counts,
    mode,
    ontab,
    onmode,
  }: {
    tab: DiffKind;
    counts: Record<DiffKind, number>;
    mode: "tree" | "list";
    ontab: (t: DiffKind) => void;
    onmode: (m: "tree" | "list") => void;
  } = $props();

  const TABS: { id: DiffKind; label: string }[] = [
    { id: "pending", label: "Pending" },
    { id: "accepted", label: "Accepted" },
    { id: "rejected", label: "Rejected" },
  ];

  function onkey(e: KeyboardEvent) {
    if (e.key !== "ArrowRight" && e.key !== "ArrowLeft") return;
    const i = TABS.findIndex((t) => t.id === tab);
    const next = TABS[(i + (e.key === "ArrowRight" ? 1 : TABS.length - 1)) % TABS.length];
    ontab(next.id);
    e.preventDefault();
  }
</script>

<div class="bar">
  <div class="tabs" role="tablist" aria-label="Review state" tabindex="-1" onkeydown={onkey}>
    {#each TABS as t (t.id)}
      <button
        role="tab"
        class="tab {t.id}"
        class:active={tab === t.id}
        aria-selected={tab === t.id}
        tabindex={tab === t.id ? 0 : -1}
        onclick={() => ontab(t.id)}
      >
        {t.label}
        <span class="count num" class:zero={counts[t.id] === 0}>{counts[t.id]}</span>
      </button>
    {/each}
  </div>

  <div class="mode" role="group" aria-label="Layout">
    <button class:on={mode === "tree"} aria-pressed={mode === "tree"} title="Tree" onclick={() => onmode("tree")}>
      <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
        <path d="M3 3h4M6 8h7M6 13h7M4 4v8.5c0 .3.2.5.5.5H6M4 8h2" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" />
      </svg>
    </button>
    <button class:on={mode === "list"} aria-pressed={mode === "list"} title="Flat list" onclick={() => onmode("list")}>
      <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
        <path d="M3 4h10M3 8h10M3 12h10" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" />
      </svg>
    </button>
  </div>
</div>

<style>
  .bar {
    display: flex;
    align-items: stretch;
    justify-content: space-between;
    height: 40px;
    padding: 0 12px 0 10px;
    border-bottom: 1px solid var(--rule);
    flex: none;
  }

  .tabs {
    display: flex;
  }

  .tab {
    position: relative;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 0 9px;
    font-size: var(--fs-body);
    font-weight: 500;
    color: var(--muted);
    transition: color 120ms var(--ease);
  }
  .tab:hover,
  .tab.active {
    color: var(--text);
  }
  .tab.active {
    font-weight: 600;
  }
  .tab::after {
    content: "";
    position: absolute;
    left: 9px;
    right: 9px;
    bottom: -1px;
    height: 2px;
    background: transparent;
  }
  .tab.active::after {
    background: var(--text);
  }

  .count {
    font-size: var(--fs-small);
    font-weight: 500;
    color: var(--muted);
  }
  .tab.pending .count:not(.zero) {
    color: var(--pending);
  }
  .count.zero {
    color: var(--faint);
    font-weight: 400;
  }

  .mode {
    display: flex;
    align-items: center;
    gap: 2px;
  }
  .mode button {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    border-radius: var(--radius-control);
    color: var(--faint);
    transition:
      color 120ms var(--ease),
      background 120ms var(--ease);
  }
  .mode button:hover {
    color: var(--text);
    background: var(--hover);
  }
  .mode button.on {
    color: var(--text);
    background: var(--active);
  }
</style>
