import assert from "node:assert/strict";
import { test } from "node:test";
import { createServer } from "vite";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { existsSync } from "node:fs";

const chrome = process.env.CHROME_BIN || "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe";

test("reminder subscribes before loading, ignores stale reads and hides an empty stack", { skip: !existsSync(chrome) }, async () => {
  const temporary = await mkdtemp(join(tmpdir(), "daynote-reminder-test-"));
  const vite = await createServer({
    cacheDir: join(temporary, "cache"),
    server: { host: "127.0.0.1", port: 0, hmr: { port: 0 } },
    plugins: [{
      name: "reminder-fixture", enforce: "pre",
      resolveId(id) { if (id === "/__reminder-test.js") return id; },
      load(id) {
        if (id.replaceAll("\\", "/").endsWith("/src/lib/backend.ts")) return `
          export const inTauri = true;
          export const on = async (_, handler) => {
            if (window.failListen) throw new Error('fixture: listener registration failed');
            // A reminder arrives while the asynchronous native subscription is pending.
            setTimeout(() => { window.queue = [window.firstCard]; }, 20);
            await new Promise(r => setTimeout(r, 40));
            window.handler = handler; window.subscribed = true; return () => {};
          };
          export const cmd = async (name, args) => {
            if (name === 'reminder_resize') {
              if (window.holdHeight && !window.heldHeight) {
                window.heldHeight = true; await new Promise(r => window.releaseHeight = r);
              }
              window.heights.push(args.height); return;
            }
            if (name === 'reminder_cards') {
              if (window.delay) return new Promise(resolve => window.oldRead = resolve);
              return window.queue;
            }
          };
        `;
        if (id === "/__reminder-test.js") return `
          import { mount, unmount, tick } from 'svelte';
          import Reminder from '/src/ReminderApp.svelte';
          const assert = (ok, message) => { if (!ok) throw new Error(message); };
          const settle = async () => { await new Promise(r => setTimeout(r, 80)); await tick(); };
          const card = title => ({ id: title, title, kind: 'task', subtitle: '', tasks: [] });
          window.heights = []; window.queue = []; window.firstCard = card('First reminder');
          try {
            const instance = mount(Reminder, { target: document.body }); await settle();
            assert(document.body.textContent.includes('First reminder'), 'initial reminder was lost during subscription');
            window.delay = true; window.handler(); await settle();
            window.delay = false; window.queue = [card('Latest reminder')]; window.handler(); await settle();
            window.oldRead([card('Stale reminder')]); await settle();
            assert(document.body.textContent.includes('Latest reminder') && !document.body.textContent.includes('Stale reminder'), 'stale read replaced the live queue');
            const stack = document.querySelector('.stack');
            stack.style.width = '200px';
            [...document.querySelectorAll('button')].find(b => b.textContent === '稍后').click(); await settle();
            assert(window.heights.at(-1) === Math.ceil(stack.getBoundingClientRect().height), 'snooze choices are clipped after wrapping');
            window.holdHeight = true; window.queue = []; window.handler(); await settle();
            assert(window.releaseHeight, 'empty-stack resize did not reach the backend');
            window.queue = [card('Arrived while hiding')]; window.handler(); await settle();
            window.releaseHeight(); await settle();
            assert(window.heights.at(-1) > 0, 'an old empty-stack resize hid a newer reminder');
            window.queue = []; window.handler(); await settle();
            assert(window.heights.at(-1) === 0, 'empty transparent padding still covers the desktop');
            await unmount(instance);
            window.failListen = true; window.queue = [card('Listener fallback')];
            mount(Reminder, { target: document.body }); await settle();
            assert(document.body.textContent.includes('Listener fallback'), 'listener failure prevented the initial read');
            document.body.dataset.result = 'PASS';
          } catch (error) { document.body.dataset.result = 'FAIL: ' + error.message; }
        `;
      },
      configureServer(server) {
        server.middlewares.use((request, response, next) => {
          if (request.url !== "/__reminder-test") return next();
          response.setHeader("Content-Type", "text/html");
          response.end('<!doctype html><body><script type="module" src="/__reminder-test.js"></script></body>');
        });
      },
    }],
  });
  try {
    await vite.listen();
    const { stdout } = await promisify(execFile)(chrome, ["--headless=new", "--no-first-run", "--no-default-browser-check", "--no-sandbox", "--disable-gpu", "--disable-background-networking", `--user-data-dir=${join(temporary, "profile")}`, "--dump-dom", "--virtual-time-budget=3000", `http://127.0.0.1:${vite.httpServer.address().port}/__reminder-test`], { windowsHide: true, timeout: 45000, maxBuffer: 1024 * 1024 });
    assert.equal(stdout.match(/data-result="([^"]*)"/)?.[1], "PASS");
  } finally {
    await vite.close();
    await rm(temporary, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
  }
});
