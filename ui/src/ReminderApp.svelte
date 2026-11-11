<script lang="ts">
  // Reminder window: a stack of cards in the bottom-right corner of the screen.
  // The shell owns the queue; this window shows it and reports which button was pressed.
  import { onMount, tick } from "svelte";
  import type { Card } from "./lib/types.ts";
  import { cmd, on, inTauri } from "./lib/backend.ts";
  import { timeOf } from "./lib/dates.ts";
  import Icon from "./components/Icon.svelte";

  let cards = $state<Card[]>([]);
  let snoozeFor = $state<string | null>(null);
  let stack = $state<HTMLElement>();

  async function load() {
    try {
      cards = await cmd<Card[]>("reminder_cards");
    } catch {
      cards = [];
    }
    await tick();
    // The window is only as tall as its cards, so nothing else on screen is covered.
    if (stack) cmd("reminder_resize", { height: Math.ceil(stack.getBoundingClientRect().height) }).catch(() => {});
  }

  onMount(() => {
    load();
    const off = on("reminders-changed", load);
    return () => off.then((fn) => fn());
  });

  async function act(card: Card, action: string, minutes?: number) {
    snoozeFor = null;
    cards = cards.filter((c) => c.id !== card.id);
    try {
      await cmd("reminder_action", { cardId: card.id, action, minutes: minutes ?? null });
    } finally {
      load();
    }
  }

  const kindLabel: Record<Card["kind"], string> = { task: "提醒", missed: "错过的提醒", morning: "晨间简报", evening: "晚间回顾" };
  const snoozeChoices = [
    { label: "10 分钟", minutes: 10 },
    { label: "30 分钟", minutes: 30 },
    { label: "1 小时", minutes: 60 },
    { label: "3 小时", minutes: 180 },
  ];
</script>

<div class="stack" class:preview={!inTauri} bind:this={stack}>
  {#each cards as card (card.id)}
    <article class="card" class:task={card.kind === "task"}>
      <div class="top">
        <span class="kind">
          <Icon name={card.kind === "morning" ? "sun" : card.kind === "evening" ? "moon" : "bell"} size={13} />
          {kindLabel[card.kind]}
        </span>
        <span class="when">{card.subtitle}</span>
        <button class="icon-btn close" title="知道了" onclick={() => act(card, "dismiss")}><Icon name="x" size={13} /></button>
      </div>

      <h2>{card.title}</h2>

      {#if card.kind === "task"}
        {@const task = card.tasks[0]}
        {#if task?.note}<p class="note">{task.note}</p>{/if}
        {#if task?.subtasks?.length}
          <ul>
            {#each task.subtasks.slice(0, 4) as sub (sub.id)}
              <li class:done={sub.status === "done"}>{sub.title}</li>
            {/each}
          </ul>
        {/if}
      {:else if card.tasks.length}
        <ul>
          {#each card.tasks.slice(0, 5) as t (t.id)}
            <li>
              {t.title}{#if t.remind_at}<span class="t">{timeOf(t.remind_at)}</span>{/if}
            </li>
          {/each}
          {#if card.tasks.length > 5}<li class="more">还有 {card.tasks.length - 5} 件…</li>{/if}
        </ul>
      {/if}

      {#if snoozeFor === card.id}
        <div class="actions">
          {#each snoozeChoices as c}
            <button class="btn" onclick={() => act(card, "snooze", c.minutes)}>{c.label}</button>
          {/each}
          <button class="btn" onclick={() => act(card, "tomorrow")}>明天</button>
        </div>
      {:else if card.kind === "task"}
        <div class="actions">
          <button class="btn primary" onclick={() => act(card, "done")}><Icon name="check" size={14} />完成</button>
          <button class="btn" onclick={() => (snoozeFor = card.id)}>稍后</button>
          <button class="btn ghost" onclick={() => act(card, "dismiss")}>知道了</button>
        </div>
      {:else if card.kind === "evening"}
        <div class="actions">
          <button class="btn primary" onclick={() => act(card, "tomorrow")}>都挪到明天</button>
          <button class="btn" onclick={() => act(card, "open")}>打开看看</button>
          <button class="btn ghost" onclick={() => act(card, "dismiss")}>知道了</button>
        </div>
      {:else}
        <div class="actions">
          <button class="btn primary" onclick={() => act(card, "open")}>打开看看</button>
          <button class="btn ghost" onclick={() => act(card, "dismiss")}>知道了</button>
        </div>
      {/if}
    </article>
  {/each}
</div>

<style>
  .stack {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 12px;
  }
  .stack.preview {
    width: 384px;
  }
  .card {
    padding: 13px 14px 14px 16px;
    border-radius: var(--r-lg);
    background: var(--card);
    border: 1px solid var(--line-strong);
    box-shadow: 0 10px 30px rgba(0, 0, 0, 0.45);
    animation: slide 0.28s var(--ease);
  }
  .card.task {
    border-left: 3px solid var(--accent);
  }
  @keyframes slide {
    from {
      opacity: 0;
      transform: translateX(24px);
    }
  }
  .top {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 22px;
  }
  .kind {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-size: 11px;
    letter-spacing: 0.06em;
    color: var(--accent);
  }
  .when {
    flex: 1;
    font-size: 11px;
    color: var(--text-3);
    font-variant-numeric: tabular-nums;
  }
  .close {
    width: 22px;
    height: 22px;
    margin-right: -4px;
  }
  h2 {
    margin: 6px 0 0;
    font-family: var(--font-display);
    font-size: 15.5px;
    font-weight: 600;
    line-height: 1.35;
    overflow-wrap: anywhere;
  }
  .note {
    margin: 5px 0 0;
    font-size: 12px;
    color: var(--text-2);
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    white-space: pre-line;
  }
  ul {
    list-style: none;
    margin: 8px 0 0;
    padding: 0;
    font-size: 12.5px;
    color: var(--text-2);
  }
  li {
    position: relative;
    padding: 2px 0 2px 14px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  li::before {
    content: "";
    position: absolute;
    left: 2px;
    top: 10px;
    width: 4px;
    height: 4px;
    border-radius: 50%;
    background: var(--text-3);
  }
  li.done {
    color: var(--text-3);
    text-decoration: line-through;
  }
  li.more {
    color: var(--text-3);
  }
  li.more::before {
    display: none;
  }
  .t {
    margin-left: 8px;
    font-size: 11px;
    color: var(--text-3);
    font-variant-numeric: tabular-nums;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-top: 12px;
  }
</style>
