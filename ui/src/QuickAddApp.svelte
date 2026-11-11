<script lang="ts">
  // Quick-add box, summoned by the global shortcut: type one line, Enter saves, Esc closes.
  import { onMount } from "svelte";
  import { call, cmd, errorText, on, inTauri } from "./lib/backend.ts";
  import { todayStr } from "./lib/dates.ts";
  import { parseQuick } from "./lib/quickparse.ts";
  import Icon from "./components/Icon.svelte";

  let text = $state("");
  let error = $state("");
  let saved = $state("");
  let input = $state<HTMLInputElement>();

  const parsed = $derived(parseQuick(text, todayStr(), new Date()));

  function reset() {
    text = "";
    error = "";
    saved = "";
    input?.focus();
  }

  onMount(() => {
    input?.focus();
    const off = on("quickadd-shown", reset);
    return () => off.then((fn) => fn());
  });

  const close = () => cmd("quickadd_hide").catch(() => {});

  async function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      close();
      return;
    }
    if (e.key !== "Enter" || e.isComposing || !parsed.title) return;
    try {
      await call("add_tasks", { date: parsed.date, tasks: [{ title: parsed.title, remind_at: parsed.remind_at }] });
      saved = parsed.hint ? `已记下 · ${parsed.hint}` : "已记到今天";
      text = "";
      error = "";
      // Shift+Enter keeps the box open for the next line.
      if (!e.shiftKey) setTimeout(close, 420);
    } catch (err) {
      error = errorText(err);
    }
  }
</script>

<div class="wrap" class:preview={!inTauri}>
  <div class="box">
    <span class="mark"><Icon name="plus" size={18} /></span>
    <input
      bind:this={input}
      bind:value={text}
      onkeydown={onKey}
      placeholder="记一件事…　明天 15:00 回访客户"
      aria-label="速记"
      spellcheck="false"
    />
    {#if text.trim() && parsed.hint}<span class="hint">{parsed.hint}</span>{/if}
  </div>
  <div class="foot">
    {#if error}
      <span class="err">{error}</span>
    {:else if saved}
      <span class="ok"><Icon name="check" size={13} />{saved}</span>
    {:else}
      <span>回车保存 · Shift+回车连续记 · Esc 关闭</span>
    {/if}
  </div>
</div>

<style>
  .wrap {
    height: 100%;
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 7px;
    padding: 10px 14px;
    background: var(--card);
    border: 1px solid var(--line-strong);
    border-radius: var(--r-lg);
  }
  .wrap.preview {
    width: 560px;
    height: 92px;
    margin: 40px auto;
  }
  .box {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .mark {
    color: var(--accent);
    display: grid;
  }
  input {
    flex: 1;
    min-width: 0;
    font-size: 17px;
    font-family: var(--font-display);
    height: 34px;
  }
  .hint {
    flex: none;
    font-size: 11.5px;
    padding: 3px 9px;
    border-radius: 999px;
    color: var(--accent);
    background: var(--accent-soft);
    white-space: nowrap;
  }
  .foot {
    padding-left: 28px;
    font-size: 11px;
    color: var(--text-3);
    min-height: 16px;
  }
  .ok {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    color: var(--accent);
  }
  .err {
    color: var(--danger);
  }
</style>
