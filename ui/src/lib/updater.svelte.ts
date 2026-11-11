import { cmd, errorText, inTauri } from "./backend.ts";
import { Channel } from "@tauri-apps/api/core";
import { store } from "./store.svelte.ts";

export interface UpdateInfo {
  version: string;
  current_version: string;
  body: string | null;
}

interface UpdateProgress {
  downloaded: number;
  total: number | null;
}

class Updater {
  update = $state<UpdateInfo | null>(null);
  dialogOpen = $state(false);
  phase = $state<"idle" | "checking" | "available" | "latest" | "unsupported" | "downloading" | "preparing" | "installing" | "error">("idle");
  error = $state("");
  downloaded = $state(0);
  total = $state<number | null>(null);
  busy = $derived(["checking", "downloading", "preparing", "installing"].includes(this.phase));
  locked = $derived(this.phase === "preparing" || this.phase === "installing");
  #deferredVersion: string | null = null;
  #downloadReady = false;

  /** Only the main desktop window starts these checks. Browser previews stay offline. */
  start(): () => void {
    if (!inTauri) return () => {};
    const startup = setTimeout(() => this.check(), 3000);
    const recurring = setInterval(() => this.check(), 6 * 60 * 60 * 1000);
    return () => { clearTimeout(startup); clearInterval(recurring); };
  }

  defer() {
    if (this.busy) return;
    this.#deferredVersion = this.update?.version ?? null;
    this.dialogOpen = false;
  }

  async check(manual = false) {
    if (!inTauri) { this.phase = "unsupported"; return; }
    if (this.busy) return;
    this.phase = "checking";
    this.error = "";
    try {
      this.update = await cmd<UpdateInfo | null>("check_update");
      this.#downloadReady = false;
      this.phase = this.update ? "available" : "latest";
      this.dialogOpen = this.update !== null && (manual || this.update.version !== this.#deferredVersion);
    } catch (e) {
      this.error = errorText(e);
      this.phase = "error";
    }
  }

  /** Called only by the explicit download/install action. Native code verifies the package. */
  async install() {
    if (!inTauri || !this.update || this.busy || !this.dialogOpen) return;
    this.error = "";
    try {
      if (!this.#downloadReady) {
        this.phase = "downloading";
        this.downloaded = 0;
        this.total = null;
        const progress = new Channel<UpdateProgress>((value) => {
          if (this.phase !== "downloading") return;
          this.downloaded = value.downloaded;
          this.total = value.total;
        });
        await cmd("download_update", { progress });
        this.#downloadReady = true;
      }
      this.phase = "preparing";
      if (!(await store.prepareForUpdate())) {
        throw new Error("便签尚未全部保存，已停止安装。草稿已保留，请重试或稍后更新。");
      }
      this.phase = "installing";
      await cmd("install_update");
      // A successful native install exits this process; keep editing blocked until it does.
    } catch (e) {
      if (this.phase === "installing") this.#downloadReady = false;
      this.error = errorText(e);
      this.phase = "error";
    }
  }
}

export const updater = new Updater();
