/**
 * Frontend IPC helpers.
 * Phase 2–3: typed surfaces + mock adapter when daemon/Tauri is unavailable.
 */

import { mockTaskDetail, mockTaskList } from "./mockData";
import type { TaskDetail, TaskListItem } from "./types";

export type SurfaceId = "popover" | "task" | "history" | "settings";

export interface ConnectionHealth {
  status: "offline" | "connecting" | "online" | "degraded";
  daemonVersion?: string;
  reason?: string;
}

export function defaultHealth(): ConnectionHealth {
  return { status: "offline" };
}

export function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** Load task detail — mock in web/test mode. */
export async function fetchTaskDetail(taskId?: string): Promise<TaskDetail> {
  if (!isTauriRuntime()) {
    const d = mockTaskDetail();
    if (taskId) d.task.taskId = taskId;
    return d;
  }
  // GUI host will wire real daemon IPC in Phase 4; fall back to mock until then.
  return mockTaskDetail();
}

export async function fetchTaskList(): Promise<TaskListItem[]> {
  if (!isTauriRuntime()) {
    return mockTaskList();
  }
  return mockTaskList();
}
