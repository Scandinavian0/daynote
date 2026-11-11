// The only place the UI talks to the Tauri shell.
// Outside Tauri (plain `npm run dev` in a browser) a dev-only in-memory mock stands in,
// so the interface can be previewed without building the app.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

type Mock = typeof import("./mock.ts");
let mock: Promise<Mock> | null = null;
const getMock = () => (mock ??= import("./mock.ts"));

/** Run a daynote-core tool (list_tasks, add_tasks, …). Rejects with { code, message }. */
export async function call<T = any>(tool: string, args: Record<string, unknown> = {}): Promise<T> {
  if (inTauri) return invoke<T>("api", { tool, args });
  if (import.meta.env.DEV) return (await getMock()).mockCall(tool, args) as T;
  throw new Error("后端不可用");
}

/** Run a shell command (window, settings, reminder queue). */
export async function cmd<T = any>(name: string, args: Record<string, unknown> = {}): Promise<T> {
  if (inTauri) return invoke<T>(name, args);
  if (import.meta.env.DEV) return (await getMock()).mockCmd(name, args) as T;
  throw new Error("后端不可用");
}

/** Subscribe to a shell event; returns an unsubscribe function. */
export async function on<T = unknown>(event: string, handler: (payload: T) => void): Promise<() => void> {
  if (inTauri) return listen<T>(event, (e) => handler(e.payload));
  if (import.meta.env.DEV) return (await getMock()).mockOn(event, handler as (p: unknown) => void);
  return () => {};
}

export function errorText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e && typeof e === "object" && "message" in e) return String((e as { message: unknown }).message);
  return "操作失败";
}
