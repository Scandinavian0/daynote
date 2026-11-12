// State of the main window: which day is shown, its tasks, and every action the UI can take.
// After any change the day is simply re-read from the backend, so linked rules
// (sub-task roll-up, recurring instances, edits made by an AI) always show their real result.

import type { AppState, DayCount, Series, Summary, Task } from "./types.ts";
import { call, cmd, errorText, on } from "./backend.ts";
import { addDays, monthOf, monthRange, todayStr } from "./dates.ts";
import { parseQuick } from "./quickparse.ts";

interface Toast {
  text: string;
  undo?: () => void;
}

const defaultApp: AppState = {
  expanded: false,
  pinned: false,
  autostart: false,
  hotkey: "",
  morning_enabled: true,
  morning_time: "09:00",
  evening_enabled: true,
  evening_time: "18:00",
  sound: true,
  http_enabled: true,
  http_port: 47821,
  http_active_port: 0,
  http_error: null,
  token_path: "",
  data_dir: "",
  next_data_dir: "",
  data_dir_error: null,
  data_dir_overridden: false,
  cli_path: "",
  version: "",
  glass: true,
};

class Store {
  today = $state(todayStr());
  date = $state(todayStr());
  tasks = $state<Task[]>([]);
  overdue = $state<Task[]>([]);
  note = $state("");
  calMonth = $state(monthOf(todayStr()));
  days = $state<Record<string, DayCount>>({});
  monthTotal = $state({ total: 0, done: 0 });
  series = $state<Series[]>([]);
  app = $state<AppState>(defaultApp);
  ready = $state(false);
  view = $state<"list" | "settings">("list");
  openId = $state<number | null>(null);
  toast = $state<Toast | null>(null);

  open = $derived(this.tasks.filter((t) => t.status === "todo"));
  done = $derived(this.tasks.filter((t) => t.status === "done"));
  isToday = $derived(this.date === this.today);

  #seq = 0;
  #noteTimer: ReturnType<typeof setTimeout> | undefined;
  #noteVersion = 0;
  #noteDrafts = new Map<string, { content: string; version: number }>();
  #noteSaves = new Map<string, Promise<void>>();
  #toastTimer: ReturnType<typeof setTimeout> | undefined;

  async init() {
    await this.loadApp();
    await on("data-changed", () => this.refresh());
    await on("app-state-changed", () => this.loadApp());
    await on<{ date?: string }>("navigate", (p) => {
      this.view = "list";
      this.go(p?.date ?? this.today);
    });
    // Follow the calendar across midnight (and after the PC wakes from sleep).
    setInterval(() => {
      const now = todayStr();
      if (now === this.today) return;
      const wasToday = this.isToday;
      this.today = now;
      if (wasToday) this.go(now);
      else this.refresh();
    }, 30_000);
    await this.refresh();
    this.ready = true;
  }

  async loadApp() {
    try {
      this.app = await cmd<AppState>("app_state");
    } catch (e) {
      this.fail(e);
    }
  }

  async refresh() {
    const seq = ++this.#seq;
    const date = this.date;
    const noteVersion = this.#noteVersion;
    const draftAtRead = this.#noteDrafts.get(date);
    try {
      const [day, late, note] = await Promise.all([
        call<{ today: string; tasks: Task[] }>("list_tasks", { date }),
        call<{ tasks: Task[] }>("list_tasks", { status: "overdue" }),
        call<{ content: string }>("get_day_note", { date }),
      ]);
      if (seq !== this.#seq) return;
      this.tasks = day.tasks;
      this.overdue = late.tasks;
      const draft = this.#noteDrafts.get(date);
      if (draft) this.note = draft.content;
      else if (noteVersion === this.#noteVersion) this.note = draftAtRead?.content ?? note.content;
      if (this.app.expanded) this.loadMonth();
    } catch (e) {
      this.fail(e);
    }
  }

  async loadMonth() {
    const ym = this.calMonth;
    try {
      const s = await call<Summary>("get_summary", monthRange(ym));
      if (ym !== this.calMonth) return;
      this.days = Object.fromEntries(s.days.map((d) => [d.date, d]));
      this.monthTotal = { total: s.total, done: s.done };
    } catch (e) {
      this.fail(e);
    }
  }

  async loadSeries() {
    try {
      this.series = (await call<{ series: Series[] }>("list_recurring")).series;
    } catch (e) {
      this.fail(e);
    }
  }

  // Navigation ------------------------------------------------------------

  go(date: string) {
    this.flushNote();
    this.date = date;
    this.note = this.#noteDrafts.get(date)?.content ?? "";
    this.openId = null;
    if (monthOf(date) !== this.calMonth) this.calMonth = monthOf(date);
    this.refresh();
  }
  step(n: number) {
    this.go(addDays(this.date, n));
  }
  setMonth(ym: string) {
    this.calMonth = ym;
    this.loadMonth();
  }

  // Tasks -----------------------------------------------------------------

  async #run(tool: string, args: Record<string, unknown>) {
    try {
      const result = await call(tool, args);
      await this.refresh();
      return result;
    } catch (e) {
      this.fail(e);
      await this.refresh();
      return null;
    }
  }

  /** Add one typed line; "明天 15:00 …" style prefixes set the day and the reminder. */
  async add(text: string): Promise<boolean> {
    const parsed = parseQuick(text, this.date, new Date());
    if (!parsed.title) return false;
    const ok = await this.#run("add_tasks", {
      date: parsed.date,
      tasks: [{ title: parsed.title, remind_at: parsed.remind_at }],
    });
    if (ok && parsed.date !== this.date) this.notify(`已记到 ${parsed.hint || parsed.date}`);
    return !!ok;
  }

  toggle(task: Task) {
    const done = task.status !== "done";
    task.status = done ? "done" : "todo"; // show it at once; refresh brings the real result
    return this.#run("complete_task", { id: task.id, done });
  }

  update(id: number, patch: Record<string, unknown>) {
    return this.#run("update_task", { id, ...patch });
  }

  addSubtask(parentId: number, title: string) {
    return this.#run("add_subtasks", { parent_id: parentId, subtasks: [title] });
  }

  move(ids: number[], date: string) {
    if (ids.length) return this.#run("move_tasks", { ids, date });
  }

  reorder(ids: number[]) {
    return this.#run("reorder_tasks", { date: this.date, ids });
  }

  async remove(task: Task) {
    if (this.openId === task.id) this.openId = null;
    const ok = await this.#run("delete_task", { id: task.id });
    if (ok) this.notify(`已删除「${task.title}」`, () => this.#run("restore_task", { id: task.id }));
  }

  // Day note --------------------------------------------------------------

  editNote(text: string) {
    this.note = text;
    const date = this.date;
    this.#noteDrafts.set(date, { content: text, version: ++this.#noteVersion });
    clearTimeout(this.#noteTimer);
    this.#noteTimer = setTimeout(() => this.flushNote(date), 600);
  }

  flushNote(date = this.date): Promise<void> {
    clearTimeout(this.#noteTimer);
    if (!this.#noteDrafts.has(date)) return Promise.resolve();
    // Queue writes per day: an older, slower save must never overwrite a newer draft.
    const previous = this.#noteSaves.get(date) ?? Promise.resolve();
    const save = previous.then(async () => {
      const draft = this.#noteDrafts.get(date);
      if (!draft) return;
      try {
        await call("set_day_note", { date, content: draft.content });
        if (this.#noteDrafts.get(date)?.version === draft.version) this.#noteDrafts.delete(date);
      } catch (e) {
        // Keep the dated draft across refresh/navigation; the next flush retries it.
        this.fail(e);
      }
    });
    this.#noteSaves.set(date, save);
    void save.finally(() => {
      if (this.#noteSaves.get(date) === save) this.#noteSaves.delete(date);
    });
    return save;
  }

  /** Drain dated drafts before an update may close the app. Failed writes keep their draft. */
  async prepareForUpdate(): Promise<boolean> {
    clearTimeout(this.#noteTimer);
    const dates = new Set([...this.#noteDrafts.keys(), ...this.#noteSaves.keys()]);
    await Promise.all([...dates].map((date) => this.flushNote(date)));
    return this.#noteDrafts.size === 0 && this.#noteSaves.size === 0;
  }

  // Recurring tasks ---------------------------------------------------------

  async addSeries(args: Record<string, unknown>) {
    await this.#run("add_recurring", args);
    await this.loadSeries();
  }
  async updateSeries(id: number, patch: Record<string, unknown>) {
    await this.#run("update_recurring", { id, ...patch });
    await this.loadSeries();
  }
  async removeSeries(id: number) {
    await this.#run("delete_recurring", { id });
    await this.loadSeries();
  }

  // Window & preferences ----------------------------------------------------

  async shell(name: string, args: Record<string, unknown> = {}) {
    try {
      await cmd(name, args);
      await this.loadApp();
      return true;
    } catch (e) {
      this.fail(e);
      await this.loadApp();
      return false;
    }
  }

  async setExpanded(expanded: boolean) {
    await this.shell("set_expanded", { expanded });
    if (expanded) this.loadMonth();
  }

  // Feedback --------------------------------------------------------------

  notify(text: string, undo?: () => void) {
    clearTimeout(this.#toastTimer);
    this.toast = { text, undo };
    this.#toastTimer = setTimeout(() => (this.toast = null), undo ? 6000 : 3000);
  }
  fail(e: unknown) {
    this.notify(errorText(e));
  }
  dismissToast() {
    clearTimeout(this.#toastTimer);
    this.toast = null;
  }
}

export const store = new Store();
