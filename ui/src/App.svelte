<script lang="ts">
  // Main window. Compact: one column (the sticky-note panel).
  // Expanded: calendar on the left, the day in the middle, the day's note on the right.
  import { onMount } from "svelte";
  import type { Task } from "./lib/types.ts";
  import { store } from "./lib/store.svelte.ts";
  import { inTauri } from "./lib/backend.ts";
  import { monthDay, relativeDay, weekday } from "./lib/dates.ts";
  import { parseQuick } from "./lib/quickparse.ts";
  import Calendar from "./components/Calendar.svelte";
  import Icon from "./components/Icon.svelte";
  import Settings from "./components/Settings.svelte";
  import TaskRow from "./components/TaskRow.svelte";

  onMount(() => {
    store.init();
    const flush = () => store.flushNote();
    window.addEventListener("blur", flush);
    return () => window.removeEventListener("blur", flush);
  });

  const app = $derived(store.app);
  const total = $derived(store.tasks.length);
  const doneCount = $derived(store.done.length);
  const dayName = $derived(relativeDay(store.date, store.today));
  const named = $derived(dayName !== monthDay(store.date));

  // Add bar ---------------------------------------------------------------
  let draft = $state("");
  const hint = $derived(draft.trim() ? parseQuick(draft, store.date, new Date()).hint : "");

  async function submit(e: KeyboardEvent) {
    if (e.key !== "Enter" || e.isComposing) return;
    const text = draft;
    if (!text.trim()) return;
    draft = "";
    if (!(await store.add(text))) draft = text;
  }

  // Sections ----------------------------------------------------------------
  let showDone = $state(true);
  let showLate = $state(true);
  let noteOpen = $state(false);

  // Drag to reorder (open tasks only) ---------------------------------------
  let dragId = $state<number | null>(null);
  let dragOrder = $state<number[] | null>(null);
  let listEl = $state<HTMLElement>();

  const openTasks = $derived.by(() => {
    if (!dragOrder) return store.open;
    const byId = new Map(store.open.map((t) => [t.id, t]));
    return dragOrder.map((id) => byId.get(id)).filter((t): t is Task => !!t);
  });

  function startDrag(e: PointerEvent, task: Task) {
    if (e.button !== 0) return;
    e.preventDefault();
    store.openId = null;
    dragId = task.id;
    dragOrder = store.open.map((t) => t.id);

    const move = (ev: PointerEvent) => {
      if (!listEl || !dragOrder) return;
      const rows = [...listEl.querySelectorAll<HTMLElement>(":scope > [data-task-id]")];
      const others = rows.filter((r) => Number(r.dataset.taskId) !== task.id);
      const index = others.filter((r) => {
        const box = r.getBoundingClientRect();
        return box.top + box.height / 2 < ev.clientY;
      }).length;
      const next = dragOrder.filter((id) => id !== task.id);
      next.splice(index, 0, task.id);
      if (next.join() !== dragOrder.join()) dragOrder = next;
    };
    const end = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", end);
      window.removeEventListener("pointercancel", end);
      const order = dragOrder;
      const changed = order && order.join() !== store.open.map((t) => t.id).join();
      dragId = null;
      if (order && changed) {
        store.reorder([...order, ...store.done.map((t) => t.id)]).finally(() => (dragOrder = null));
      } else {
        dragOrder = null;
      }
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", end);
    window.addEventListener("pointercancel", end);
  }

  function onKey(e: KeyboardEvent) {
    if (e.key !== "Escape") return;
    if (store.openId !== null) store.openId = null;
    else if (store.view === "settings") store.view = "list";
    else if (inTauri) store.shell("hide_main");
  }
</script>

<svelte:window onkeydown={onKey} />

{#snippet windowControls()}
  <div class="controls">
    <button class="icon-btn" class:on={app.pinned} title={app.pinned ? "取消置顶" : "置顶"} onclick={() => store.shell("set_pinned", { pinned: !app.pinned })}>
      <Icon name="pin" size={15} />
    </button>
    <button class="icon-btn" class:on={app.expanded} title={app.expanded ? "收成小面板" : "展开日历视图"} onclick={() => store.setExpanded(!app.expanded)}>
      <Icon name="sidebar" size={15} />
    </button>
    <button class="icon-btn" class:on={store.view === "settings"} title="设置" onclick={() => (store.view = store.view === "settings" ? "list" : "settings")}>
      <Icon name="sliders" size={15} />
    </button>
    <button class="icon-btn" title="收到托盘" onclick={() => store.shell("hide_main")}>
      <Icon name="minus" size={15} />
    </button>
  </div>
{/snippet}

{#snippet noteArea()}
  <textarea
    class="note-text"
    placeholder="这一天的便签：想法、链接、要记住的话…"
    value={store.note}
    oninput={(e) => store.editNote(e.currentTarget.value)}
    onblur={() => store.flushNote()}
    aria-label="当日便签"
  ></textarea>
{/snippet}

<div class="shell" class:expanded={app.expanded} class:solid={!app.glass || !inTauri} class:preview={!inTauri}>
  {#if app.expanded}
    <aside class="side" data-tauri-drag-region>
      <div class="brand" data-tauri-drag-region>
        <span class="logo" data-tauri-drag-region></span>
        <span data-tauri-drag-region>DayNote</span>
      </div>
      <Calendar />
      <div class="month-stat">
        <span class="label">本月</span>
        <span class="num"><b>{store.monthTotal.done}</b> / {store.monthTotal.total} 已完成</span>
      </div>
    </aside>
  {/if}

  <main>
    <header data-tauri-drag-region>
      <div class="day" data-tauri-drag-region>
        <h1 data-tauri-drag-region>{store.view === "settings" ? "设置" : dayName}</h1>
        {#if store.view === "list"}
          <span class="sub" data-tauri-drag-region>{named ? `${monthDay(store.date)} ` : ""}{weekday(store.date)}</span>
        {/if}
      </div>
      {#if store.view === "list"}
        <div class="nav">
          {#if !store.isToday}
            <button class="chip" onclick={() => store.go(store.today)}>回今天</button>
          {/if}
          <button class="icon-btn" title="前一天" onclick={() => store.step(-1)}><Icon name="chevron-left" /></button>
          <button class="icon-btn" title="后一天" onclick={() => store.step(1)}><Icon name="chevron-right" /></button>
        </div>
      {:else}
        <div class="nav">
          <button class="chip" onclick={() => (store.view = "list")}>完成</button>
        </div>
      {/if}
      {#if !app.expanded}{@render windowControls()}{/if}
    </header>

    {#if store.view === "settings"}
      <div class="scroll"><Settings /></div>
    {:else}
      <div class="progress" data-tauri-drag-region>
        <div class="bar"><span style:width={total ? `${(doneCount / total) * 100}%` : "0%"}></span></div>
        <span class="count num">
          {#if total}{doneCount} / {total} 已完成{:else}还没有安排{/if}
        </span>
      </div>

      <div class="scroll">
        {#if store.isToday && store.overdue.length}
          <section class="late">
            <div class="section-head">
              <button class="fold" onclick={() => (showLate = !showLate)}>
                <span class="chev" class:shut={!showLate}><Icon name="chevron-down" size={13} /></span>
                遗留 <span class="num">{store.overdue.length}</span>
              </button>
              <button class="link" onclick={() => store.move(store.overdue.map((t) => t.id), store.today)}>全部挪到今天</button>
            </div>
            {#if showLate}
              {#each store.overdue as task (task.id)}
                <div class="late-row">
                  <span class="late-date num" title={task.date}>{relativeDay(task.date, store.today)}</span>
                  <span class="late-title">{task.title}</span>
                  <button class="link" title="挪到今天" onclick={() => store.move([task.id], store.today)}>
                    今天做 <Icon name="arrow-right" size={12} />
                  </button>
                </div>
              {/each}
            {/if}
          </section>
        {/if}

        <div class="list" class:dragging={dragId !== null} bind:this={listEl}>
          {#each openTasks as task (task.id)}
            <div class="slot" class:lifted={dragId === task.id} data-task-id={task.id}>
              <TaskRow {task} ongrip={startDrag} />
            </div>
          {/each}
        </div>

        {#if store.ready && total === 0}
          <div class="empty">
            <p>{store.isToday ? "今天还没有事项" : "这一天还没有事项"}</p>
            <p class="dim">在下面记一件，或者让 AI 帮你把计划写进来</p>
          </div>
        {:else if store.ready && store.open.length === 0}
          <div class="empty small"><p>都做完了</p></div>
        {/if}

        {#if store.done.length}
          <section class="done-section">
            <div class="section-head">
              <button class="fold" onclick={() => (showDone = !showDone)}>
                <span class="chev" class:shut={!showDone}><Icon name="chevron-down" size={13} /></span>
                已完成 <span class="num">{store.done.length}</span>
              </button>
            </div>
            {#if showDone}
              {#each store.done as task (task.id)}
                <TaskRow {task} />
              {/each}
            {/if}
          </section>
        {/if}
      </div>

      {#if !app.expanded}
        <div class="note-box" class:open={noteOpen}>
          <button class="note-toggle" onclick={() => (noteOpen = !noteOpen)}>
            <Icon name="note" size={14} />
            <span class="note-peek">{noteOpen ? "便签" : store.note.split("\n")[0] || "便签"}</span>
            <span class="chev" class:shut={!noteOpen}><Icon name="chevron-down" size={13} /></span>
          </button>
          {#if noteOpen}{@render noteArea()}{/if}
        </div>
      {/if}

      <div class="add">
        <span class="plus"><Icon name="plus" size={15} /></span>
        <input placeholder="记一件事…　试试「明天 15:00 回访客户」" bind:value={draft} onkeydown={submit} aria-label="新事项" />
        {#if hint}<span class="hint">{hint}</span>{/if}
      </div>
    {/if}
  </main>

  {#if app.expanded}
    <aside class="note-pane">
      <header data-tauri-drag-region>
        <span class="pane-title" data-tauri-drag-region>便签</span>
        {@render windowControls()}
      </header>
      {@render noteArea()}
    </aside>
  {/if}

  {#if store.toast}
    <div class="toast" role="status">
      <span>{store.toast.text}</span>
      {#if store.toast.undo}
        <button
          class="link"
          onclick={() => {
            store.toast?.undo?.();
            store.dismissToast();
          }}
        >
          <Icon name="undo" size={13} />撤销
        </button>
      {/if}
    </div>
  {/if}
</div>

<style>
  .shell {
    position: relative;
    height: 100%;
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    background: var(--glass);
    overflow: hidden;
    /* The window has no system frame: draw the hairline edge ourselves (Windows 11
       rounds the window itself to an 8px radius). */
    border-radius: 8px;
    box-shadow: inset 0 0 0 1px var(--line-strong);
  }
  .shell.solid {
    background: var(--glass-solid);
  }
  /* Browser preview only: stand-in for the desktop behind the glass. */
  .shell.preview {
    background:
      radial-gradient(120% 80% at 0% 0%, rgba(96, 120, 200, 0.28), transparent 60%),
      radial-gradient(90% 70% at 100% 100%, rgba(150, 100, 180, 0.2), transparent 60%), #14161c;
  }
  .shell.expanded {
    grid-template-columns: 252px minmax(0, 1fr) 292px;
  }

  main,
  aside {
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  /* Header ---------------------------------------------------------------- */
  header {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 14px 10px 6px 18px;
    min-height: 54px;
  }
  .day {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: baseline;
    gap: 9px;
  }
  h1 {
    margin: 0;
    font-family: var(--font-display);
    font-size: 21px;
    font-weight: 600;
    letter-spacing: 0.01em;
    white-space: nowrap;
  }
  .sub {
    font-size: 12px;
    color: var(--text-2);
    white-space: nowrap;
  }
  .nav,
  .controls {
    display: flex;
    align-items: center;
    gap: 1px;
  }
  .controls {
    margin-left: 4px;
    padding-left: 5px;
    border-left: 1px solid var(--line);
  }
  .nav .chip {
    margin-right: 4px;
  }

  .progress {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 2px 18px 10px;
  }
  .bar {
    flex: 1;
    height: 3px;
    border-radius: 3px;
    background: var(--fill-2);
    overflow: hidden;
    pointer-events: none;
  }
  .bar span {
    display: block;
    height: 100%;
    border-radius: 3px;
    background: var(--accent);
    transition: width 0.35s var(--ease);
  }
  .count {
    font-size: 11.5px;
    color: var(--text-3);
    pointer-events: none;
  }

  /* List -------------------------------------------------------------------- */
  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 0 8px 8px;
    scrollbar-gutter: stable;
  }
  .list {
    display: flex;
    flex-direction: column;
  }
  .slot {
    border-radius: var(--r-md);
  }
  .slot.lifted {
    background: var(--fill-2);
    box-shadow: 0 6px 20px rgba(0, 0, 0, 0.35), inset 0 0 0 1px var(--line-strong);
  }
  .list.dragging {
    cursor: grabbing;
  }

  .section-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 6px 10px 4px;
  }
  .fold {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-size: 11.5px;
    letter-spacing: 0.04em;
    color: var(--text-3);
  }
  .fold:hover {
    color: var(--text-2);
  }
  .chev {
    display: grid;
    transition: transform 0.18s var(--ease);
  }
  .chev.shut {
    transform: rotate(-90deg);
  }
  .link {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 11.5px;
    color: var(--accent);
    white-space: nowrap;
    border-radius: 4px;
  }
  .link:hover {
    text-decoration: underline;
    text-underline-offset: 3px;
  }

  .late {
    margin-bottom: 8px;
    padding-bottom: 6px;
    border-radius: var(--r-md);
    background: var(--amber-soft);
  }
  .late .fold {
    color: var(--amber);
  }
  .late-row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 5px 10px;
    font-size: 12.5px;
  }
  .late-date {
    flex: none;
    min-width: 34px;
    font-size: 11px;
    color: var(--amber);
  }
  .late-title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text-2);
  }
  .late-row .link {
    opacity: 0;
  }
  .late-row:hover .link,
  .late-row .link:focus-visible {
    opacity: 1;
  }

  .done-section {
    margin-top: 10px;
    padding-top: 4px;
    border-top: 1px solid var(--line);
  }

  .empty {
    padding: 44px 16px 20px;
    text-align: center;
    color: var(--text-2);
  }
  .empty.small {
    padding: 18px 16px 6px;
    font-size: 12.5px;
    color: var(--text-3);
  }
  .empty p {
    margin: 0;
  }
  .empty .dim {
    margin-top: 6px;
    font-size: 12px;
    color: var(--text-3);
  }

  /* Note -------------------------------------------------------------------- */
  .note-box {
    margin: 0 10px;
    border-top: 1px solid var(--line);
  }
  .note-toggle {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 8px;
    color: var(--text-3);
    font-size: 12px;
    text-align: left;
  }
  .note-toggle:hover {
    color: var(--text-2);
  }
  .note-peek {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .note-text {
    width: 100%;
    line-height: 1.6;
    color: var(--text);
  }
  .note-box .note-text {
    height: 132px;
    padding: 0 8px 8px;
    font-size: 12.5px;
    color: var(--text-2);
  }

  /* Add bar ----------------------------------------------------------------- */
  .add {
    display: flex;
    align-items: center;
    gap: 9px;
    margin: 0 10px 10px;
    padding: 0 12px;
    height: 40px;
    border-radius: var(--r-lg);
    background: var(--fill-2);
    border: 1px solid var(--line);
    transition: border-color 0.15s, background 0.15s;
  }
  .add:focus-within {
    border-color: rgba(143, 176, 255, 0.55);
    background: var(--fill-3);
  }
  .plus {
    color: var(--text-3);
    display: grid;
  }
  .add:focus-within .plus {
    color: var(--accent);
  }
  .add input {
    flex: 1;
    min-width: 0;
    height: 100%;
  }
  .hint {
    flex: none;
    font-size: 11px;
    padding: 2px 8px;
    border-radius: 999px;
    color: var(--accent);
    background: var(--accent-soft);
    white-space: nowrap;
  }

  /* Expanded view ------------------------------------------------------------- */
  .side {
    padding: 16px 16px 14px;
    gap: 16px;
    border-right: 1px solid var(--line);
    background: rgba(0, 0, 0, 0.14);
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 9px;
    height: 24px;
    font-family: var(--font-display);
    font-size: 13px;
    font-weight: 600;
    color: var(--text-2);
    letter-spacing: 0.02em;
  }
  .logo {
    width: 14px;
    height: 14px;
    border-radius: 50%;
    border: 2.5px solid var(--accent);
  }
  .month-stat {
    margin-top: auto;
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    padding-top: 12px;
    border-top: 1px solid var(--line);
    font-size: 12px;
    color: var(--text-2);
  }
  .month-stat b {
    font-size: 15px;
    font-weight: 600;
    color: var(--text);
  }

  .note-pane {
    border-left: 1px solid var(--line);
    background: rgba(0, 0, 0, 0.14);
  }
  .note-pane header {
    padding-left: 18px;
  }
  .pane-title {
    flex: 1;
    font-size: 11px;
    letter-spacing: 0.08em;
    color: var(--text-3);
  }
  .note-pane .note-text {
    flex: 1;
    padding: 4px 18px 18px;
    font-size: 13px;
  }
  .expanded main header {
    padding-right: 14px;
  }

  /* Toast ------------------------------------------------------------------- */
  .toast {
    position: absolute;
    left: 50%;
    bottom: 60px;
    transform: translateX(-50%);
    display: flex;
    align-items: center;
    gap: 12px;
    max-width: calc(100% - 32px);
    padding: 8px 14px;
    border-radius: 999px;
    background: var(--card);
    border: 1px solid var(--line-strong);
    box-shadow: 0 8px 28px rgba(0, 0, 0, 0.45);
    font-size: 12.5px;
    animation: rise 0.2s var(--ease);
    z-index: 10;
  }
  .toast span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  @keyframes rise {
    from {
      opacity: 0;
      transform: translate(-50%, 6px);
    }
  }
</style>
