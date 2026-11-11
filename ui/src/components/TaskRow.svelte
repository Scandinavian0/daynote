<script lang="ts">
  // One task: the row itself, its sub-tasks, and an inline editor that opens beneath it.
  import type { Task } from "../lib/types.ts";
  import { store } from "../lib/store.svelte.ts";
  import { addDays, relativeDay, timeOf } from "../lib/dates.ts";
  import Check from "./Check.svelte";
  import Icon from "./Icon.svelte";

  let { task, ongrip }: { task: Task; ongrip?: (e: PointerEvent, task: Task) => void } = $props();

  const subs = $derived(task.subtasks ?? []);
  const subsDone = $derived(subs.filter((s) => s.status === "done").length);
  const isOpen = $derived(store.openId === task.id);
  const done = $derived(task.status === "done");
  const byAi = $derived(!["user", "cli", "http", ""].includes(task.source));
  // A reminder still to come is highlighted; one that has fired (or passed) goes quiet.
  const pending = $derived(!!task.remind_at && !done && !task.reminded_at);

  let folded = $state(false);
  let subDraft = $state("");

  function toggleOpen() {
    store.openId = isOpen ? null : task.id;
  }

  function commitTitle(e: Event, id: number, previous: string) {
    const input = e.currentTarget as HTMLInputElement;
    const title = input.value.trim();
    if (!title) input.value = previous;
    else if (title !== previous) store.update(id, { title });
  }

  function setTime(e: Event) {
    const value = (e.currentTarget as HTMLInputElement).value;
    store.update(task.id, { remind_at: value ? `${task.date}T${value}:00` : null });
  }

  function addSub(e: KeyboardEvent) {
    if (e.key !== "Enter" || e.isComposing) return;
    const title = subDraft.trim();
    if (!title) return;
    subDraft = "";
    store.addSubtask(task.id, title);
  }

  function blurOnEnter(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.isComposing) (e.currentTarget as HTMLElement).blur();
    if (e.key === "Escape") {
      e.stopPropagation();
      (e.currentTarget as HTMLElement).blur();
    }
  }
</script>

<div class="task" class:done class:open={isOpen} data-task-id={task.id}>
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="row" onclick={toggleOpen}>
    {#if ongrip && !done}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <span class="grip" onpointerdown={(e) => ongrip?.(e, task)} onclick={(e) => e.stopPropagation()}>
        <Icon name="grip" size={14} />
      </span>
    {/if}
    <Check checked={done} label={done ? "标为未完成" : "标为完成"} onchange={() => store.toggle(task)} />

    <div class="body">
      {#if isOpen}
        <!-- svelte-ignore a11y_autofocus -->
        <input
          class="title-input"
          value={task.title}
          aria-label="任务标题"
          onclick={(e) => e.stopPropagation()}
          onblur={(e) => commitTitle(e, task.id, task.title)}
          onkeydown={blurOnEnter}
        />
      {:else}
        <span class="title">
          {#if task.priority > 0}<span class="flag" title="重要"></span>{/if}{task.title}
        </span>
      {/if}
    </div>

    <div class="meta">
      {#if subs.length}
        <button
          class="count num"
          title={folded ? "展开步骤" : "收起步骤"}
          onclick={(e) => {
            e.stopPropagation();
            folded = !folded;
          }}
        >
          {subsDone}/{subs.length}
        </button>
      {/if}
      {#if task.series_id}<span class="glyph" title="重复任务"><Icon name="repeat" size={12} /></span>{/if}
      {#if byAi}<span class="ai" title={`由 ${task.source} 写入`}>AI</span>{/if}
      {#if task.remind_at}
        <span class="time num" class:pending title="提醒时间">{timeOf(task.remind_at)}</span>
      {/if}
    </div>
  </div>

  {#if !isOpen && task.note}
    <p class="note-preview">{task.note}</p>
  {/if}

  {#if subs.length && !folded}
    <ul class="subs">
      {#each subs as sub (sub.id)}
        <li class:done={sub.status === "done"}>
          <Check
            small
            checked={sub.status === "done"}
            label={sub.status === "done" ? "标为未完成" : "标为完成"}
            onchange={() => store.toggle(sub)}
          />
          {#if isOpen}
            <input
              class="sub-input"
              value={sub.title}
              aria-label="步骤"
              onblur={(e) => commitTitle(e, sub.id, sub.title)}
              onkeydown={blurOnEnter}
            />
            <button class="sub-del" title="删除步骤" onclick={() => store.remove(sub)}><Icon name="x" size={12} /></button>
          {:else}
            <span class="sub-title">{sub.title}</span>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}

  {#if isOpen}
    <div class="editor">
      <input
        class="sub-add"
        placeholder="＋ 添加步骤，回车保存"
        bind:value={subDraft}
        onkeydown={addSub}
        aria-label="添加步骤"
      />
      <textarea
        class="note"
        rows="2"
        placeholder="备注…"
        value={task.note ?? ""}
        aria-label="备注"
        onblur={(e) => {
          const note = e.currentTarget.value;
          if (note !== (task.note ?? "")) store.update(task.id, { note: note || null });
        }}
      ></textarea>

      <div class="controls">
        <label class="chip time-chip" class:on={!!task.remind_at} title="提醒时间">
          <Icon name="bell" size={13} />
          <input type="time" value={timeOf(task.remind_at)} onchange={setTime} aria-label="提醒时间" />
        </label>
        <button
          class="chip amber"
          class:on={task.priority > 0}
          onclick={() => store.update(task.id, { priority: task.priority > 0 ? 0 : 1 })}
        >
          <Icon name="flag" size={13} />重要
        </button>
        <span class="spacer"></span>
        <button class="chip danger" title="删除" onclick={() => store.remove(task)}><Icon name="trash" size={13} /></button>
      </div>

      <div class="controls">
        <span class="label">挪到</span>
        {#each [0, 1] as offset}
          {@const target = addDays(store.today, offset)}
          {#if target !== task.date}
            <button class="chip" onclick={() => store.move([task.id], target)}>{relativeDay(target, store.today)}</button>
          {/if}
        {/each}
        <label class="chip date-chip">
          <input
            type="date"
            value={task.date}
            aria-label="改日期"
            onchange={(e) => e.currentTarget.value && store.move([task.id], e.currentTarget.value)}
          />
        </label>
      </div>
    </div>
  {/if}
</div>

<style>
  .task {
    border-radius: var(--r-md);
    transition: background 0.15s;
  }
  .task.open {
    background: var(--fill-1);
    box-shadow: inset 0 0 0 1px var(--line);
    margin: 2px 0;
  }

  .row {
    position: relative;
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 8px 10px;
    border-radius: var(--r-md);
    transition: background 0.15s;
  }
  .task:not(.open) > .row:hover {
    background: var(--fill-1);
  }
  .row :global(.check) {
    margin-top: 1px;
  }

  .grip {
    position: absolute;
    left: -5px;
    top: 9px;
    color: var(--text-3);
    opacity: 0;
    cursor: grab;
    transition: opacity 0.15s;
    touch-action: none;
  }
  .row:hover .grip {
    opacity: 1;
  }

  .body {
    flex: 1;
    min-width: 0;
  }
  .title {
    display: block;
    overflow-wrap: anywhere;
    transition: color 0.2s;
  }
  .flag {
    display: inline-block;
    width: 6px;
    height: 6px;
    margin-right: 7px;
    border-radius: 50%;
    background: var(--amber);
    vertical-align: 2px;
  }
  .done .title {
    color: var(--text-3);
    text-decoration: line-through;
    text-decoration-color: rgba(255, 255, 255, 0.22);
  }
  .title-input {
    width: 100%;
    padding: 0;
    font-weight: 500;
  }

  .meta {
    display: flex;
    align-items: center;
    gap: 7px;
    flex: none;
    height: 20px;
    color: var(--text-3);
    font-size: 11.5px;
  }
  .count {
    color: var(--text-3);
    font-size: 11.5px;
    padding: 0 2px;
    border-radius: 4px;
  }
  .count:hover {
    color: var(--text);
  }
  .glyph {
    display: grid;
  }
  .ai {
    font-size: 9.5px;
    font-weight: 600;
    letter-spacing: 0.06em;
    padding: 0 5px;
    line-height: 15px;
    border-radius: 4px;
    color: var(--accent);
    background: var(--accent-soft);
  }
  .time.pending {
    color: var(--accent);
  }
  .done .meta {
    opacity: 0.6;
  }

  .note-preview {
    margin: -5px 10px 6px 38px;
    font-size: 12px;
    color: var(--text-3);
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    white-space: pre-line;
  }

  .subs {
    list-style: none;
    margin: -2px 0 4px;
    padding: 0 10px 0 38px;
  }
  .subs li {
    display: flex;
    align-items: center;
    gap: 9px;
    min-height: 26px;
    font-size: 12.5px;
    color: var(--text-2);
  }
  .subs li.done .sub-title,
  .subs li.done .sub-input {
    color: var(--text-3);
    text-decoration: line-through;
    text-decoration-color: rgba(255, 255, 255, 0.2);
  }
  .sub-title {
    overflow-wrap: anywhere;
  }
  .sub-input {
    flex: 1;
    min-width: 0;
    padding: 0;
  }
  .sub-del {
    color: var(--text-3);
    opacity: 0;
    padding: 3px;
    border-radius: 4px;
  }
  .subs li:hover .sub-del {
    opacity: 1;
  }
  .sub-del:hover {
    color: var(--danger);
  }

  .editor {
    padding: 0 10px 10px 38px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    animation: reveal 0.18s var(--ease);
  }
  @keyframes reveal {
    from {
      opacity: 0;
      transform: translateY(-3px);
    }
  }
  .sub-add {
    font-size: 12.5px;
    padding: 2px 0;
  }
  .note {
    width: 100%;
    padding: 7px 9px;
    font-size: 12.5px;
    line-height: 1.5;
    color: var(--text-2);
    border-radius: var(--r-sm);
    background: rgba(0, 0, 0, 0.18);
    border: 1px solid transparent;
    field-sizing: content;
    min-height: 34px;
    max-height: 120px;
  }
  .note:focus {
    border-color: var(--line-strong);
    color: var(--text);
  }

  .controls {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 6px;
  }
  .spacer {
    flex: 1;
  }
  .time-chip input {
    width: 68px;
    padding: 0;
  }
  .date-chip input {
    width: 106px;
    padding: 0;
    font-size: 12px;
    color: var(--text-2);
  }
</style>
