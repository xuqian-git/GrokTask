<script setup lang="ts">
import { onMounted, onUnmounted, ref, watch } from "vue";
import {
  type ActionResult,
  type AgentId,
  type AgentIntegrationStatus,
  type DoctorReport,
  type SettingsSnapshot,
  type TrayMode,
  type WorkflowStatus,
  disableWorkflow,
  enableWorkflow,
  fetchAgentsStatus,
  fetchDoctorReport,
  fetchSettings,
  fetchWorkspaceCwd,
  installAgent,
  removeAgent,
  setTrayMode,
} from "@/lib/settings";

type Section = "general" | "integrations" | "diagnostics" | "history";

const section = ref<Section>("general");
const settings = ref<SettingsSnapshot | null>(null);
const agents = ref<AgentIntegrationStatus[]>([]);
const doctor = ref<DoctorReport | null>(null);
const workspaceCwd = ref<string>("");
/** Essentials only (settings / cwd / agents). Never includes doctor probing. */
const loading = ref(true);
/** Diagnostics-only doctor_report load; does not block General/Tools. */
const doctorLoading = ref(false);
const busyAgent = ref<AgentId | null>(null);
const busyWorkflow = ref<AgentId | null>(null);
const actionMessage = ref<string | null>(null);
const actionOk = ref<boolean | null>(null);
const traySaving = ref(false);

/** In-flight doctor fetch so repeated Diagnostics tab clicks share one request. */
let doctorInFlight: Promise<void> | null = null;

const trayOptions: { value: TrayMode; label: string; hint: string }[] = [
  {
    value: "off",
    label: "关闭",
    hint: "不显示菜单栏图标；仅可通过 CLI 或窗口打开。会移除登录项。",
  },
  {
    value: "active",
    label: "任务活动时",
    hint: "应用运行时/任务活动时显示；不安装登录项。",
  },
  {
    value: "always",
    label: "始终显示",
    hint: "始终显示托盘；安装用户登录项以启动 --gui-host。",
  },
];

function mcpStatusLabel(s: string): string {
  switch (s) {
    case "not_installed":
      return "未安装";
    case "installed":
      return "已安装";
    case "outdated":
      return "需更新";
    case "invalid_config":
      return "配置无效";
    case "unavailable":
      return "不可用";
    default:
      return s;
  }
}

function workflowStatusLabel(s: WorkflowStatus | string): string {
  switch (s) {
    case "not_enabled":
      return "未启用";
    case "enabled":
      return "已启用";
    case "outdated":
      return "需更新";
    case "invalid_file":
      return "文件异常";
    case "unavailable":
      return "不可用";
    default:
      return s;
  }
}

function agentTitle(id: AgentId): string {
  return id === "codex" ? "Codex" : "Claude Code";
}

function selectSection(next: Section) {
  section.value = next;
  // Keep URL query consistent so reopening / setup deep-links stay single-click.
  const params = new URLSearchParams(window.location.search);
  params.set("view", "settings");
  params.set("section", next);
  const qs = params.toString();
  window.history.replaceState({}, "", qs ? `?${qs}` : "?");
}

/** Lightweight Settings essentials for General / Tools — never runs doctor probes. */
async function refreshEssentials() {
  loading.value = true;
  actionMessage.value = null;
  actionOk.value = null;
  try {
    const [s, cwd, a] = await Promise.all([
      fetchSettings(),
      fetchWorkspaceCwd().catch(() => ""),
      fetchAgentsStatus(undefined, undefined),
    ]);
    settings.value = s;
    workspaceCwd.value = cwd;
    // Re-fetch with workspace so workflow paths are accurate.
    if (cwd) {
      const report = await fetchAgentsStatus(undefined, cwd);
      agents.value = report.agents;
    } else {
      agents.value = a.agents;
    }
  } finally {
    loading.value = false;
  }
}

/**
 * Lazy doctor_report for the Diagnostics tab only.
 * By default skips when already loaded; `force` re-fetches (Refresh button).
 * Concurrent callers share the same in-flight promise.
 */
async function refreshDoctor(options: { force?: boolean } = {}) {
  const force = options.force ?? false;
  if (!force && doctor.value !== null) {
    return;
  }
  if (doctorInFlight) {
    return doctorInFlight;
  }
  doctorLoading.value = true;
  doctorInFlight = (async () => {
    try {
      doctor.value = await fetchDoctorReport();
    } catch (e) {
      actionOk.value = false;
      actionMessage.value = e instanceof Error ? e.message : String(e);
    } finally {
      doctorLoading.value = false;
      doctorInFlight = null;
    }
  })();
  return doctorInFlight;
}

async function onTrayModeChange(mode: TrayMode) {
  traySaving.value = true;
  actionMessage.value = null;
  try {
    settings.value = await setTrayMode(mode);
    actionOk.value = true;
    actionMessage.value = `托盘模式已设为「${trayOptions.find((o) => o.value === mode)?.label ?? mode}」。`;
  } catch (e) {
    actionOk.value = false;
    actionMessage.value = e instanceof Error ? e.message : String(e);
  } finally {
    traySaving.value = false;
  }
}

function patchAgent(status: AgentIntegrationStatus) {
  agents.value = agents.value.map((a) =>
    a.agent === status.agent ? status : a,
  );
}

async function onInstall(agent: AgentId) {
  const card = agents.value.find((a) => a.agent === agent);
  if (card && !card.canWrite) {
    actionOk.value = false;
    actionMessage.value =
      card.detail ?? "无法写入：配置无效或不可用。请先手动修复配置文件。";
    return;
  }
  busyAgent.value = agent;
  actionMessage.value = null;
  try {
    const result: ActionResult = await installAgent(
      agent,
      workspaceCwd.value || undefined,
    );
    actionOk.value = result.ok;
    actionMessage.value =
      result.message ?? (result.ok ? "完成。" : "安装失败。");
    if (result.status) {
      patchAgent(result.status);
    } else {
      const report = await fetchAgentsStatus(
        undefined,
        workspaceCwd.value || undefined,
      );
      agents.value = report.agents;
    }
  } catch (e) {
    actionOk.value = false;
    actionMessage.value = e instanceof Error ? e.message : String(e);
  } finally {
    busyAgent.value = null;
  }
}

async function onRemove(agent: AgentId) {
  const card = agents.value.find((a) => a.agent === agent);
  if (card && !card.canRemove) {
    actionOk.value = false;
    actionMessage.value = card.detail ?? "无法移除：配置无效或不可用。";
    return;
  }
  busyAgent.value = agent;
  actionMessage.value = null;
  try {
    const result: ActionResult = await removeAgent(
      agent,
      workspaceCwd.value || undefined,
    );
    actionOk.value = result.ok;
    actionMessage.value =
      result.message ?? (result.ok ? "完成。" : "移除失败。");
    if (result.status) {
      patchAgent(result.status);
    } else {
      const report = await fetchAgentsStatus(
        undefined,
        workspaceCwd.value || undefined,
      );
      agents.value = report.agents;
    }
  } catch (e) {
    actionOk.value = false;
    actionMessage.value = e instanceof Error ? e.message : String(e);
  } finally {
    busyAgent.value = null;
  }
}

async function onWorkflowEnable(agent: AgentId) {
  if (!workspaceCwd.value) {
    actionOk.value = false;
    actionMessage.value =
      "无法解析工作区路径；请从项目目录运行 GrokTask setup。";
    return;
  }
  const card = agents.value.find((a) => a.agent === agent);
  if (card && !card.canWriteWorkflow) {
    actionOk.value = false;
    actionMessage.value = card.workflowDetail ?? "无法写入工作流指令文件。";
    return;
  }
  busyWorkflow.value = agent;
  actionMessage.value = null;
  try {
    const result = await enableWorkflow(agent, workspaceCwd.value || undefined);
    actionOk.value = result.ok;
    actionMessage.value =
      result.message ?? (result.ok ? "已启用协作指令。" : "启用失败。");
    if (result.status) {
      patchAgent(result.status);
    } else {
      const report = await fetchAgentsStatus(
        undefined,
        workspaceCwd.value || undefined,
      );
      agents.value = report.agents;
    }
  } catch (e) {
    actionOk.value = false;
    actionMessage.value = e instanceof Error ? e.message : String(e);
  } finally {
    busyWorkflow.value = null;
  }
}

async function onWorkflowDisable(agent: AgentId) {
  if (!workspaceCwd.value) {
    actionOk.value = false;
    actionMessage.value =
      "无法解析工作区路径；请从项目目录运行 GrokTask setup。";
    return;
  }
  const card = agents.value.find((a) => a.agent === agent);
  if (card && !card.canWriteWorkflow) {
    actionOk.value = false;
    actionMessage.value = card.workflowDetail ?? "无法修改工作流指令文件。";
    return;
  }
  busyWorkflow.value = agent;
  actionMessage.value = null;
  try {
    const result = await disableWorkflow(
      agent,
      workspaceCwd.value || undefined,
    );
    actionOk.value = result.ok;
    actionMessage.value =
      result.message ?? (result.ok ? "已禁用协作指令。" : "禁用失败。");
    if (result.status) {
      patchAgent(result.status);
    } else {
      const report = await fetchAgentsStatus(
        undefined,
        workspaceCwd.value || undefined,
      );
      agents.value = report.agents;
    }
  } catch (e) {
    actionOk.value = false;
    actionMessage.value = e instanceof Error ? e.message : String(e);
  } finally {
    busyWorkflow.value = null;
  }
}

function applySectionFromQuery() {
  const params = new URLSearchParams(window.location.search);
  const s = params.get("section");
  if (
    s === "general" ||
    s === "integrations" ||
    s === "diagnostics" ||
    s === "history"
  ) {
    section.value = s;
  }
}

function onSettingsSectionEvent(ev: Event) {
  const detail = (ev as CustomEvent<string>).detail;
  if (
    detail === "general" ||
    detail === "integrations" ||
    detail === "diagnostics" ||
    detail === "history"
  ) {
    // External nav (CLI setup / tray) — update state and URL once.
    selectSection(detail);
  }
}

onMounted(() => {
  applySectionFromQuery();
  window.addEventListener("groktask-settings-section", onSettingsSectionEvent);
  // Essentials only — doctor_report is deferred until Diagnostics is selected.
  void refreshEssentials();
  if (section.value === "diagnostics") {
    void refreshDoctor();
  }
});

onUnmounted(() => {
  window.removeEventListener(
    "groktask-settings-section",
    onSettingsSectionEvent,
  );
});

watch(section, (next) => {
  actionMessage.value = null;
  actionOk.value = null;
  if (next === "diagnostics") {
    void refreshDoctor();
  }
});
</script>

<template>
  <section class="settings" data-testid="settings-shell">
    <nav class="tabs" aria-label="设置分区" data-testid="settings-tabs">
      <button
        type="button"
        data-testid="tab-general"
        :class="{ active: section === 'general' }"
        @click="selectSection('general')"
      >
        通用
      </button>
      <button
        type="button"
        data-testid="tab-integrations"
        :class="{ active: section === 'integrations' }"
        @click="selectSection('integrations')"
      >
        工具开关
      </button>
      <button
        type="button"
        data-testid="tab-diagnostics"
        :class="{ active: section === 'diagnostics' }"
        @click="selectSection('diagnostics')"
      >
        诊断
      </button>
      <button
        type="button"
        data-testid="tab-history"
        :class="{ active: section === 'history' }"
        @click="selectSection('history')"
      >
        历史
      </button>
    </nav>

    <p v-if="loading" class="hint" data-testid="settings-loading">加载中…</p>

    <div
      v-if="actionMessage"
      class="banner"
      :class="actionOk === false ? 'banner-error' : 'banner-ok'"
      data-testid="action-result"
      role="status"
    >
      {{ actionMessage }}
    </div>

    <template v-if="!loading && settings">
      <!-- General -->
      <div
        v-if="section === 'general'"
        class="panel section"
        data-testid="section-general"
      >
        <h2>通用</h2>
        <fieldset class="field">
          <legend>菜单栏 / 托盘图标</legend>
          <div class="radio-list" data-testid="tray-mode-controls">
            <label
              v-for="opt in trayOptions"
              :key="opt.value"
              class="radio-row"
            >
              <input
                type="radio"
                name="trayMode"
                :value="opt.value"
                :checked="settings.trayMode === opt.value"
                :disabled="traySaving"
                @change="onTrayModeChange(opt.value)"
              />
              <span>
                <strong>{{ opt.label }}</strong>
                <span class="hint">{{ opt.hint }}</span>
              </span>
            </label>
          </div>
        </fieldset>

        <div class="meta-grid">
          <div>
            <span class="label">语言</span>
            <span data-testid="language-value">{{ settings.language }}</span>
            <span class="hint">界面默认简体中文</span>
          </div>
          <div>
            <span class="label">主题</span>
            <span data-testid="theme-value">{{ settings.theme }}</span>
            <span class="hint">跟随系统</span>
          </div>
          <div>
            <span class="label">浮层尺寸</span>
            <span data-testid="popover-size">
              {{ settings.popoverWidth }}×{{ settings.popoverHeight }}
            </span>
          </div>
          <div>
            <span class="label">最大并发任务</span>
            <span>{{ settings.maxConcurrentTasks }}</span>
          </div>
          <div>
            <span class="label">版本</span>
            <span data-testid="app-version">{{ settings.version }}</span>
          </div>
        </div>
      </div>

      <!-- Tools / Integrations -->
      <div
        v-else-if="section === 'integrations'"
        class="panel section"
        data-testid="section-integrations"
      >
        <h2>工具开关</h2>
        <p class="intro">
          两层集成相互独立：<strong>MCP 服务</strong>只让 Agent 能调用
          <code>groktask</code>
          工具；<strong>协作指令</strong>写入项目指令文件，引导 Agent
          在编码时主动使用 GrokTask。仅安装 MCP 不会自动启用协作指令。
        </p>
        <p class="workspace-line" data-testid="workspace-cwd">
          <span class="label">当前工作区（项目指令写入位置）</span>
          <code>{{
            workspaceCwd || "（无法解析 / 请从项目目录运行 GrokTask setup）"
          }}</code>
        </p>
        <p
          v-if="!workspaceCwd"
          class="hint warn"
          data-testid="workspace-cwd-missing"
        >
          未选定项目工作区。请在项目目录中运行
          <code>GrokTask setup</code>
          后再启用/禁用协作指令；菜单栏或 Finder
          打开时不会使用进程当前目录作为写入目标。
        </p>

        <article
          v-for="card in agents"
          :key="card.agent"
          class="card"
          :data-testid="`agent-card-${card.agent}`"
          :data-status="card.status"
          :data-workflow="card.workflowStatus"
        >
          <header class="card-head">
            <h3>{{ agentTitle(card.agent) }}</h3>
          </header>

          <!-- MCP layer -->
          <div class="layer" data-testid="mcp-layer">
            <div class="layer-head">
              <strong>MCP 服务</strong>
              <span
                class="status-pill"
                :data-status="card.status"
                data-testid="agent-status"
              >
                {{ mcpStatusLabel(card.status) }}
              </span>
            </div>
            <dl class="card-meta">
              <div>
                <dt>配置文件</dt>
                <dd data-testid="agent-config-path">
                  {{ card.configPath }}
                </dd>
              </div>
              <div>
                <dt>将写入的二进制路径</dt>
                <dd data-testid="agent-binary-path">
                  {{ card.binaryPath }}
                </dd>
              </div>
              <div v-if="card.detail">
                <dt>说明</dt>
                <dd data-testid="agent-detail">
                  {{ card.detail }}
                </dd>
              </div>
            </dl>
            <div class="card-actions">
              <button
                type="button"
                data-testid="agent-install"
                :disabled="!card.canWrite || busyAgent === card.agent"
                @click="onInstall(card.agent)"
              >
                {{
                  card.status === "outdated"
                    ? "更新"
                    : card.status === "installed"
                      ? "重新安装"
                      : "安装"
                }}
              </button>
              <button
                type="button"
                class="danger"
                data-testid="agent-remove"
                :disabled="!card.canRemove || busyAgent === card.agent"
                @click="onRemove(card.agent)"
              >
                移除
              </button>
            </div>
            <p
              v-if="!card.canWrite || !card.canRemove"
              class="hint warn"
              data-testid="agent-disabled-reason"
            >
              {{ card.detail || "因配置无法安全编辑，写入操作已禁用。" }}
            </p>
            <p class="hint reminder" data-testid="agent-reminder">
              安装 / 更新 / 移除后，请在 Agent 中重启或重新加载 MCP。
            </p>
          </div>

          <!-- Workflow layer -->
          <div class="layer" data-testid="workflow-layer">
            <div class="layer-head">
              <strong>协作指令</strong>
              <span
                class="status-pill"
                :data-status="card.workflowStatus"
                data-testid="workflow-status"
              >
                {{ workflowStatusLabel(card.workflowStatus) }}
              </span>
            </div>
            <dl class="card-meta">
              <div>
                <dt>指令文件（项目级）</dt>
                <dd data-testid="workflow-path">
                  {{ card.workflowPath }}
                </dd>
              </div>
              <div v-if="card.workflowDetail">
                <dt>说明</dt>
                <dd data-testid="workflow-detail">
                  {{ card.workflowDetail }}
                </dd>
              </div>
            </dl>
            <div class="card-actions">
              <button
                type="button"
                data-testid="workflow-enable"
                :disabled="
                  !workspaceCwd ||
                  !card.canWriteWorkflow ||
                  busyWorkflow === card.agent
                "
                @click="onWorkflowEnable(card.agent)"
              >
                {{
                  card.workflowStatus === "outdated"
                    ? "更新指令"
                    : card.workflowStatus === "enabled"
                      ? "重新写入"
                      : "启用"
                }}
              </button>
              <button
                type="button"
                class="danger"
                data-testid="workflow-disable"
                :disabled="
                  !workspaceCwd ||
                  !card.canWriteWorkflow ||
                  busyWorkflow === card.agent
                "
                @click="onWorkflowDisable(card.agent)"
              >
                禁用
              </button>
            </div>
            <p
              v-if="!workspaceCwd || !card.canWriteWorkflow"
              class="hint warn"
              data-testid="workflow-disabled-reason"
            >
              {{
                !workspaceCwd
                  ? "无法解析工作区路径；请从项目目录运行 GrokTask setup。"
                  : card.workflowDetail ||
                    "无法安全写入指令文件（标记异常或路径不可用）。"
              }}
            </p>
            <p class="hint reminder" data-testid="workflow-reminder">
              仅写入托管区块（
              <code>GrokTask:begin</code>
              …
              <code>end</code>
              ），不会改动 AskHuman 或其它用户内容。全局指令注入尚未支持。
            </p>
          </div>
        </article>
      </div>

      <!-- Diagnostics -->
      <div
        v-else-if="section === 'diagnostics'"
        class="panel section"
        data-testid="section-diagnostics"
      >
        <h2>诊断</h2>
        <p
          v-if="doctorLoading && !doctor"
          class="hint"
          data-testid="doctor-loading"
        >
          加载诊断信息…
        </p>
        <p
          v-else-if="doctorLoading && doctor"
          class="hint"
          data-testid="doctor-loading"
        >
          刷新中…
        </p>
        <template v-if="doctor">
          <div class="meta-grid">
            <div>
              <span class="label">GrokTask</span>
              <span>{{ doctor.version }}</span>
              <span class="hint mono">{{ doctor.executable }}</span>
            </div>
            <div>
              <span class="label">Daemon</span>
              <span data-testid="daemon-status">{{ doctor.daemon }}</span>
            </div>
            <div>
              <span class="label">托盘</span>
              <span data-testid="tray-capability">
                可用={{ doctor.tray.trayAvailable }} · 点击={{
                  doctor.tray.trayClick
                }}
              </span>
              <span v-if="doctor.tray.detail" class="hint">{{
                doctor.tray.detail
              }}</span>
            </div>
            <div>
              <span class="label">Grok CLI</span>
              <span data-testid="grok-state">{{ doctor.grok.state }}</span>
              <span v-if="doctor.grok.executable" class="hint mono">{{
                doctor.grok.executable
              }}</span>
              <span v-if="doctor.grok.version" class="hint"
                >版本 {{ doctor.grok.version }}</span
              >
              <span v-if="doctor.grok.guidance" class="hint warn">{{
                doctor.grok.guidance
              }}</span>
            </div>
          </div>
          <p class="hint">
            GrokTask 从不读取或存储 xAI token，也不会自动启动交互式
            <code>grok login</code>。
          </p>
          <button
            type="button"
            data-testid="refresh-doctor"
            :disabled="doctorLoading"
            @click="refreshDoctor({ force: true })"
          >
            刷新
          </button>
        </template>
      </div>

      <!-- History settings -->
      <div v-else class="panel section" data-testid="section-history">
        <h2>历史保留</h2>
        <div class="meta-grid">
          <div>
            <span class="label">历史条数上限</span>
            <span data-testid="history-limit">{{ settings.historyLimit }}</span>
            <span class="hint">本地 SQLite 保留的任务数</span>
          </div>
        </div>
        <p class="hint">
          清空历史将在存储清理 API 安全暴露后提供。当前版本不提供破坏性清空。
        </p>
        <button type="button" disabled data-testid="clear-history-disabled">
          清空历史（暂不可用）
        </button>
      </div>
    </template>
  </section>
</template>

<style scoped>
.settings {
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 16px;
  min-height: 0;
  height: 100%;
  overflow: auto;
}
.tabs {
  display: flex;
  gap: 4px;
  flex-wrap: wrap;
}
.tabs button {
  border: 1px solid var(--border);
  background: var(--card);
  color: var(--subtle);
  font-size: 12px;
  padding: 6px 12px;
  border-radius: 8px;
  cursor: pointer;
}
.tabs button.active {
  background: var(--muted-bg);
  color: var(--muted-fg);
  font-weight: 600;
  border-color: transparent;
}
.section h2 {
  margin: 0 0 12px;
  font-size: 15px;
}
.intro {
  margin: 0 0 12px;
  font-size: 13px;
  color: var(--subtle);
  line-height: 1.45;
}
.workspace-line {
  margin: 0 0 16px;
  font-size: 12px;
}
.workspace-line .label {
  display: block;
  font-size: 11px;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  color: var(--subtle);
  margin-bottom: 4px;
}
.workspace-line code {
  word-break: break-all;
}
.field {
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 12px;
  margin: 0 0 16px;
}
.field legend {
  padding: 0 6px;
  font-size: 12px;
  font-weight: 600;
}
.radio-list {
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.radio-row {
  display: flex;
  gap: 10px;
  align-items: flex-start;
  font-size: 13px;
  cursor: pointer;
}
.radio-row strong {
  display: block;
}
.hint {
  display: block;
  font-size: 12px;
  color: var(--subtle);
  margin-top: 2px;
}
.hint.warn {
  color: var(--danger);
}
.hint.mono {
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  word-break: break-all;
}
.meta-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
  gap: 12px;
  margin-bottom: 12px;
}
.meta-grid .label {
  display: block;
  font-size: 11px;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  color: var(--subtle);
  margin-bottom: 2px;
}
.card {
  border: 1px solid var(--border);
  border-radius: 12px;
  padding: 14px;
  margin-bottom: 12px;
  background: var(--bg);
}
.card-head {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-bottom: 12px;
}
.card-head h3 {
  margin: 0;
  font-size: 14px;
}
.layer {
  border-top: 1px solid var(--border);
  padding-top: 12px;
  margin-top: 4px;
}
.layer:first-of-type {
  border-top: none;
  padding-top: 0;
}
.layer-head {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 8px;
  font-size: 13px;
}
.status-pill {
  font-size: 11px;
  padding: 2px 8px;
  border-radius: 999px;
  background: var(--muted-bg);
  color: var(--muted-fg);
}
.status-pill[data-status="invalid_config"],
.status-pill[data-status="invalid_file"],
.status-pill[data-status="unavailable"] {
  background: #fef2f2;
  color: var(--danger);
}
.status-pill[data-status="installed"],
.status-pill[data-status="enabled"] {
  background: #ecfdf5;
  color: var(--success);
}
.status-pill[data-status="outdated"] {
  background: #fffbeb;
  color: #b45309;
}
.card-meta {
  margin: 0 0 12px;
  display: grid;
  gap: 8px;
  font-size: 12px;
}
.card-meta dt {
  color: var(--subtle);
  font-size: 11px;
}
.card-meta dd {
  margin: 2px 0 0;
  word-break: break-all;
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
}
.card-actions {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}
.card-actions button,
.section > button {
  border: 1px solid var(--border);
  background: var(--card);
  color: var(--fg);
  font-size: 12px;
  padding: 6px 12px;
  border-radius: 8px;
  cursor: pointer;
}
.card-actions button:disabled,
.section > button:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
.card-actions button.danger {
  color: var(--danger);
}
.reminder {
  margin-top: 10px;
}
.banner {
  font-size: 13px;
  padding: 10px 12px;
  border-radius: 8px;
}
.banner-ok {
  background: #ecfdf5;
  color: var(--success);
}
.banner-error {
  background: #fef2f2;
  color: var(--danger);
}
code {
  font-size: 12px;
  background: var(--muted-bg);
  padding: 1px 4px;
  border-radius: 4px;
}
</style>
