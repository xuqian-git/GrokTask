/**
 * Settings + Agent integration IPC surface.
 * Uses Tauri invoke when available; otherwise in-memory mocks for web/tests.
 */

import { isTauriRuntime } from "./ipc";

export type TrayMode = "off" | "active" | "always";

export type IntegrationStatus =
  "not_installed" | "installed" | "outdated" | "invalid_config" | "unavailable";

export type WorkflowStatus =
  "not_enabled" | "enabled" | "outdated" | "invalid_file" | "unavailable";

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
  /** MCP server status */
  status: IntegrationStatus;
  configPath: string;
  binaryPath: string;
  detail?: string;
  canWrite: boolean;
  canRemove: boolean;
  /** Project workflow instruction status */
  workflowStatus: WorkflowStatus;
  workflowPath: string;
  workflowDetail?: string;
  canWriteWorkflow: boolean;
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
  trayMode: "active",
  language: "zh-CN",
  theme: "system",
  historyLimit: 200,
  popoverWidth: 420,
  popoverHeight: 620,
  maxConcurrentTasks: 3,
  version: "0.1.0",
};

let mockWorkspaceCwd = "/mock/workspace";

function defaultAgent(
  agent: AgentId,
  status: IntegrationStatus = "not_installed",
  workflowStatus: WorkflowStatus = "not_enabled",
): AgentIntegrationStatus {
  const filename = agent === "codex" ? "AGENTS.md" : "CLAUDE.md";
  const configPath =
    agent === "codex" ? "~/.codex/config.toml" : "~/.claude.json";
  return {
    agent,
    status,
    configPath,
    binaryPath: "/mock/GrokTask",
    canWrite: true,
    canRemove: true,
    workflowStatus,
    workflowPath: `${mockWorkspaceCwd}/${filename}`,
    canWriteWorkflow: true,
  };
}

let mockAgents: AgentIntegrationStatus[] = [
  defaultAgent("codex"),
  defaultAgent("claude"),
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

export async function fetchWorkspaceCwd(): Promise<string> {
  if (!isTauriRuntime()) {
    if (!mockWorkspaceCwd.trim()) {
      throw new Error("无法解析工作区路径；请从项目目录运行 GrokTask setup");
    }
    return mockWorkspaceCwd;
  }
  return invokeTauri<string>("workspace_cwd");
}

export async function fetchAgentsStatus(
  agent?: AgentId,
  cwd?: string,
): Promise<AgentStatusReport> {
  if (!isTauriRuntime()) {
    const agents = agent
      ? mockAgents.filter((a) => a.agent === agent)
      : [...mockAgents];
    return { agents };
  }
  return invokeTauri<AgentStatusReport>("agents_status", {
    agent: agent ?? null,
    cwd: cwd ?? null,
  });
}

export async function installAgent(
  agent: AgentId,
  cwd?: string,
): Promise<ActionResult> {
  if (!isTauriRuntime()) {
    mockAgents = mockAgents.map((a) =>
      a.agent === agent
        ? { ...a, status: "installed" as const, detail: undefined }
        : a,
    );
    const status = mockAgents.find((a) => a.agent === agent);
    return {
      ok: true,
      message: "已安装/更新 MCP 条目。请在 Agent 中重启或重新加载 MCP。",
      status,
    };
  }
  return invokeTauri<ActionResult>("agents_install", {
    agent,
    cwd: cwd ?? null,
  });
}

export async function removeAgent(
  agent: AgentId,
  cwd?: string,
): Promise<ActionResult> {
  if (!isTauriRuntime()) {
    const target = mockAgents.find((a) => a.agent === agent);
    if (target && !target.canRemove) {
      return {
        ok: false,
        message: target.detail ?? "无法移除：配置无效或不可用",
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
      message: "已移除 MCP 条目。请在 Agent 中重新加载 MCP。",
      status,
    };
  }
  return invokeTauri<ActionResult>("agents_remove", {
    agent,
    cwd: cwd ?? null,
  });
}

export async function enableWorkflow(
  agent: AgentId,
  cwd?: string,
): Promise<ActionResult> {
  if (!isTauriRuntime()) {
    const target = mockAgents.find((a) => a.agent === agent);
    if (target && !target.canWriteWorkflow) {
      return {
        ok: false,
        message: target.workflowDetail ?? "无法写入工作流指令文件",
      };
    }
    mockAgents = mockAgents.map((a) =>
      a.agent === agent
        ? {
            ...a,
            workflowStatus: "enabled" as const,
            workflowDetail: undefined,
          }
        : a,
    );
    const status = mockAgents.find((a) => a.agent === agent);
    return {
      ok: true,
      message: `已写入协作指令到 ${status?.workflowPath ?? "指令文件"}。`,
      status,
    };
  }
  return invokeTauri<ActionResult>("agents_workflow_enable", {
    agent,
    cwd: cwd ?? null,
  });
}

export async function disableWorkflow(
  agent: AgentId,
  cwd?: string,
): Promise<ActionResult> {
  if (!isTauriRuntime()) {
    const target = mockAgents.find((a) => a.agent === agent);
    if (target && !target.canWriteWorkflow) {
      return {
        ok: false,
        message: target.workflowDetail ?? "无法修改工作流指令文件",
      };
    }
    mockAgents = mockAgents.map((a) =>
      a.agent === agent
        ? {
            ...a,
            workflowStatus: "not_enabled" as const,
            workflowDetail: undefined,
          }
        : a,
    );
    const status = mockAgents.find((a) => a.agent === agent);
    return {
      ok: true,
      message: "已移除 GrokTask 托管协作指令区块。",
      status,
    };
  }
  return invokeTauri<ActionResult>("agents_workflow_disable", {
    agent,
    cwd: cwd ?? null,
  });
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
          "未找到 Grok CLI。请从 https://docs.x.ai 安装（当前为 mock 模式）。",
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

/** Open/focus the full app window (from popover). */
export async function openFullWindow(taskId?: string): Promise<void> {
  if (!isTauriRuntime()) {
    // Web/test: navigate within SPA shell.
    const params = new URLSearchParams(window.location.search);
    params.set("view", "task");
    if (taskId) params.set("task", taskId);
    params.delete("section");
    const next = `${window.location.pathname}?${params.toString()}`;
    window.history.replaceState({}, "", next);
    window.dispatchEvent(
      new CustomEvent("groktask-navigate", {
        detail: { view: "task", taskId },
      }),
    );
    return;
  }
  // Prefer focusing main via a lightweight eval-free path: open task window
  // by setting location of a known surface is not available; use invoke if we
  // add a command later. For now, navigate current window if it is the main
  // surface, else open via window label through Tauri plugin is out of scope —
  // emit navigate for same-document shells and try core open.
  try {
    const { WebviewWindow } = await import("@tauri-apps/api/webviewWindow");
    const main = await WebviewWindow.getByLabel("main");
    if (main) {
      await main.show();
      await main.setFocus();
      return;
    }
  } catch {
    // fall through
  }
  window.dispatchEvent(
    new CustomEvent("groktask-navigate", {
      detail: { view: "task", taskId },
    }),
  );
}

/** Test helper: reset mocks between tests. */
export function resetSettingsMocksForTests(): void {
  mockSettings.trayMode = "active";
  mockSettings.historyLimit = 200;
  mockSettings.language = "zh-CN";
  mockWorkspaceCwd = "/mock/workspace";
  mockAgents = [defaultAgent("codex"), defaultAgent("claude")];
}

/** Test helper: inject an agent status. */
export function setMockAgentStatus(
  status: Partial<AgentIntegrationStatus> &
    Pick<AgentIntegrationStatus, "agent">,
): void {
  // Fill workflow defaults when older tests omit them.
  const full: AgentIntegrationStatus = {
    status: "not_installed",
    configPath:
      status.agent === "codex" ? "~/.codex/config.toml" : "~/.claude.json",
    binaryPath: "/mock/GrokTask",
    canWrite: true,
    canRemove: true,
    workflowStatus: "not_enabled",
    workflowPath: `${mockWorkspaceCwd}/${status.agent === "codex" ? "AGENTS.md" : "CLAUDE.md"}`,
    canWriteWorkflow: true,
    ...status,
  };
  mockAgents = mockAgents.map((a) =>
    a.agent === full.agent ? { ...full } : a,
  );
  if (!mockAgents.some((a) => a.agent === full.agent)) {
    mockAgents.push({ ...full });
  }
}

/** Test helper: set mock workspace cwd. Empty string = no trusted project. */
export function setMockWorkspaceCwd(cwd: string): void {
  mockWorkspaceCwd = cwd;
  const hasWs = cwd.trim().length > 0;
  mockAgents = mockAgents.map((a) => {
    const filename = a.agent === "codex" ? "AGENTS.md" : "CLAUDE.md";
    if (!hasWs) {
      return {
        ...a,
        workflowPath: `<workspace>/${filename}`,
        workflowStatus: "unavailable" as const,
        canWriteWorkflow: false,
        workflowDetail: "无法解析工作区路径；请从项目目录运行 GrokTask setup",
      };
    }
    return {
      ...a,
      workflowPath: `${cwd}/${filename}`,
    };
  });
}
