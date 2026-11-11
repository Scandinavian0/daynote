<script lang="ts">
  import { updater } from "../lib/updater.svelte.ts";

  let dialog = $state<HTMLElement>();
  const percent = $derived(updater.total && updater.total > 0 ? Math.min(100, Math.floor(updater.downloaded / updater.total * 100)) : null);
  const size = (bytes: number) => `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  $effect(() => {
    if (updater.dialogOpen && dialog) dialog.focus();
  });
</script>

{#if updater.dialogOpen && updater.update}
  <div class="update-backdrop">
    <div class="update-dialog" role="dialog" aria-modal="true" aria-labelledby="update-title" aria-describedby="update-restart" aria-busy={updater.busy} tabindex="-1" bind:this={dialog}>
      <p class="eyebrow">DAYNOTE 更新</p>
      <h2 id="update-title">{updater.phase === "installing" ? "正在安装更新" : `新版本 ${updater.update.version}`}</h2>
      <p class="version">当前 {updater.update.current_version} → {updater.update.version}</p>
      {#if updater.update.body}<div class="release-notes">{updater.update.body}</div>{/if}
      <p class="restart" id="update-restart">安装时会关闭并重新启动 DayNote。安装前会保存所有日期的便签。</p>
      <p class="restart">请先保存尚未提交的任务或速记输入，并退出正在使用 DayNote 的 MCP 客户端。</p>

      {#if updater.phase === "downloading"}
        <div class="download" role="status">
          <p>正在下载… {percent === null ? size(updater.downloaded) : `${percent}%`}</p>
          <progress max="100" value={percent ?? undefined} aria-label="更新下载进度"></progress>
          {#if updater.total}<span>{size(updater.downloaded)} / {size(updater.total)}</span>{/if}
        </div>
      {:else if updater.phase === "preparing"}
        <p class="status" role="status">正在保存便签，请稍候…</p>
      {:else if updater.phase === "installing"}
        <p class="status" role="status">正在启动安装程序，请等待应用关闭并重新启动…</p>
      {/if}
      {#if updater.error}<p class="error" role="alert">{updater.error}</p>{/if}

      <div class="actions">
        <button class="btn" disabled={updater.busy} onclick={() => updater.defer()}>稍后</button>
        <button class="btn primary" disabled={updater.busy} onclick={() => updater.install()}>{updater.phase === "downloading" ? "下载中…" : updater.locked ? "正在安装…" : updater.phase === "error" ? "重试下载并安装" : "下载并安装"}</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .update-backdrop { position: fixed; inset: 0; z-index: 100; display: grid; place-items: center; padding: 16px; background: rgba(7, 9, 14, 0.68); backdrop-filter: blur(7px); }
  .update-dialog { width: 360px; max-width: 100%; max-height: 100%; overflow: auto; padding: 20px; border: 1px solid var(--line-strong); border-radius: var(--r-lg); background: var(--card); box-shadow: 0 18px 60px #0006; outline: none; }
  .eyebrow { margin: 0 0 7px; font-size: 10px; letter-spacing: 0.12em; color: var(--accent); }
  h2 { margin: 0; font-size: 18px; font-weight: 600; }
  .version { margin: 6px 0 14px; color: var(--text-3); font-size: 12px; }
  .release-notes { max-height: 140px; overflow: auto; white-space: pre-wrap; overflow-wrap: anywhere; user-select: text; font-size: 12.5px; line-height: 1.6; color: var(--text-2); }
  .restart { margin: 14px 0; font-size: 12px; line-height: 1.6; color: var(--text-2); }
  .download p, .status { margin: 12px 0 7px; font-size: 12px; color: var(--accent); }
  .download span { color: var(--text-3); font-size: 11px; }
  progress { width: 100%; height: 5px; display: block; margin-bottom: 6px; border: 0; border-radius: 4px; overflow: hidden; accent-color: var(--accent); }
  progress::-webkit-progress-bar { background: var(--fill-3); }
  progress::-webkit-progress-value { background: var(--accent); }
  .error { color: var(--danger); font-size: 12px; line-height: 1.6; overflow-wrap: anywhere; }
  .actions { margin-top: 18px; display: flex; justify-content: flex-end; gap: 8px; flex-wrap: wrap; }
  button:disabled { opacity: 0.45; cursor: default; }
</style>
