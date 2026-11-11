<script lang="ts">
  // Preferences, recurring tasks, and the connection details an AI agent needs.
  import { onMount } from "svelte";
  import { store } from "../lib/store.svelte.ts";
  import { updater } from "../lib/updater.svelte.ts";
  import { ruleText } from "../lib/dates.ts";
  import Icon from "./Icon.svelte";
  import { cmd, errorText } from "../lib/backend.ts";

  onMount(() => {
    store.loadSeries();
  });

  const app = $derived(store.app);
  const pref = (key: string, value: string | boolean | number) => store.shell("set_pref", { key, value: String(value) });

  // Global shortcut: click the field, then press the combination.
  let recording = $state(false);
  function recordKey(e: KeyboardEvent) {
    e.preventDefault();
    e.stopPropagation();
    if (e.key === "Escape") {
      recording = false;
      (e.currentTarget as HTMLElement).blur();
      return;
    }
    if (["Control", "Shift", "Alt", "Meta"].includes(e.key)) return;
    const mods = [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Super"].filter(Boolean);
    if (!mods.length) return;
    const key = e.code.startsWith("Key")
      ? e.code.slice(3)
      : e.code.startsWith("Digit")
        ? e.code.slice(5)
        : e.code === "Space"
          ? "Space"
          : e.key.length === 1
            ? e.key.toUpperCase()
            : e.key;
    recording = false;
    (e.currentTarget as HTMLElement).blur();
    store.shell("set_hotkey", { accelerator: [...mods, key].join("+") });
  }

  // New recurring task form
  let title = $state("");
  let kind = $state<"daily" | "weekdays" | "weekly" | "monthly">("daily");
  let weekDays = $state<number[]>([1]);
  let monthDay = $state(1);
  let time = $state("");

  function toggleDay(n: number) {
    weekDays = weekDays.includes(n) ? weekDays.filter((d) => d !== n) : [...weekDays, n].sort();
  }

  async function addSeries() {
    const name = title.trim();
    if (!name) return;
    if (kind === "weekly" && !weekDays.length) return;
    const rule = kind === "weekly" ? `weekly:${weekDays.join(",")}` : kind === "monthly" ? `monthly:${monthDay}` : kind;
    await store.addSeries({ title: name, rule, remind_time: time || null });
    title = "";
    time = "";
  }

  async function copy(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      store.notify("已复制");
    } catch {
      store.notify("复制失败，请手动选择文本");
    }
  }

  const mcpCommand = $derived(`"${app.cli_path}" mcp`);
  const httpUrl = $derived(`http://127.0.0.1:${app.http_active_port}/v1`);
  let port = $state("");
  let portDirty = $state(false);
  let httpSaving = $state(false);
  let httpMessage = $state("");
  let httpFailed = $state(false);
  let tokenSaving = $state(false);
  let tokenMessage = $state("");
  let tokenFailed = $state(false);
  let directory = $state("");
  let directoryDirty = $state(false);
  let directorySaving = $state(false);
  let directoryMessage = $state("");
  let directoryFailed = $state(false);
  $effect(() => {
    if (!portDirty) port = String(app.http_port);
  });
  $effect(() => {
    if (!directoryDirty) directory = app.next_data_dir || app.data_dir;
  });

  async function regenerateToken() {
    if (tokenSaving) return;
    tokenSaving = true;
    tokenMessage = "";
    tokenFailed = false;
    try {
      await cmd("regenerate_api_token");
      tokenMessage = "令牌已重新生成，旧令牌已失效；请让 HTTP 客户端重新读取令牌文件";
    } catch (e) {
      tokenMessage = errorText(e);
      tokenFailed = true;
    } finally { tokenSaving = false; }
  }

  async function saveDirectory() {
    if (directorySaving || !directory.trim()) return;
    directorySaving = true;
    directoryMessage = "";
    directoryFailed = false;
    try {
      const message = await cmd<string>("set_data_directory", { path: directory.trim() });
      await store.loadApp();
      directoryDirty = false;
      directoryMessage = `目录已保存，重启 DayNote 和 MCP 客户端后生效：${message}`;
    } catch (e) {
      directoryMessage = errorText(e);
      directoryFailed = true;
    } finally { directorySaving = false; }
  }

  async function saveHttp(key: "http_port" | "http_enabled", value: string) {
    if (httpSaving) return;
    httpMessage = "";
    httpFailed = false;
    if (key === "http_port" && (!/^\d+$/.test(value.trim()) || Number(value) < 1024 || Number(value) > 65535)) {
      httpMessage = "端口应为 1024–65535 之间的整数";
      httpFailed = true;
      return;
    }
    httpSaving = true;
    try {
      await cmd("set_pref", { key, value });
      await store.loadApp();
      if (key === "http_port") {
        port = String(app.http_port);
        portDirty = false;
      }
      httpMessage = key === "http_port" ? "端口已保存" : value === "true" ? "HTTP 接口已开启" : "HTTP 接口已关闭";
    } catch (e) {
      httpMessage = errorText(e);
      httpFailed = true;
      await store.loadApp();
    } finally {
      httpSaving = false;
    }
  }

  function toggleHttp(event: Event) {
    const checkbox = event.currentTarget as HTMLInputElement;
    const requested = checkbox.checked;
    // Native clicks toggle before onchange; retain the saved state until the
    // command succeeds, including when Svelte's cached value is unchanged.
    checkbox.checked = app.http_enabled;
    void saveHttp("http_enabled", String(requested));
  }
</script>

<div class="settings">
  <section>
    <h3>常规</h3>
    <label class="line">
      <span>开机自动启动</span>
      <input type="checkbox" class="switch" checked={app.autostart} onchange={(e) => store.shell("set_autostart", { enabled: e.currentTarget.checked })} />
    </label>
    <label class="line">
      <span>提醒时播放提示音</span>
      <input type="checkbox" class="switch" checked={app.sound} onchange={(e) => pref("sound", e.currentTarget.checked)} />
    </label>
    <div class="line">
      <span>速记快捷键</span>
      <input
        class="field key"
        readonly
        value={recording ? "按下组合键…" : app.hotkey || "未设置"}
        onfocus={() => (recording = true)}
        onblur={() => (recording = false)}
        onkeydown={recordKey}
        aria-label="速记快捷键"
      />
    </div>
  </section>

  <section>
    <h3>每日提醒</h3>
    <div class="line">
      <span><Icon name="sun" size={14} />晨间简报</span>
      <input type="time" class="field" value={app.morning_time} disabled={!app.morning_enabled} onchange={(e) => e.currentTarget.value && pref("morning_time", e.currentTarget.value)} aria-label="晨间简报时间" />
      <input type="checkbox" class="switch" checked={app.morning_enabled} onchange={(e) => pref("morning_enabled", e.currentTarget.checked)} aria-label="开启晨间简报" />
    </div>
    <div class="line">
      <span><Icon name="moon" size={14} />晚间回顾</span>
      <input type="time" class="field" value={app.evening_time} disabled={!app.evening_enabled} onchange={(e) => e.currentTarget.value && pref("evening_time", e.currentTarget.value)} aria-label="晚间回顾时间" />
      <input type="checkbox" class="switch" checked={app.evening_enabled} onchange={(e) => pref("evening_enabled", e.currentTarget.checked)} aria-label="开启晚间回顾" />
    </div>
    <p class="help">晨间简报在开机后或到点时弹出今天的事项；晚间回顾列出还没做完的事，可一键挪到明天。</p>
  </section>

  <section>
    <h3>重复任务</h3>
    {#each store.series as s (s.id)}
      <div class="series" class:off={!s.active}>
        <div class="series-main">
          <span class="series-title">{s.title}</span>
          <span class="series-rule">{ruleText(s.rule)}{s.remind_time ? ` · ${s.remind_time} 提醒` : ""}</span>
        </div>
        <input type="checkbox" class="switch" checked={s.active} onchange={(e) => store.updateSeries(s.id, { active: e.currentTarget.checked })} aria-label="启用" />
        <button class="icon-btn" title="删除" onclick={() => store.removeSeries(s.id)}><Icon name="trash" size={14} /></button>
      </div>
    {:else}
      <p class="help">还没有重复任务。</p>
    {/each}

    <div class="new-series">
      <input class="field grow" placeholder="新的重复任务，例如：回复询盘" bind:value={title} onkeydown={(e) => e.key === "Enter" && !e.isComposing && addSeries()} />
      <div class="rule-row">
        {#each [["daily", "每天"], ["weekdays", "工作日"], ["weekly", "每周"], ["monthly", "每月"]] as [value, text]}
          <button class="chip" class:on={kind === value} onclick={() => (kind = value as typeof kind)}>{text}</button>
        {/each}
      </div>
      {#if kind === "weekly"}
        <div class="rule-row">
          {#each ["一", "二", "三", "四", "五", "六", "日"] as name, i}
            <button class="chip dot" class:on={weekDays.includes(i + 1)} onclick={() => toggleDay(i + 1)}>{name}</button>
          {/each}
        </div>
      {/if}
      <div class="rule-row">
        {#if kind === "monthly"}
          <label class="inline">每月 <input type="number" class="field num-field" min="1" max="31" bind:value={monthDay} /> 日</label>
        {/if}
        <label class="inline"><Icon name="bell" size={13} /><input type="time" class="field" bind:value={time} aria-label="提醒时间（可选）" /></label>
        <span class="grow"></span>
        <button class="btn primary" disabled={!title.trim()} onclick={addSeries}>添加</button>
      </div>
    </div>
  </section>

  <section>
    <h3>AI 接口</h3>
    <p class="help">Codex 和 Claude Code 通过 MCP 读写这里的事项；应用没开时也能用。</p>
    <div class="kv">
      <span class="label">MCP 启动命令</span>
      <code>{mcpCommand}</code>
      <button class="icon-btn" title="复制" onclick={() => copy(mcpCommand)}><Icon name="copy" size={14} /></button>
    </div>
    <label class="line">
      <span>本地 HTTP 接口</span>
      <input type="checkbox" class="switch" checked={app.http_enabled} disabled={httpSaving} aria-label="开启本地 HTTP 接口" onchange={toggleHttp} />
    </label>
    <form class="line" onsubmit={(e) => { e.preventDefault(); saveHttp("http_port", port); }}>
      <label class="grow" for="http-port">HTTP 端口</label>
      <input id="http-port" class="field port-field" type="text" inputmode="numeric" aria-label="HTTP 端口" value={portDirty ? port : String(app.http_port)} disabled={httpSaving} oninput={(e) => { port = e.currentTarget.value; portDirty = true; httpMessage = ""; }} />
      <button type="submit" class="btn" disabled={httpSaving}>{httpSaving ? "保存中…" : "保存端口"}</button>
    </form>
    <p class="help">端口范围 1024–65535。保存后立即生效；接口关闭时保存端口不会开启接口。</p>
    {#if httpMessage}<p class="help" class:failed={httpFailed} role={httpFailed ? "alert" : "status"}>{httpMessage}</p>{/if}
    <p class="help" role="status">{!app.http_enabled ? "HTTP 接口已关闭" : app.http_active_port ? `正在监听 127.0.0.1:${app.http_active_port}` : "HTTP 接口未启动"}</p>
    {#if app.http_enabled && app.http_error}<p class="help failed" role="alert">{app.http_error}</p>{/if}
    {#if app.http_enabled}
      {#if app.http_active_port}
      <div class="kv">
        <span class="label">地址</span>
        <code>{httpUrl}</code>
        <button class="icon-btn" title="复制" onclick={() => copy(httpUrl)}><Icon name="copy" size={14} /></button>
      </div>
      {/if}
      <div class="kv">
        <span class="label">令牌文件</span>
        <code>{app.token_path}</code>
        <button class="icon-btn" title="复制路径" onclick={() => copy(app.token_path)}><Icon name="copy" size={14} /></button>
      </div>
    {/if}
    <p class="help">重新生成后旧令牌立即失效，外部 HTTP 客户端需要更新令牌；MCP 启动命令会自动生成。</p>
    <button class="btn" disabled={tokenSaving || httpSaving} onclick={regenerateToken}>{tokenSaving ? "正在更新令牌…" : "重新生成访问令牌"}</button>
    {#if tokenMessage}<p class="help" class:failed={tokenFailed} role={tokenFailed ? "alert" : "status"}>{tokenMessage}</p>{/if}
    <div class="kv">
      <span class="label">数据目录</span>
      <code>{app.data_dir}</code>
      <button class="icon-btn" title="复制路径" onclick={() => copy(app.data_dir)}><Icon name="copy" size={14} /></button>
    </div>
    <label for="data-directory">下次启动的数据目录</label>
    <input id="data-directory" class="field" aria-label="数据目录" value={directoryDirty ? directory : app.next_data_dir || app.data_dir} disabled={directorySaving || app.data_dir_overridden} oninput={(e) => { directory = e.currentTarget.value; directoryDirty = true; directoryMessage = ""; }} />
    <p class="help">仅切换目录，不自动搬迁；原数据保留。先备份并复制数据，保存后重启 DayNote 和 MCP 客户端。</p>
    {#if app.data_dir_overridden}<p class="help">当前由 DAYNOTE_DATA_DIR 环境变量指定目录，请修改环境变量后重启。</p>{/if}
    <button class="btn" disabled={directorySaving || app.data_dir_overridden || !(directoryDirty ? directory : app.next_data_dir || app.data_dir).trim()} onclick={saveDirectory}>{directorySaving ? "保存中…" : "保存数据目录"}</button>
    {#if app.next_data_dir && app.next_data_dir !== app.data_dir}<p class="help" role="status">已选择新目录，重启 DayNote 和 MCP 客户端后生效。</p>{/if}
    {#if directoryMessage}<p class="help" class:failed={directoryFailed} role={directoryFailed ? "alert" : "status"}>{directoryMessage}</p>{/if}
  </section>

  <section>
    <h3>应用更新</h3>
    <div class="line">
      <span>当前版本 {app.version || "读取中…"}</span>
      <button class="btn" disabled={updater.busy} onclick={() => updater.check(true)}>{updater.phase === "checking" ? "检查中…" : updater.phase === "error" ? "重新检查" : "检查更新"}</button>
    </div>
    {#if updater.phase === "latest"}<p class="help" role="status">已是最新版本</p>{/if}
    {#if updater.phase === "available" && updater.update}<p class="help" role="status">新版本 {updater.update.version} 可用</p>{/if}
    {#if updater.phase === "unsupported"}<p class="help" role="status">自动更新仅支持 DayNote 桌面应用，请在桌面应用中检查更新。</p>{/if}
    {#if updater.phase === "error"}<p class="help failed" role="alert">{updater.error}</p>{/if}
  </section>

  <footer>
    <span>DayNote {app.version}</span>
    <button class="btn ghost" onclick={() => store.shell("quit_app")}>退出应用</button>
  </footer>
</div>

<style>
  .settings {
    padding: 4px 16px 20px;
    display: flex;
    flex-direction: column;
    gap: 22px;
    max-width: 560px;
  }
  section {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  h3 {
    margin: 0 0 2px;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.08em;
    color: var(--text-3);
  }
  .line {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 32px;
  }
  .line > span:first-child {
    flex: 1;
    display: inline-flex;
    align-items: center;
    gap: 7px;
  }
  .help {
    margin: 0;
    font-size: 12px;
    color: var(--text-3);
    line-height: 1.55;
  }
  .key {
    width: 150px;
    text-align: center;
    cursor: pointer;
    font-size: 12.5px;
  }
  input[type="time"].field {
    width: 96px;
  }
  input:disabled {
    opacity: 0.4;
  }

  .switch {
    appearance: none;
    flex: none;
    width: 34px;
    height: 20px;
    margin: 0;
    border-radius: 999px;
    background: var(--fill-3);
    position: relative;
    cursor: pointer;
    transition: background 0.18s;
  }
  .switch::after {
    content: "";
    position: absolute;
    top: 3px;
    left: 3px;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: rgba(255, 255, 255, 0.85);
    transition: transform 0.18s var(--ease), background 0.18s;
  }
  .switch:checked {
    background: var(--accent);
  }
  .switch:checked::after {
    transform: translateX(14px);
    background: var(--accent-ink);
  }

  .series {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 6px 8px 12px;
    border-radius: var(--r-md);
    background: var(--fill-1);
  }
  .series.off .series-main {
    opacity: 0.45;
  }
  .series-main {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .series-title {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .series-rule {
    font-size: 11.5px;
    color: var(--text-3);
  }

  .new-series {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px;
    border-radius: var(--r-md);
    border: 1px dashed var(--line-strong);
  }
  .rule-row {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 6px;
  }
  .grow {
    flex: 1;
  }
  .chip.dot {
    width: 26px;
    padding: 0;
    justify-content: center;
  }
  .inline {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 12.5px;
    color: var(--text-2);
  }
  .num-field {
    width: 54px;
  }
  .port-field { width: 82px; }
  .help.failed { color: var(--danger, #d04444); }
  .btn:disabled {
    opacity: 0.4;
    pointer-events: none;
  }

  .kv {
    display: grid;
    grid-template-columns: 1fr auto;
    align-items: center;
    column-gap: 6px;
    padding: 8px 6px 8px 12px;
    border-radius: var(--r-md);
    background: var(--fill-1);
  }
  .kv .label {
    grid-column: 1 / -1;
  }
  code {
    font-family: "Cascadia Mono", Consolas, monospace;
    font-size: 11.5px;
    color: var(--text-2);
    overflow-wrap: anywhere;
    user-select: text;
  }

  footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    font-size: 11.5px;
    color: var(--text-3);
    padding-top: 6px;
    border-top: 1px solid var(--line);
  }
</style>
