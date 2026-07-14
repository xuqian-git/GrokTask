/**
 * Settings + Agent integration IPC surface.
 * Uses Tauri invoke when available; otherwise in-memory mocks for web/tests.
 */

import { isTauriRuntime } from "./ipc";

export type TrayMode = "off" | "active" | "always";

export type IntegrationStatus =
  "not_installed" | "installed" | "outdated" | "invalid_config" | "unavailable";

export type AgentId = "codex" | "claude";

export interface SettingsSnapshot {
  trayMode: TrayMode;
  language: string;
  theme: string;
  historyLimit: number;
  popoverWidth: number;
  popoverHeight: number;
  maxConcurrentTasks: number;
  version: string;
}

export interface AgentIntegrationStatus {
  agent: AgentId;
  status: IntegrationStatus;
  configPath: string;
  binaryPath: string;
  detail?: string;
  canWrite: boolean;
  canRemove: boolean;
}

export interface AgentStatusReport {
  agents: AgentIntegrationStatus[];
}

export interface ActionResult {
  ok: boolean;
  message?: string;
  status?: AgentIntegrationStatus;
}

export interface GrokCliStatus {
  state:
    "not_found" | "found" | "logged_in" | "not_logged_in" | "version_unknown";
  executable?: string;
  version?: string;
  guidance?: string;
  checkedAt: string;
}

export interface TrayCapability {
  trayAvailable: boolean;
  trayClick: "available" | "unavailable" | "degraded";
  detail?: string;
}

export interface DoctorReport {
  version: string;
  executable: string;
  daemon: string;
  grok: GrokCliStatus;
  tray: TrayCapability;
  trayMode?: string;
}

/** In-memory mock for tests / non-Tauri web. */
const mockSettings: SettingsSnapshot = {
  trayMode: "off",
  language: "system",
  theme: "system",
  historyLimit: 200,
  popoverWidth: 420,
  popoverHeight: 620,
  maxConcurrentTasks: 3,
  version: "0.1.0",
};

let mockAgents: AgentIntegrationStatus[] = [
  {
    agent: "codex",
    status: "not_installed",
    configPath: "~/.codex/config.toml",
    binaryPath: "/mock/GrokTask",
    canWrite: true,
    canRemove: true,
  },
  {
    agent: "claude",
    status: "not_installed",
    configPath: "~/.claude.json",
    binaryPath: "/mock/GrokTask",
    canWrite: true,
    canRemove: true,
  },
];

async function invokeTauri<T>(
  cmd: string,
  args?: Record<string, unknown>,
): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, args);
}

export async function fetchSettings(): Promise<SettingsSnapshot> {
  if (!isTauriRuntime()) {
    return { ...mockSettings };
  }
  return invokeTauri<SettingsSnapshot>("settings_get");
}

export async function setTrayMode(mode: TrayMode): Promise<SettingsSnapshot> {
  if (!isTauriRuntime()) {
    mockSettings.trayMode = mode;
    return { ...mockSettings };
  }
  return invokeTauri<SettingsSnapshot>("settings_set_tray_mode", { mode });
}

export async function fetchAgentsStatus(
  agent?: AgentId,
): Promise<AgentStatusReport> {
  if (!isTauriRuntime()) {
    const agents = agent
      ? mockAgents.filter((a) => a.agent === agent)
      : [...mockAgents];
    return { agents };
  }
  return invokeTauri<AgentStatusReport>("agents_status", {
    agent: agent ?? null,
  });
}

export async function installAgent(agent: AgentId): Promise<ActionResult> {
  if (!isTauriRuntime()) {
    mockAgents = mockAgents.map((a) =>
      a.agent === agent
        ? { ...a, status: "installed" as const, detail: undefined }
        : a,
    );
    const status = mockAgents.find((a) => a.agent === agent);
    return {
      ok: true,
      message:
        "Installed/updated MCP entry. Restart or reload MCP in the agent to apply.",
      status,
    };
  }
  return invokeTauri<ActionResult>("agents_install", { agent });
}

export async function removeAgent(agent: AgentId): Promise<ActionResult> {
  if (!isTauriRuntime()) {
    const target = mockAgents.find((a) => a.agent === agent);
    if (target && !target.canRemove) {
      return {
        ok: false,
        message:
          target.detail ?? "Cannot remove: config invalid or unavailable",
      };
    }
    mockAgents = mockAgents.map((a) =>
      a.agent === agent
        ? { ...a, status: "not_installed" as const, detail: undefined }
        : a,
    );
    const status = mockAgents.find((a) => a.agent === agent);
    return {
      ok: true,
      message: "Removed MCP entry. Reload MCP in the agent to apply.",
      status,
    };
  }
  return invokeTauri<ActionResult>("agents_remove", { agent });
}

export async function fetchDoctorReport(): Promise<DoctorReport> {
  if (!isTauriRuntime()) {
    return {
      version: mockSettings.version,
      executable: "/mock/GrokTask",
      daemon: "stopped (mock)",
      grok: {
        state: "not_found",
        guidance:
          "Grok CLI not found. Install from https://docs.x.ai (mock mode).",
        checkedAt: new Date().toISOString(),
      },
      tray: {
        trayAvailable: true,
        trayClick: "available",
      },
      trayMode: mockSettings.trayMode,
    };
  }
  return invokeTauri<DoctorReport>("doctor_report");
}

export async function fetchDaemonStatus(): Promise<string> {
  if (!isTauriRuntime()) {
    return "stopped (mock)";
  }
  return invokeTauri<string>("daemon_status_text");
}

export async function restartDaemon(force = false): Promise<string> {
  if (!isTauriRuntime()) {
    return "daemon restart requested (mock)";
  }
  return invokeTauri<string>("daemon_restart", { force });
}

/** Test helper: reset mocks between tests. */
export function resetSettingsMocksForTests(): void {
  mockSettings.trayMode = "off";
  mockSettings.historyLimit = 200;
  mockAgents = [
    {
      agent: "codex",
      status: "not_installed",
      configPath: "~/.codex/config.toml",
      binaryPath: "/mock/GrokTask",
      canWrite: true,
      canRemove: true,
    },
    {
      agent: "claude",
      status: "not_installed",
      configPath: "~/.claude.json",
      binaryPath: "/mock/GrokTask",
      canWrite: true,
      canRemove: true,
    },
  ];
}

/** Test helper: inject an agent status. */
export function setMockAgentStatus(status: AgentIntegrationStatus): void {
  mockAgents = mockAgents.map((a) =>
    a.agent === status.agent ? { ...status } : a,
  );
  if (!mockAgents.some((a) => a.agent === status.agent)) {
    mockAgents.push({ ...status });
  }
}
