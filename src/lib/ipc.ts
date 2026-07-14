/**
 * Frontend IPC helpers.
 * Tauri runtime → daemon via host commands; web/test → mock fixtures.
 */

import {
  mockRunningTaskDetail,
  mockTaskDetail,
  mockTaskList,
} from "./mockData";
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

async function invokeTauri<T>(
  cmd: string,
  args?: Record<string, unknown>,
): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, args);
}

/** Offline / vitest fixtures, including multi-task demo variants. */
function mockDetailForId(taskId?: string): TaskDetail {
  if (taskId === "task-demo-2") {
    return mockRunningTaskDetail();
  }
  if (taskId === "task-demo-3") {
    const d = mockTaskDetail();
    d.task.taskId = taskId;
    d.task.status = "failed";
    d.title = "Fix build";
    d.cwd = "/tmp/other";
    d.activePlan = undefined;
    d.task.latestAction = "cargo clippy failed";
    return d;
  }
  const d = mockTaskDetail();
  if (taskId) d.task.taskId = taskId;
  return d;
}

/** Load task detail — real daemon in Tauri; mock in web/test mode. */
export async function fetchTaskDetail(taskId?: string): Promise<TaskDetail> {
  if (!isTauriRuntime()) {
    return mockDetailForId(taskId);
  }
  if (!taskId) {
    throw new Error("taskId is required");
  }
  return invokeTauri<TaskDetail>("tasks_show", { taskId });
}

/** Load task list — real daemon in Tauri; mock in web/test mode. */
export async function fetchTaskList(limit?: number): Promise<TaskListItem[]> {
  if (!isTauriRuntime()) {
    return mockTaskList();
  }
  return invokeTauri<TaskListItem[]>("tasks_list", {
    limit: limit ?? null,
  });
}
