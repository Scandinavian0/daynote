// JSON shapes shared with daynote-core (see crates/core api::dispatch) and the Tauri shell.

export interface Task {
  id: number;
  date: string;
  title: string;
  note: string | null;
  status: "todo" | "done";
  completed_at: string | null;
  completed_by: string | null;
  remind_at: string | null;
  snooze_until: string | null;
  reminded_at: string | null;
  priority: number;
  sort_order: number;
  parent_id: number | null;
  series_id: number | null;
  source: string;
  created_at: string;
  updated_at: string;
  subtasks?: Task[];
}

export interface DayCount {
  date: string;
  total: number;
  done: number;
  todo: number;
}

export interface Summary {
  today: string;
  from: string;
  to: string;
  total: number;
  done: number;
  todo: number;
  days: DayCount[];
  overdue: Task[];
}

export interface Series {
  id: number;
  title: string;
  note: string | null;
  rule: string;
  remind_time: string | null;
  start_date: string;
  end_date: string | null;
  active: boolean;
  source: string;
}

/** Shell-side state, returned by the `app_state` command. */
export interface AppState {
  expanded: boolean;
  pinned: boolean;
  autostart: boolean;
  hotkey: string;
  morning_enabled: boolean;
  morning_time: string;
  evening_enabled: boolean;
  evening_time: string;
  sound: boolean;
  http_enabled: boolean;
  http_port: number;
  http_active_port: number;
  http_error: string | null;
  token_path: string;
  data_dir: string;
  next_data_dir: string;
  data_dir_overridden: boolean;
  cli_path: string;
  version: string;
  glass: boolean;
}

/** One card in the reminder window. `tasks` holds the single task for kind "task". */
export interface Card {
  id: string;
  kind: "task" | "missed" | "morning" | "evening";
  title: string;
  subtitle: string;
  tasks: Task[];
}
