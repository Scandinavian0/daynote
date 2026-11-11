import assert from "node:assert/strict";
import { test } from "node:test";
import { createServer } from "vite";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

const update = { version: "0.1.1", current_version: "0.1.0", body: "改进自动更新" };

async function withUpdater(run, desktop = true) {
  const requests = [];
  const notes = new Map();
  let failSave = false;
  let failCheck = false;
  let failDownload = false;
  let releaseDownload;
  let progress;
  let checkResult = update;
  const originalWindow = globalThis.window;
  globalThis.window = { __TAURI_INTERNALS__: { transformCallback: () => 1 } };
  globalThis.__daynoteUpdaterTest = {
    desktop,
    async call(tool, args) {
      if (tool === "set_day_note") {
        if (failSave) throw new Error("database busy");
        notes.set(args.date, args.content);
      }
      if (tool === "get_day_note") return { content: notes.get(args.date) ?? "" };
      return { tasks: [], today: "2026-11-11" };
    },
    async cmd(name, args) {
      requests.push(name);
      if (name === "check_update") {
        if (failCheck) throw new Error("offline");
        return checkResult;
      }
      if (name === "download_update") {
        progress = args.progress.onmessage;
        if (failDownload) throw new Error("download interrupted");
        await new Promise((resolve) => { releaseDownload = resolve; });
      }
      return null;
    },
  };
  const temporary = await mkdtemp(join(tmpdir(), "daynote-updater-test-"));
  const vite = await createServer({
    cacheDir: temporary, server: { middlewareMode: true, hmr: false }, appType: "custom",
    plugins: [{ name: "updater-native-boundary", enforce: "pre", load(id) {
      if (id.replaceAll("\\", "/").endsWith("/src/lib/backend.ts")) return `
        export const inTauri = globalThis.__daynoteUpdaterTest.desktop;
        export const call = (...args) => globalThis.__daynoteUpdaterTest.call(...args);
        export const cmd = (...args) => globalThis.__daynoteUpdaterTest.cmd(...args);
        export const on = async () => () => {};
        export const errorText = e => String(e.message ?? e);
      `;
    } }],
  });
  try {
    const { updater } = await vite.ssrLoadModule("/src/lib/updater.svelte.ts");
    const { store } = await vite.ssrLoadModule("/src/lib/store.svelte.ts");
    await run({
      updater, store, requests, notes,
      failSave: (value) => { failSave = value; },
      failCheck: (value) => { failCheck = value; },
      failDownload: (value) => { failDownload = value; },
      checkResult: (value) => { checkResult = value; },
      progress: (value) => progress(value),
      finishDownload: () => releaseDownload(),
    });
  } finally {
    globalThis.window = originalWindow;
    delete globalThis.__daynoteUpdaterTest;
    await vite.close();
    await rm(temporary, { recursive: true, force: true });
  }
}

test("discovering a new update only offers it, without downloading or installing", async () => {
  await withUpdater(async ({ updater, requests }) => {
    await updater.check();
    assert.equal(updater.update.version, "0.1.1");
    assert.equal(updater.dialogOpen, true);
    assert.deepEqual(requests, ["check_update"]);
  });
});

test("later suppresses the same version for automatic checks but manual checks offer it again", async () => {
  await withUpdater(async ({ updater, requests }) => {
    await updater.check();
    updater.defer();
    assert.equal(updater.dialogOpen, false);
    await updater.check();
    assert.equal(updater.dialogOpen, false);
    await updater.check(true);
    assert.equal(updater.dialogOpen, true);
    assert.equal(requests.includes("download_update"), false);
  });
});

test("only confirmation downloads, reports progress, and prevents duplicate update operations", async () => {
  await withUpdater(async ({ updater, requests, progress, finishDownload }) => {
    await updater.check();
    const installing = updater.install();
    assert.equal(updater.phase, "downloading");
    progress({ downloaded: 25, total: 100 });
    assert.equal(updater.downloaded, 25);
    assert.equal(updater.total, 100);
    await updater.install();
    await updater.check(true);
    updater.defer();
    assert.equal(updater.dialogOpen, true);
    assert.equal(requests.filter((name) => name === "download_update").length, 1);
    finishDownload();
    await installing;
    assert.equal(updater.phase, "installing");
    assert.deepEqual(requests, ["check_update", "download_update", "install_update"]);
  });
});

test("a failed dated note save blocks installation, preserves the draft and can be retried", async () => {
  await withUpdater(async ({ updater, store, requests, notes, failSave, finishDownload }) => {
    await updater.check();
    const date = store.date;
    store.editNote("draft must survive update failure");
    failSave(true);
    const first = updater.install();
    finishDownload();
    await first;
    assert.equal(requests.includes("install_update"), false);
    assert.equal(updater.phase, "error");
    assert.match(updater.error, /便签.*保存/);
    await store.refresh();
    assert.equal(store.note, "draft must survive update failure");
    failSave(false);
    await updater.install();
    assert.equal(notes.get(date), "draft must survive update failure");
    assert.equal(requests.filter((name) => name === "download_update").length, 1);
    assert.equal(requests.includes("install_update"), true);
    store.dismissToast();
  });
});

test("failed checks and downloads expose errors and allow an explicit retry", async () => {
  await withUpdater(async ({ updater, requests, failCheck, failDownload, finishDownload, checkResult }) => {
    failCheck(true);
    await updater.check(true);
    assert.equal(updater.phase, "error");
    assert.equal(updater.error, "offline");
    failCheck(false);
    await updater.check(true);
    failDownload(true);
    await updater.install();
    assert.equal(updater.error, "download interrupted");
    assert.equal(updater.busy, false);
    assert.equal(requests.includes("install_update"), false);
    failDownload(false);
    const retry = updater.install();
    finishDownload();
    await retry;
    assert.equal(updater.phase, "installing");
  });
});

test("desktop startup checks after three seconds and every six hours, with cleanup", async () => {
  await withUpdater(async ({ updater, requests, checkResult }) => {
    const original = { setTimeout, clearTimeout, setInterval, clearInterval };
    let startup;
    let recurring;
    const cancelled = [];
    try {
      checkResult(null);
      globalThis.setTimeout = (fn, ms) => { assert.equal(ms, 3000); startup = fn; return 11; };
      globalThis.setInterval = (fn, ms) => { assert.equal(ms, 21600000); recurring = fn; return 12; };
      globalThis.clearTimeout = (id) => cancelled.push(id);
      globalThis.clearInterval = (id) => cancelled.push(id);
      const stop = updater.start();
      assert.deepEqual(requests, []);
      await startup();
      assert.equal(updater.phase, "latest");
      await recurring();
      assert.deepEqual(requests, ["check_update", "check_update"]);
      stop();
      assert.deepEqual(cancelled, [11, 12]);
    } finally { Object.assign(globalThis, original); }
  });
});

test("browser previews never auto-check and manual checks explain desktop-only support", async () => {
  await withUpdater(async ({ updater, requests }) => {
    updater.start()();
    await updater.check(true);
    assert.equal(updater.phase, "unsupported");
    assert.equal(updater.dialogOpen, false);
    assert.deepEqual(requests, []);
  }, false);
});
