import assert from "node:assert/strict";
import { test } from "node:test";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createServer } from "vite";

async function withStore(run) {
  const notes = new Map();
  const writes = [];
  let failNext = false;
  let nextReadGate;
  let interval;
  const originalInterval = globalThis.setInterval;
  const originalTimeout = globalThis.setTimeout;
  const originalClear = globalThis.clearTimeout;
  const fixture = {
    async call(tool, args) {
      if (tool === "set_day_note") {
        writes.push({ ...args });
        if (failNext) { failNext = false; throw new Error("database busy"); }
        notes.set(args.date, args.content);
        return {};
      }
      if (tool === "get_day_note") {
        const content = notes.get(args.date) ?? "";
        const gate = nextReadGate;
        nextReadGate = undefined;
        if (gate) await gate;
        return { content };
      }
      return { tasks: [], today: "2026-11-11" };
    },
    async cmd() { return { expanded: false }; },
  };
  globalThis.__daynoteNoteTest = fixture;
  const cacheDir = await mkdtemp(join(tmpdir(), "daynote-note-test-"));
  const vite = await createServer({
    cacheDir,
    server: { middlewareMode: true, hmr: false }, appType: "custom",
    plugins: [{
      name: "note-test-backend", enforce: "pre",
      load(id) {
        if (id.replaceAll("\\", "/").endsWith("/src/lib/backend.ts")) {
          return `export const call = (...args) => globalThis.__daynoteNoteTest.call(...args);
            export const cmd = (...args) => globalThis.__daynoteNoteTest.cmd(...args);
            export const on = async () => () => {};
            export const errorText = (e) => String(e.message ?? e);`;
        }
      },
    }],
  });
  try {
    const { store } = await vite.ssrLoadModule("/src/lib/store.svelte.ts");
    // Only the store's timers are intercepted; Vite's own startup/cleanup use real timers.
    globalThis.setInterval = (callback) => { interval = callback; return 1; };
    globalThis.setTimeout = () => 1;
    globalThis.clearTimeout = () => {};
    await store.init();
    await run({
      store, notes, writes, failOnce: () => { failNext = true; }, midnight: () => interval(),
      holdNextRead: () => {
        let release;
        nextReadGate = new Promise((resolve) => { release = resolve; });
        return release;
      },
    });
  } finally {
    globalThis.setInterval = originalInterval;
    globalThis.setTimeout = originalTimeout;
    globalThis.clearTimeout = originalClear;
    delete globalThis.__daynoteNoteTest;
    await vite.close();
    await rm(cacheDir, { recursive: true, force: true });
  }
}

test("a pending day note stays on its original date when midnight advances the view", async () => {
  await withStore(async ({ store, notes, writes, midnight }) => {
    store.today = "2026-11-10";
    store.date = "2026-11-10";
    store.editNote("yesterday's draft");
    midnight();
    await store.flushNote();
    await new Promise((resolve) => setImmediate(resolve));
    assert.equal(notes.get("2026-11-10"), "yesterday's draft");
    assert.equal(writes.some((w) => w.date !== "2026-11-10"), false);
  });
});

test("a failed note write survives refresh and navigation and can be retried", async () => {
  await withStore(async ({ store, notes, failOnce }) => {
    const date = store.date;
    store.editNote("unsaved draft");
    failOnce();
    await store.flushNote();
    await store.refresh();
    assert.equal(store.note, "unsaved draft");
    store.go("2026-11-12");
    await store.refresh();
    store.go(date);
    await store.refresh();
    assert.equal(store.note, "unsaved draft");
    await store.flushNote();
    assert.equal(notes.get(date), "unsaved draft");
  });
});

test("an older refresh cannot overwrite a newer note after its save succeeds", async () => {
  await withStore(async ({ store, holdNextRead }) => {
    store.editNote("first draft");
    const release = holdNextRead();
    const stale = store.refresh();
    store.editNote("newer draft");
    await store.flushNote();
    release();
    await stale;
    assert.equal(store.note, "newer draft");
  });
});
