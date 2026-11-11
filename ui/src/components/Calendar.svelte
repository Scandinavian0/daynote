<script lang="ts">
  // Month grid for the expanded view. A dot under a day shows whether its tasks are finished.
  import { store } from "../lib/store.svelte.ts";
  import { addMonths, monthGrid, monthOf } from "../lib/dates.ts";
  import Icon from "./Icon.svelte";

  const cells = $derived(monthGrid(store.calMonth));
  const label = $derived(`${Number(store.calMonth.slice(0, 4))}年${Number(store.calMonth.slice(5))}月`);

  function state(date: string): "" | "done" | "late" | "open" {
    const d = store.days[date];
    if (!d || d.total === 0) return "";
    if (d.todo === 0) return "done";
    return date < store.today ? "late" : "open";
  }
</script>

<div class="cal">
  <div class="head">
    <span class="month">{label}</span>
    <button class="icon-btn" title="上个月" onclick={() => store.setMonth(addMonths(store.calMonth, -1))}>
      <Icon name="chevron-left" size={15} />
    </button>
    <button class="icon-btn" title="下个月" onclick={() => store.setMonth(addMonths(store.calMonth, 1))}>
      <Icon name="chevron-right" size={15} />
    </button>
  </div>

  <div class="grid week">
    {#each ["一", "二", "三", "四", "五", "六", "日"] as w}<span>{w}</span>{/each}
  </div>

  <div class="grid">
    {#each cells as date (date)}
      {@const s = state(date)}
      <button
        class="day num"
        class:out={monthOf(date) !== store.calMonth}
        class:today={date === store.today}
        class:selected={date === store.date}
        aria-label={date}
        aria-current={date === store.date ? "date" : undefined}
        onclick={() => store.go(date)}
      >
        {Number(date.slice(8))}
        {#if s}<i class={s}></i>{/if}
      </button>
    {/each}
  </div>

  <div class="legend">
    <span><i class="done"></i>全部完成</span>
    <span><i class="open"></i>待办</span>
    <span><i class="late"></i>遗留</span>
  </div>
</div>

<style>
  .head {
    display: flex;
    align-items: center;
    gap: 2px;
    margin-bottom: 8px;
  }
  .month {
    flex: 1;
    font-family: var(--font-display);
    font-size: 14px;
    font-weight: 600;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(7, 1fr);
    gap: 2px;
  }
  .week {
    margin-bottom: 4px;
  }
  .week span {
    text-align: center;
    font-size: 10.5px;
    color: var(--text-3);
  }
  .day {
    position: relative;
    aspect-ratio: 1;
    border-radius: 8px;
    font-size: 12px;
    color: var(--text-2);
    transition: background 0.12s, color 0.12s;
  }
  .day:hover {
    background: var(--fill-2);
    color: var(--text);
  }
  .day.out {
    color: var(--text-3);
    opacity: 0.55;
  }
  .day.today {
    color: var(--accent);
    font-weight: 600;
    box-shadow: inset 0 0 0 1px rgba(143, 176, 255, 0.45);
  }
  .day.selected {
    background: var(--accent);
    color: var(--accent-ink);
    font-weight: 600;
    box-shadow: none;
  }
  i {
    display: inline-block;
    width: 4px;
    height: 4px;
    border-radius: 50%;
  }
  .day i {
    position: absolute;
    left: 50%;
    bottom: 4px;
    margin-left: -2px;
  }
  i.done {
    background: var(--accent);
  }
  i.open {
    background: rgba(255, 255, 255, 0.5);
  }
  i.late {
    background: var(--amber);
  }
  .day.selected i {
    background: var(--accent-ink);
  }
  .legend {
    display: flex;
    gap: 12px;
    margin-top: 10px;
    font-size: 10.5px;
    color: var(--text-3);
  }
  .legend span {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }
</style>
