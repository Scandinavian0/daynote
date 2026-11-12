// Dev-only stand-in for the Tauri shell, loaded when the UI runs in a plain browser.
// It imitates daynote-core closely enough to preview and click through the interface;
// the real rules live in crates/core.

import type { AppState, Card, Series, Task } from "./types.ts";
import { addDays, fmtDateTime, todayStr } from "./dates.ts";

const today = todayStr();
let nextId = 1;
const stamp = () => fmtDateTime(new Date());

function make(date: string, title: string, extra: Partial<Task> = {}): Task {
  return {
    id: nextId++,
    date,
    title,
    note: "",
    status: "todo",
    completed_at: null,
    completed_by: null,
    remind_at: null,
    snooze_until: null,
    reminded_at: null,
    priority: 0,
    sort_order: nextId,
    parent_id: null,
    series_id: null,
    source: "user",
    created_at: stamp(),
    updated_at: stamp(),
    ...extra,
  };
}

let tasks: Task[] = [];
let deleted: Task[] = [];
const notes: Record<string, string> = {
  [today]: "周五前准备项目演示。\n先整理提纲，再补充示例。",
};
let series: Series[] = [
  { id: 1, title: "处理收件箱", note: "", rule: "weekdays", remind_time: "09:30", start_date: today, end_date: null, active: true, source: "user" },
];

function seed() {
  const done = { status: "done" as const, completed_at: stamp(), completed_by: "user" };
  tasks.push(make(today, "处理收件箱", { remind_at: `${today}T09:30:00`, reminded_at: stamp(), series_id: 1, ...done }));
  tasks.push(make(today, "核对项目检查清单", { priority: 1, note: "先处理三个待确认的问题", ...done }));
  const plan = make(today, "准备项目演示", { source: "claude-code", remind_at: `${today}T15:00:00` });
  tasks.push(plan);
  tasks.push(make(today, "整理提纲", { parent_id: plan.id, source: "claude-code", ...done }));
  tasks.push(make(today, "制作演示材料", { parent_id: plan.id, source: "claude-code" }));
  tasks.push(make(today, "检查示例", { parent_id: plan.id, source: "claude-code" }));
  tasks.push(make(today, "练习演讲", { source: "codex", remind_at: `${today}T20:00:00` }));
  tasks.push(make(today, "确认下次会议时间"));
  tasks.push(make(addDays(today, -1), "整理上周的跟进表"));
  tasks.push(make(addDays(today, -3), "更新项目说明 PDF", { source: "codex" }));
  tasks.push(make(addDays(today, 1), "复盘本周数据", { remind_at: `${addDays(today, 1)}T10:00:00` }));
  tasks.push(make(addDays(today, -2), "寄出资料", done));
  tasks.push(make(addDays(today, -2), "对账", done));
}
seed();

const top = (t: Task) => t.parent_id === null;
const withSubs = (t: Task): Task => ({ ...t, subtasks: tasks.filter((s) => s.parent_id === t.id).map((s) => ({ ...s })) });
const sorted = (list: Task[]) => [...list].sort((a, b) => a.date.localeCompare(b.date) || a.sort_order - b.sort_order);
const env = <T extends object>(body: T) => ({ today, now: stamp(), ...body });
const find = (id: unknown) => {
  const t = tasks.find((x) => x.id === Number(id));
  if (!t) throw { code: "not_found", message: `找不到任务 ${id}` };
  return t;
};

const listeners: Record<string, Set<(p: unknown) => void>> = {};
function emit(event: string, payload?: unknown) {
  listeners[event]?.forEach((fn) => fn(payload));
}

export function mockOn(event: string, handler: (p: unknown) => void): () => void {
  (listeners[event] ??= new Set()).add(handler);
  return () => listeners[event].delete(handler);
}

function setDone(t: Task, done: boolean) {
  t.status = done ? "done" : "todo";
  t.completed_at = done ? stamp() : null;
  t.completed_by = done ? "user" : null;
}

export async function mockCall(tool: string, a: Record<string, any>): Promise<unknown> {
  switch (tool) {
    case "list_tasks": {
      let list = tasks.filter(top);
      if (a.status === "overdue") list = list.filter((t) => t.date < today && t.status === "todo");
      else {
        const from = a.from ?? a.date ?? today;
        const to = a.to ?? a.date ?? today;
        list = list.filter((t) => t.date >= from && t.date <= to);
        if (a.status === "todo" || a.status === "done") list = list.filter((t) => t.status === a.status);
      }
      return env({ tasks: sorted(list).map(withSubs) });
    }
    case "add_tasks": {
      const date = a.date ?? today;
      const created: Task[] = [];
      for (const n of a.tasks ?? []) {
        const t = make(date, n.title, { note: n.note ?? "", remind_at: n.remind_at ?? null, priority: n.priority ?? 0 });
        tasks.push(t);
        for (const s of n.subtasks ?? []) tasks.push(make(date, typeof s === "string" ? s : s.title, { parent_id: t.id }));
        created.push(t);
      }
      return env({ tasks: created.map(withSubs) });
    }
    case "add_subtasks": {
      const p = find(a.parent_id);
      for (const s of a.subtasks ?? []) tasks.push(make(p.date, typeof s === "string" ? s : s.title, { parent_id: p.id }));
      if (p.status === "done") setDone(p, false);
      return env({ tasks: [withSubs(p)] });
    }
    case "update_task": {
      const t = find(a.id);
      if (a.title !== undefined) t.title = a.title;
      if (a.note !== undefined) t.note = a.note;
      if (a.priority !== undefined) t.priority = a.priority;
      if (a.remind_at !== undefined) {
        t.remind_at = a.remind_at;
        t.reminded_at = null;
        t.snooze_until = null;
      }
      if (a.date !== undefined && a.date !== t.date) moveOne(t, a.date);
      return env({ task: withSubs(t) });
    }
    case "complete_task": {
      const t = find(a.id);
      const done = a.done ?? true;
      setDone(t, done);
      if (top(t)) {
        if (done) tasks.filter((s) => s.parent_id === t.id && s.status === "todo").forEach((s) => setDone(s, true));
        return env({ task: withSubs(t) });
      }
      const parent = find(t.parent_id);
      const open = tasks.some((s) => s.parent_id === parent.id && s.status === "todo");
      if (!open && parent.status === "todo") setDone(parent, true);
      if (open && parent.status === "done") setDone(parent, false);
      return env({ task: { ...t }, parent: withSubs(parent) });
    }
    case "move_tasks": {
      const moved = (a.ids as number[]).map(find);
      moved.forEach((t) => moveOne(t, a.date));
      return env({ tasks: moved.map(withSubs) });
    }
    case "reorder_tasks": {
      (a.ids as number[]).forEach((id, i) => (find(id).sort_order = i + 1));
      return env({ tasks: sorted(tasks.filter((t) => top(t) && t.date === a.date)).map(withSubs) });
    }
    case "delete_task": {
      const t = find(a.id);
      const gone = tasks.filter((x) => x.id === t.id || x.parent_id === t.id);
      tasks = tasks.filter((x) => !gone.includes(x));
      deleted.push(...gone);
      return env({ task: { ...t } });
    }
    case "restore_task": {
      const back = deleted.filter((x) => x.id === Number(a.id) || x.parent_id === Number(a.id));
      deleted = deleted.filter((x) => !back.includes(x));
      tasks.push(...back);
      return env({ task: withSubs(find(a.id)) });
    }
    case "get_summary": {
      const from = a.from ?? a.date ?? today;
      const to = a.to ?? a.date ?? today;
      const inRange = tasks.filter((t) => top(t) && t.date >= from && t.date <= to);
      const days = [...new Set(inRange.map((t) => t.date))].sort().map((date) => {
        const list = inRange.filter((t) => t.date === date);
        const done = list.filter((t) => t.status === "done").length;
        return { date, total: list.length, done, todo: list.length - done };
      });
      const done = inRange.filter((t) => t.status === "done").length;
      const overdue = sorted(tasks.filter((t) => top(t) && t.date < today && t.status === "todo")).map(withSubs);
      return env({ from, to, total: inRange.length, done, todo: inRange.length - done, days, overdue });
    }
    case "get_day_note":
      return env({ date: a.date ?? today, content: notes[a.date ?? today] ?? "", updated_at: null });
    case "set_day_note":
      notes[a.date ?? today] = a.content ?? "";
      return env({ date: a.date ?? today, content: a.content ?? "", updated_at: stamp() });
    case "list_recurring":
      return env({ series });
    case "add_recurring": {
      const s: Series = {
        id: series.length + 1,
        title: a.title,
        note: a.note ?? "",
        rule: a.rule,
        remind_time: a.remind_time ?? null,
        start_date: a.start_date ?? today,
        end_date: a.end_date ?? null,
        active: true,
        source: "user",
      };
      series = [...series, s];
      return env({ series: s });
    }
    case "update_recurring": {
      const s = series.find((x) => x.id === a.id)!;
      Object.assign(s, a);
      return env({ series: s });
    }
    case "delete_recurring": {
      const s = series.find((x) => x.id === a.id)!;
      series = series.filter((x) => x.id !== a.id);
      return env({ series: s });
    }
    default:
      throw { code: "invalid_argument", message: `mock 未实现 ${tool}` };
  }
}

function moveOne(t: Task, date: string) {
  const shift = (m: string | null) => (m ? `${date}${m.slice(10)}` : null);
  for (const x of tasks.filter((x) => x.id === t.id || x.parent_id === t.id)) {
    x.date = date;
    if (x.remind_at) {
      x.remind_at = shift(x.remind_at);
      x.reminded_at = null;
      x.snooze_until = null;
    }
  }
  t.series_id = null;
  t.sort_order = Math.max(0, ...tasks.filter((x) => x.date === date).map((x) => x.sort_order)) + 1;
}

const app: AppState = {
  expanded: typeof window !== "undefined" && window.innerWidth > 700,
  pinned: false,
  autostart: true,
  hotkey: "Ctrl+Alt+Space",
  morning_enabled: true,
  morning_time: "09:00",
  evening_enabled: true,
  evening_time: "18:00",
  sound: true,
  http_enabled: true,
  http_port: 47821,
  http_active_port: 47821,
  http_error: null,
  token_path: "C:\\Users\\Example\\AppData\\Local\\DayNoteData\\api-token.txt",
  data_dir: "C:\\Users\\Example\\AppData\\Local\\DayNoteData",
  next_data_dir: "C:\\Users\\Example\\AppData\\Local\\DayNoteData",
  data_dir_error: null,
  data_dir_overridden: false,
  cli_path: "C:\\Users\\Example\\AppData\\Local\\DayNote\\daynote-cli.exe",
  version: "0.1.0",
  glass: true,
};

function mockCards(): Card[] {
  const open = sorted(tasks.filter((t) => top(t) && t.date === today && t.status === "todo")).map(withSubs);
  const late = sorted(tasks.filter((t) => top(t) && t.date < today && t.status === "todo")).map(withSubs);
  return [
    { id: "task-3", kind: "task", title: open[0]?.title ?? "准备项目演示", subtitle: "15:00 · 今天", tasks: open.slice(0, 1) },
    { id: "evening", kind: "evening", title: `还有 ${open.length} 件没做完`, subtitle: "晚间回顾", tasks: open },
    { id: "morning", kind: "morning", title: `今天 ${open.length} 件事`, subtitle: `另有 ${late.length} 件遗留`, tasks: open },
  ];
}
let cards: Card[] | null = null;

export async function mockCmd(name: string, a: Record<string, any>): Promise<unknown> {
  switch (name) {
    case "app_state":
      return { ...app };
    case "set_expanded":
      app.expanded = a.expanded;
      break;
    case "set_pinned":
      app.pinned = a.pinned;
      break;
    case "set_autostart":
      app.autostart = a.enabled;
      break;
    case "set_hotkey":
      app.hotkey = a.accelerator;
      break;
    case "regenerate_api_token":
      break;
    case "set_data_directory":
      // Saved for the next start; choosing the directory in use withdraws the choice.
      app.next_data_dir = a.path;
      emit("app-state-changed");
      return a.path;
    case "set_pref":
      if (a.key === "http_port" && (!/^\d+$/.test(a.value.trim()) || Number(a.value) < 1024 || Number(a.value) > 65535)) {
        throw new Error("端口应为 1024–65535 之间的整数");
      }
      (app as any)[a.key] = a.value === "true" ? true : a.value === "false" ? false : a.key === "http_port" ? Number(a.value) : a.value;
      app.http_active_port = app.http_enabled ? app.http_port : 0;
      app.http_error = null;
      break;
    case "reminder_cards":
      return (cards ??= mockCards());
    case "reminder_action":
      cards = (cards ?? []).filter((c) => c.id !== a.cardId);
      emit("reminders-changed");
      return null;
    default:
      return null;
  }
  emit("app-state-changed");
  return null;
}
