import assert from "node:assert/strict";
import { test } from "node:test";
import { createServer } from "vite";
import { mkdtemp, rm } from "node:fs/promises";
import { existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { execFile } from "node:child_process";
import { promisify } from "node:util";

const chrome = process.env.CHROME_BIN || "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe";

test("main-window update actions show progress/errors, preserve drafts, and block editing before restart", { skip: !existsSync(chrome) }, async () => {
  const temporary = await mkdtemp(join(tmpdir(), "daynote-update-dom-"));
  const vite = await createServer({
    cacheDir: join(temporary, "cache"),
    server: { host: "127.0.0.1", port: 0, strictPort: false, hmr: { port: 0 } },
    plugins: [{
      name: "update-dom-fixture", enforce: "pre",
      resolveId(id) { if (id === "/__update-harness.js") return id; },
      load(id) {
        if (id === "/__update-harness.js") return `
          import { mount, tick } from "svelte";
          import App from "/src/App.svelte";
          import { store } from "/src/lib/store.svelte.ts";
          import { updater } from "/src/lib/updater.svelte.ts";
          const assert = (ok, message) => { if (!ok) throw new Error(message); };
          const settle = async () => { await new Promise(resolve => setTimeout(resolve, 20)); await tick(); };
          const action = (text) => [...document.querySelectorAll('button')].find(b => b.textContent.trim() === text);
          try {
            window.__TAURI_INTERNALS__ = { transformCallback: () => 1 };
            window.fixtureApp = { ...store.app, version: "0.1.0" };
            window.requests = [];
            window.notes = new Map();
            mount(App, { target: document.body });
            await settle();
            store.view = "settings";
            await tick();
            assert(document.body.textContent.includes("当前版本 0.1.0"), "current version missing in settings");
            assert(action("检查更新"), "settings check button missing");
            window.failCheck = true;
            action("检查更新").click();
            await settle();
            assert(document.querySelector('[role="alert"]')?.textContent.includes("offline"), "check error missing");
            window.failCheck = false;
            window.latest = true;
            action("重新检查").click();
            await settle();
            assert(document.body.textContent.includes("已是最新版本"), "latest-version feedback missing");
            window.latest = false;
            action("检查更新").click();
            await settle();
            assert(document.querySelector('[role="dialog"]'), "update dialog missing");
            assert(action("稍后") && action("下载并安装"), "explicit update choices missing");
            assert(!window.requests.includes("download_update"), "discovery downloaded without confirmation");
            action("稍后").click();
            await tick();
            await updater.check();
            await tick();
            assert(!document.querySelector('[role="dialog"]'), "deferred version reappeared automatically");
            action("检查更新").click();
            await settle();
            store.editNote("preserve me before restart");
            window.failSave = true;
            window.failDownload = true;
            action("下载并安装").click();
            await settle();
            assert(document.querySelector('[role="dialog"] [role="alert"]')?.textContent.includes("download interrupted"), "download error missing");
            window.failDownload = false;
            action("重试下载并安装").click();
            await tick();
            assert(document.querySelector('progress')?.value === 25, "download progress missing");
            assert(action("稍后").disabled, "busy update can be dismissed");
            assert(document.body.textContent.includes("25%"), "download percentage missing");
            window.finishDownload();
            await settle();
            assert(!window.requests.includes("install_update"), "failed note save still installed");
            assert(document.querySelector('[role="dialog"] [role="alert"]')?.textContent.includes("便签尚未全部保存"), "note-save error missing");
            assert(store.note === "preserve me before restart", "failed save lost draft");
            window.failSave = false;
            action("重试下载并安装").click();
            await settle();
            assert(window.notes.get(store.date) === "preserve me before restart", "retry did not save draft");
            assert(updater.phase === "installing", "install stage missing");
            assert(document.querySelector('main').inert, "main window is editable during install");
            assert(document.body.textContent.includes("关闭并重新启动"), "restart warning missing");
            document.body.dataset.result = "PASS";
          } catch (error) { document.body.dataset.result = "FAIL: " + error.message; }
        `;
        if (id.replaceAll("\\", "/").endsWith("/src/lib/backend.ts")) return `
          export const inTauri = true;
          export async function call(tool, args) {
            if (tool === "set_day_note") {
              if (window.failSave) throw new Error("database busy");
              window.notes.set(args.date, args.content);
            }
            if (tool === "get_day_note") return { content: window.notes.get(args.date) ?? "" };
            return { tasks: [], series: [] };
          }
          export async function cmd(name, args) {
            window.requests.push(name);
            if (name === "app_state") return { ...window.fixtureApp };
            if (name === "check_update") {
              if (window.failCheck) throw new Error("offline");
              return window.latest ? null : { version: "0.1.1", current_version: "0.1.0", body: "Update fixture" };
            }
            if (name === "download_update") {
              if (window.failDownload) throw new Error("download interrupted");
              args.progress.onmessage({ downloaded: 25, total: 100 });
              await new Promise(resolve => { window.finishDownload = resolve; });
            }
            if (name === "install_update") await new Promise(() => {});
          }
          export const on = async () => () => {};
          export const errorText = e => String(e.message ?? e);
        `;
      },
      configureServer(server) {
        server.middlewares.use((request, response, next) => {
          if (request.url !== "/__update-test") return next();
          response.setHeader("Content-Type", "text/html");
          response.end('<!doctype html><body><script type="module" src="/__update-harness.js"></script></body>');
        });
      },
    }],
  });
  try {
    await vite.listen();
    const address = vite.httpServer.address();
    const { stdout } = await promisify(execFile)(chrome, ["--headless=new", "--no-sandbox", "--disable-gpu", "--disable-background-networking", `--user-data-dir=${join(temporary, "profile")}`, "--dump-dom", "--virtual-time-budget=5000", `http://127.0.0.1:${address.port}/__update-test`], { windowsHide: true, timeout: 25000, maxBuffer: 1024 * 1024 });
    assert.equal(stdout.match(/data-result="([^"]*)"/)?.[1], "PASS", "browser update workflow must expose safe, actionable states");
  } finally {
    await vite.close();
    await rm(temporary, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
  }
});
