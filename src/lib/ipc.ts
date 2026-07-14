/**
 * Frontend IPC helpers.
 * Phase 0–1: typed surface only; Tauri commands connect to the GUI host later.
 */

export type SurfaceId = "popover" | "task" | "history" | "settings";

export interface ConnectionHealth {
  status: "offline" | "connecting" | "online" | "degraded";
  daemonVersion?: string;
  reason?: string;
}

export function defaultHealth(): ConnectionHealth {
  return { status: "offline" };
}
