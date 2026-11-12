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
