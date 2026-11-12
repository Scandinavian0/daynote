import assert from "node:assert/strict";
import { test } from "node:test";
import { createServer } from "vite";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { existsSync } from "node:fs";
import { execFile } from "node:child_process";
import { promisify } from "node:util";

test("AI settings render an editable configured HTTP port and a save action", async () => {
  const cacheDir = await mkdtemp(join(tmpdir(), "daynote-settings-test-"));
  const vite = await createServer({ cacheDir, server: { middlewareMode: true, hmr: false }, appType: "custom" });
  try {
    const { default: Settings } = await vite.ssrLoadModule("/src/components/Settings.svelte");
    const { render } = await vite.ssrLoadModule("svelte/server");
    const { body } = render(Settings);
    assert.match(body, /<input[^>]*aria-label="HTTP 端口"[^>]*>/);
    const input = body.match(/<input[^>]*aria-label="HTTP 端口"[^>]*>/)[0];
    assert.doesNotMatch(input, /readonly|disabled/);
    assert.match(body, /保存端口/);
    const { store } = await vite.ssrLoadModule("/src/lib/store.svelte.ts");
    store.app.http_enabled = false;
    store.app.http_port = 50001;
    store.app.http_active_port = 0;
    const disabled = render(Settings).body;
    assert.match(disabled.match(/<input[^>]*aria-label="HTTP 端口"[^>]*>/)[0], /value="50001"/);
    assert.match(disabled, /HTTP 接口已关闭/);
    assert.doesNotMatch(disabled, /http:\/\/127\.0\.0\.1:0/);
    store.app.http_enabled = true;
    store.app.http_error = "端口已被占用";
    const failed = render(Settings).body;
    assert.match(failed, /HTTP 接口未启动/);
    assert.match(failed, /端口已被占用/);
    assert.doesNotMatch(failed, /http:\/\/127\.0\.0\.1:0/);
    store.app.http_active_port = 50001;
    store.app.http_error = null;
    assert.match(render(Settings).body, /http:\/\/127\.0\.0\.1:50001\/v1/);
  } finally {
    await vite.close();
    await rm(cacheDir, { recursive: true, force: true });
  }
});

test("data directory section tells a pending choice and a failed switch apart from the directory in use", async () => {
  const cacheDir = await mkdtemp(join(tmpdir(), "daynote-settings-dir-test-"));
  const vite = await createServer({ cacheDir, server: { middlewareMode: true, hmr: false }, appType: "custom" });
  try {
    const { default: Settings } = await vite.ssrLoadModule("/src/components/Settings.svelte");
    const { render } = await vite.ssrLoadModule("svelte/server");
    const { store } = await vite.ssrLoadModule("/src/lib/store.svelte.ts");
    store.app.data_dir = "D:\\Example\\Current";
    store.app.next_data_dir = "D:\\Example\\Current";
    store.app.data_dir_error = null;
    const settled = render(Settings).body;
    assert.doesNotMatch(settled, /重启 DayNote 后生效/);
    assert.doesNotMatch(settled, /仍在使用原目录/);

    store.app.next_data_dir = "E:\\Example\\Chosen";
    const pending = render(Settings).body;
    assert.match(pending, /已选择新目录，重启 DayNote 后生效；在此之前应用和 AI 接口都继续使用当前目录/);
    assert.match(pending, /E:\\Example\\Chosen/);

    store.app.data_dir_error = "新数据目录 E:\\Example\\Chosen 未能启用：数据目录不可写入";
    const failed = render(Settings).body;
    assert.match(failed, /<p class="help failed[^"]*" role="alert">[^<]*数据目录不可写入[^<]*仍在使用原目录/);
  } finally {
    await vite.close();
    await rm(cacheDir, { recursive: true, force: true });
  }
});

const chrome = process.env.CHROME_BIN || "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe";

async function browserSettings(harness, backend = "") {
  const temporary = await mkdtemp(join(tmpdir(), "daynote-shared-mcp-test-"));
  const vite = await createServer({
    cacheDir: join(temporary, "cache"),
    server: { host: "127.0.0.1", port: 0, strictPort: false, hmr: { port: 0 } },
    plugins: [{
      name: "settings-shared-mcp-fixture", enforce: "pre",
      resolveId(id) { if (id === "/__settings-harness.js") return id; },
      load(id) {
        if (id === "/__settings-harness.js") return `
          import { mount, tick } from "svelte";
          import Settings from "/src/components/Settings.svelte";
          import { store } from "/src/lib/store.svelte.ts";
          const assert = (condition, message) => { if (!condition) throw new Error(message); };
          const settle = async () => { await new Promise(resolve => setTimeout(resolve, 20)); await tick(); };
          Object.defineProperty(navigator, "clipboard", { value: { writeText: async text => { window.copied = text; } } });
          try { ${harness}; document.body.dataset.result = "PASS"; }
          catch (error) { document.body.dataset.result = "FAIL: " + error.message; }
        `;
        if (id.replaceAll("\\", "/").endsWith("/src/lib/backend.ts")) return `
          export const inTauri = false;
          export async function call() { return { series: [], tasks: [], today: "2026-11-13", content: "", days: [], total: 0, done: 0 }; }
          export async function cmd(name, args) { ${backend}; return { ...window.fixtureApp }; }
          export const on = async (event, handler) => { (window.fixtureHandlers ??= {})[event] = handler; return () => {}; };
          export const errorText = error => error.message;
        `;
      },
      configureServer(server) {
        server.middlewares.use((request, response, next) => {
          if (request.url !== "/__settings-test") return next();
          response.setHeader("Content-Type", "text/html");
          response.end('<!doctype html><body><script type="module" src="/__settings-harness.js"></script></body>');
        });
      },
    }],
  });
  try {
    await vite.listen();
    const address = vite.httpServer.address();
    const { stdout } = await promisify(execFile)(chrome, ["--headless=new", "--no-first-run", "--no-default-browser-check", "--no-sandbox", "--disable-gpu", "--disable-background-networking", `--user-data-dir=${join(temporary, "profile")}`, "--dump-dom", "--virtual-time-budget=3000", `http://127.0.0.1:${address.port}/__settings-test`], { windowsHide: true, timeout: 45000, maxBuffer: 1024 * 1024 });
    assert.equal(stdout.match(/data-result="([^"]*)"/)?.[1], "PASS", "mounted Settings browser interaction must complete");
  } finally {
    await vite.close();
    await rm(temporary, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
  }
}

test("shared MCP copies the running endpoint and token path while legacy stdio stays secondary", { skip: !existsSync(chrome) }, async () => {
  await browserSettings(`
    Object.assign(store.app, { http_enabled: true, http_port: 50002, http_active_port: 47821, token_path: "D:/DayNote/api-token.txt", cli_path: "D:/DayNote/daynote-cli.exe" });
    window.fixtureApp = JSON.parse(JSON.stringify(store.app));
    mount(Settings, { target: document.body }); await tick();
    const copyMcp = document.querySelector('[aria-label="复制共享 MCP 地址"]');
    assert(copyMcp, "shared MCP copy action missing");
    copyMcp.click(); await settle();
    assert(window.copied === "http://127.0.0.1:47821/mcp", "must copy actual running port");
    document.querySelector('[aria-label="复制令牌文件路径"]').click(); await settle();
    assert(window.copied === "D:/DayNote/api-token.txt", "must copy path, not token contents");
    const legacy = [...document.querySelectorAll("details")].find(node => node.textContent.includes("stdio"));
    assert(legacy && !legacy.open, "legacy stdio must be collapsed");
    assert(document.body.textContent.includes("完全退出"), "shared service lifetime missing");
  `);
});

test("shared MCP endpoint appears only after the service starts and disappears when disabled", { skip: !existsSync(chrome) }, async () => {
  await browserSettings(`
    Object.assign(store.app, { http_enabled: false, http_active_port: 0, http_error: null });
    window.fixtureApp = JSON.parse(JSON.stringify(store.app));
    window.startAttempts = 0;
    mount(Settings, { target: document.body }); await tick();
    const endpoint = () => document.querySelector('[aria-label="复制共享 MCP 地址"]');
    const toggle = () => document.querySelector('[aria-label="开启本地 HTTP 接口"]');
    assert(!endpoint(), "disabled service advertises MCP endpoint");
    toggle().click(); await settle();
    assert(!endpoint(), "failed start advertises MCP endpoint");
    assert(!document.body.textContent.includes("正在监听"), "failed service appears to run");
    assert(document.querySelector('[role="alert"]')?.textContent.includes("fixture: port occupied"), "start error missing");
    toggle().click(); await settle();
    assert(endpoint(), "successful start does not show MCP endpoint");
    endpoint().click(); await settle();
    assert(window.copied === "http://127.0.0.1:47821/mcp", "wrong started endpoint");
    const port = document.querySelector('[aria-label="HTTP 端口"]');
    port.value = "50001"; port.dispatchEvent(new Event("input", { bubbles: true })); await tick();
    port.closest("form").requestSubmit(); await settle();
    endpoint().click(); await settle();
    assert(window.copied === "http://127.0.0.1:50001/mcp", "changed running port not reflected");
    toggle().click(); await settle();
    assert(!endpoint(), "disabled service still advertises endpoint");
    assert(document.body.textContent.includes("MCP / HTTP 接口已关闭"), "disabled status missing");
  `, `
    if (name === "set_pref") {
      if (args.key === "http_enabled" && args.value === "true" && ++window.startAttempts === 1) throw new Error("fixture: port occupied");
      if (args.key === "http_enabled") {
        window.fixtureApp.http_enabled = args.value === "true";
        window.fixtureApp.http_active_port = window.fixtureApp.http_enabled ? window.fixtureApp.http_port : 0;
      }
      if (args.key === "http_port") {
        window.fixtureApp.http_port = Number(args.value);
        window.fixtureApp.http_active_port = Number(args.value);
      }
    }
  `);
});

test("regenerating the token tells shared MCP clients to update credentials and reconnect", { skip: !existsSync(chrome) }, async () => {
  await browserSettings(`
    Object.assign(store.app, { http_enabled: true, http_active_port: 47821 });
    window.fixtureApp = JSON.parse(JSON.stringify(store.app));
    window.tokenResets = 0;
    mount(Settings, { target: document.body }); await tick();
    [...document.querySelectorAll("button")].find(button => button.textContent === "重新生成访问令牌").click();
    await settle();
    assert(window.tokenResets === 1, "token reset command missing");
    const feedback = [...document.querySelectorAll('[role="status"]')].map(node => node.textContent).join(" ");
    assert(feedback.includes("旧令牌已失效"), "old credential invalidation missing");
    assert(feedback.includes("MCP / HTTP 客户端配置并重新连接"), "MCP credential recovery missing");
  `, `if (name === "regenerate_api_token") { window.tokenResets++; return; }`);
});

test("service startup during the initial app-state read updates the visible MCP status without stale rollback", { skip: !existsSync(chrome) }, async () => {
  await browserSettings(`
    Object.assign(store.app, { http_enabled: true, http_active_port: 0, http_error: null });
    window.fixtureApp = JSON.parse(JSON.stringify(store.app));
    window.appReads = 0;
    mount(Settings, { target: document.body }); await tick();
    await store.init(); await settle();
    const endpoint = document.querySelector('[aria-label="复制共享 MCP 地址"]');
    assert(endpoint, "startup event was lost or stale app read overwrote running state");
    endpoint.click(); await settle();
    assert(window.copied === "http://127.0.0.1:47821/mcp", "visible service status is stale");
    assert(!document.body.textContent.includes("接口未启动"), "running service still appears stopped");
  `, `
    if (name === "app_state" && ++window.appReads === 1) {
      const oldState = { ...window.fixtureApp };
      setTimeout(() => {
        window.fixtureApp.http_active_port = 47821;
        window.fixtureHandlers?.["app-state-changed"]?.();
      }, 5);
      await new Promise(resolve => setTimeout(resolve, 30));
      return oldState;
    }
  `);
});

test("opening settings rereads settled runtime state when the initial snapshot outlives a startup notification", { skip: !existsSync(chrome) }, async () => {
  await browserSettings(`
    const { default: App } = await import("/src/App.svelte");
    Object.assign(store.app, { http_enabled: true, http_active_port: 0, hotkey: "", http_error: null });
    window.fixtureApp = JSON.parse(JSON.stringify(store.app));
    window.appReads = 0;
    mount(App, { target: document.body }); await settle();
    assert(window.appReads === 1, "main window must obtain the initial runtime snapshot");
    assert(window.fixtureHandlers?.["app-state-changed"], "runtime event subscription must exist");
    // Setup can settle after the initial snapshot. Event delivery is not replayed
    // to a late/reloaded WebView, so opening settings must also fetch current state.
    window.fixtureApp.http_active_port = 47821;
    window.fixtureApp.hotkey = "Ctrl+Alt+Space";
    document.querySelector('button[title="设置"]').click(); await settle();
    assert(window.appReads === 2, "opening settings did not request current app_state");
    const endpoint = () => document.querySelector('[aria-label="复制共享 MCP 地址"]');
    assert(endpoint(), "settings kept the startup snapshot with no running service");
    assert(document.querySelector('[aria-label="速记快捷键"]').value === "Ctrl+Alt+Space", "settings kept the empty startup hotkey");
    endpoint().click(); await settle();
    assert(window.copied === "http://127.0.0.1:47821/mcp", "settings did not recover the live endpoint");
    document.querySelector('button[title="设置"]').click(); await settle();
    window.fixtureApp.http_active_port = 50001;
    document.querySelector('button[title="设置"]').click(); await settle();
    assert(window.appReads === 3, "reopening settings must refresh again");
    endpoint().click(); await settle();
    assert(window.copied === "http://127.0.0.1:50001/mcp", "reopened settings used stale runtime state");
  `, `if (name === "app_state") window.appReads++;`);
});

test("a rejected HTTP enable restores the native checkbox to the saved disabled state", { skip: !existsSync(chrome) }, async () => {
  const temporary = await mkdtemp(join(tmpdir(), "daynote-checkbox-test-"));
  const vite = await createServer({
    cacheDir: join(temporary, "cache"),
    server: { host: "127.0.0.1", port: 0, strictPort: false, hmr: { port: 0 } },
    plugins: [{
      name: "settings-checkbox-fixture", enforce: "pre",
      resolveId(id) { if (id === "/__settings-harness.js") return id; },
      load(id) {
        if (id === "/__settings-harness.js") return `
          import { mount, tick } from "svelte";
          import Settings from "/src/components/Settings.svelte";
          import { store } from "/src/lib/store.svelte.ts";
          try {
            store.app.http_enabled = false;
            window.fixtureApp = JSON.parse(JSON.stringify(store.app));
            mount(Settings, { target: document.body });
            await tick();
            const checkbox = document.querySelector('[aria-label="开启本地 HTTP 接口"]');
            checkbox.click();
            await new Promise(resolve => setTimeout(resolve, 20));
            await tick();
            if (window.requestedEnable !== "true") throw new Error("enable was not requested");
            if (store.app.http_enabled !== false) throw new Error("saved setting changed");
            if (checkbox.checked) throw new Error("checkbox stayed checked after rejected enable");
            if (!document.querySelector('[role="alert"]')?.textContent.includes("fixture: port occupied")) throw new Error("error feedback missing");
            document.body.dataset.result = "PASS";
          } catch (error) { document.body.dataset.result = "FAIL: " + error.message; }
        `;
        if (id.replaceAll("\\", "/").endsWith("/src/lib/backend.ts")) return `
          export const inTauri = false;
          export async function call() { return { series: [] }; }
          export async function cmd(name, args) {
            if (name === "set_pref") { window.requestedEnable = args.value; throw new Error("fixture: port occupied"); }
            return { ...window.fixtureApp };
          }
          export const on = async () => () => {};
          export const errorText = error => error.message;
        `;
      },
      configureServer(server) {
        server.middlewares.use((request, response, next) => {
          if (request.url !== "/__settings-test") return next();
          response.setHeader("Content-Type", "text/html");
          response.end('<!doctype html><body><script type="module" src="/__settings-harness.js"></script></body>');
        });
      },
    }],
  });
  try {
    await vite.listen();
    const address = vite.httpServer.address();
    const { stdout } = await promisify(execFile)(chrome, ["--headless=new", "--no-first-run", "--no-default-browser-check", "--no-sandbox", "--disable-gpu", "--disable-background-networking", `--user-data-dir=${join(temporary, "profile")}`, "--dump-dom", "--virtual-time-budget=3000", `http://127.0.0.1:${address.port}/__settings-test`], { windowsHide: true, timeout: 45000, maxBuffer: 1024 * 1024 });
    assert.equal(stdout.match(/data-result="([^"]*)"/)?.[1], "PASS", "browser must restore the rejected toggle and show its error");
  } finally {
    await vite.close();
    await rm(temporary, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
  }
});
