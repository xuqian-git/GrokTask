<script setup lang="ts">
import { onMounted, onUnmounted, ref, watch } from "vue";
import {
  type ActionResult,
  type AgentId,
  type AgentIntegrationStatus,
  type DoctorReport,
  type LanguagePref,
  type SettingsSnapshot,
  type ThemePref,
  type TrayMode,
  type WorkflowStatus,
  clearHistory,
  disableWorkflow,
  enableWorkflow,
  fetchAgentsStatus,
  fetchDoctorReport,
  fetchSettings,
  installAgent,
  removeAgent,
  setLanguage,
  setHistoryLimit,
  setTheme,
  setTrayMode,
} from "@/lib/settings";

type Section = "general" | "integrations" | "diagnostics" | "history";

const section = ref<Section>("general");
const settings = ref<SettingsSnapshot | null>(null);
const agents = ref<AgentIntegrationStatus[]>([]);
const doctor = ref<DoctorReport | null>(null);
/** Essentials only (settings / cwd / agents). Never includes doctor probing. */
const loading = ref(true);
/** Diagnostics-only doctor_report load; does not block General/Tools. */
const doctorLoading = ref(false);
const busyAgent = ref<AgentId | null>(null);
const busyWorkflow = ref<AgentId | null>(null);
const actionMessage = ref<string | null>(null);
const actionOk = ref<boolean | null>(null);
const traySaving = ref(false);
const languageSaving = ref(false);
const themeSaving = ref(false);
const historySaving = ref(false);
const historyClearing = ref(false);
const historyLimitInput = ref("200");

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

const languageOptions: { value: LanguagePref; label: string; hint: string }[] =
  [
    { value: "zh-CN", label: "中文", hint: "界面显示为简体中文。" },
    { value: "en", label: "English", hint: "Use English for the interface." },
  ];

const themeOptions: { value: ThemePref; label: string; hint: string }[] = [
  { value: "dark", label: "深色", hint: "始终使用深色主题。" },
  { value: "light", label: "亮色", hint: "始终使用亮色主题。" },
  { value: "system", label: "系统", hint: "跟随 macOS 外观设置。" },
];

function applyTheme(theme: ThemePref) {
  const root = document.documentElement;
  root.classList.toggle("theme-dark", theme === "dark");
  root.classList.toggle("theme-light", theme === "light");
}

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
    // Agents status (MCP + global workflow) does not require workspace cwd.
    const [s, a] = await Promise.all([
      fetchSettings(),
      fetchAgentsStatus(undefined),
    ]);
    settings.value = s;
    applyTheme(s.theme);
    historyLimitInput.value = String(s.historyLimit);
    agents.value = a.agents;
  } finally {
    loading.value = false;
  }
}

async function onLanguageChange(language: LanguagePref) {
  languageSaving.value = true;
  actionMessage.value = null;
  try {
    settings.value = await setLanguage(language);
    actionOk.value = true;
    actionMessage.value = `语言已设为「${languageOptions.find((o) => o.value === language)?.label ?? language}」。`;
  } catch (e) {
    actionOk.value = false;
    actionMessage.value = e instanceof Error ? e.message : String(e);
  } finally {
    languageSaving.value = false;
  }
}

async function onThemeChange(theme: ThemePref) {
  themeSaving.value = true;
  actionMessage.value = null;
  try {
    const next = await setTheme(theme);
    settings.value = next;
    applyTheme(next.theme);
    actionOk.value = true;
    actionMessage.value = `主题已设为「${themeOptions.find((o) => o.value === theme)?.label ?? theme}」。`;
  } catch (e) {
    actionOk.value = false;
    actionMessage.value = e instanceof Error ? e.message : String(e);
  } finally {
    themeSaving.value = false;
  }
}

watch(
  () => settings.value?.historyLimit,
  (limit) => {
    if (typeof limit === "number" && !historySaving.value) {
      historyLimitInput.value = String(limit);
    }
  },
);

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
    const result: ActionResult = await installAgent(agent);
    actionOk.value = result.ok;
    actionMessage.value =
      result.message ?? (result.ok ? "完成。" : "安装失败。");
    if (result.status) {
      patchAgent(result.status);
    } else {
      const report = await fetchAgentsStatus(undefined);
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
    const result: ActionResult = await removeAgent(agent);
    actionOk.value = result.ok;
    actionMessage.value =
      result.message ?? (result.ok ? "完成。" : "移除失败。");
    if (result.status) {
      patchAgent(result.status);
    } else {
      const report = await fetchAgentsStatus(undefined);
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
  // Global user instruction files — workspace cwd is not required.
  const card = agents.value.find((a) => a.agent === agent);
  if (card && !card.canWriteWorkflow) {
    actionOk.value = false;
    actionMessage.value = card.workflowDetail ?? "无法写入工作流指令文件。";
    return;
  }
  busyWorkflow.value = agent;
  actionMessage.value = null;
  try {
    const result = await enableWorkflow(agent);
    actionOk.value = result.ok;
    actionMessage.value =
      result.message ?? (result.ok ? "已启用自动触发指令。" : "启用失败。");
    if (result.status) {
      patchAgent(result.status);
    } else {
      const report = await fetchAgentsStatus(undefined);
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
  // Global user instruction files — workspace cwd is not required.
  const card = agents.value.find((a) => a.agent === agent);
  if (card && !card.canWriteWorkflow) {
    actionOk.value = false;
    actionMessage.value = card.workflowDetail ?? "无法修改工作流指令文件。";
    return;
  }
  busyWorkflow.value = agent;
  actionMessage.value = null;
  try {
    const result = await disableWorkflow(agent);
    actionOk.value = result.ok;
    actionMessage.value =
      result.message ?? (result.ok ? "已禁用自动触发指令。" : "禁用失败。");
    if (result.status) {
      patchAgent(result.status);
    } else {
      const report = await fetchAgentsStatus(undefined);
      agents.value = report.agents;
    }
  } catch (e) {
    actionOk.value = false;
    actionMessage.value = e instanceof Error ? e.message : String(e);
  } finally {
    busyWorkflow.value = null;
  }
}

async function onSaveHistoryLimit() {
  const n = Number(historyLimitInput.value);
  if (!Number.isFinite(n) || n < 0 || n > 5000 || !Number.isInteger(n)) {
    actionOk.value = false;
    actionMessage.value = "历史条数上限必须是 0–5000 之间的整数。";
    return;
  }
  historySaving.value = true;
  actionMessage.value = null;
  try {
    settings.value = await setHistoryLimit(n);
    historyLimitInput.value = String(settings.value.historyLimit);
    actionOk.value = true;
    actionMessage.value = `历史保留上限已设为 ${settings.value.historyLimit}。`;
  } catch (e) {
    actionOk.value = false;
    actionMessage.value = e instanceof Error ? e.message : String(e);
  } finally {
    historySaving.value = false;
  }
}

async function onClearHistory() {
  if (!window.confirm("确定清空历史任务记录吗？运行中任务会被保留。")) {
    return;
  }
  historyClearing.value = true;
  actionMessage.value = null;
  try {
    const result = await clearHistory();
    settings.value = result.settings;
    historyLimitInput.value = String(result.settings.historyLimit);
    actionOk.value = true;
    actionMessage.value = `已清空 ${result.deleted} 条历史记录；保留 ${result.protected} 条运行中或受保护任务。`;
  } catch (e) {
    actionOk.value = false;
    actionMessage.value = e instanceof Error ? e.message : String(e);
  } finally {
    historyClearing.value = false;
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

        <fieldset class="field">
          <legend>语言</legend>
          <div class="radio-list compact" data-testid="language-controls">
            <label
              v-for="opt in languageOptions"
              :key="opt.value"
              class="radio-row"
            >
              <input
                type="radio"
                name="language"
                :value="opt.value"
                :checked="settings.language === opt.value"
                :disabled="languageSaving"
                @change="onLanguageChange(opt.value)"
              />
              <span>
                <strong>{{ opt.label }}</strong>
                <span class="hint">{{ opt.hint }}</span>
              </span>
            </label>
          </div>
        </fieldset>

        <fieldset class="field">
          <legend>主题</legend>
          <div class="radio-list compact" data-testid="theme-controls">
            <label
              v-for="opt in themeOptions"
              :key="opt.value"
              class="radio-row"
            >
              <input
                type="radio"
                name="theme"
                :value="opt.value"
                :checked="settings.theme === opt.value"
                :disabled="themeSaving"
                @change="onThemeChange(opt.value)"
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
            <span class="label">版本</span>
            <span data-testid="app-version">{{ settings.version }}</span>
          </div>
        </div>
      </div>

      <!-- Tools / Integrations -->
      <div
        v-else-if="section === 'integrations'"
        class="section integrations-section"
        data-testid="section-integrations"
      >
        <h2>工具开关</h2>
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
              <strong>自动触发指令</strong>
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
                <dt>指令文件（全局）</dt>
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
                  !card.canWriteWorkflow || busyWorkflow === card.agent
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
                  !card.canWriteWorkflow || busyWorkflow === card.agent
                "
                @click="onWorkflowDisable(card.agent)"
              >
                禁用
              </button>
            </div>
            <p
              v-if="!card.canWriteWorkflow"
              class="hint warn"
              data-testid="workflow-disabled-reason"
            >
              {{
                card.workflowDetail ||
                "无法安全写入指令文件（标记异常或路径不可用）。"
              }}
            </p>
            <p class="hint reminder" data-testid="workflow-reminder">
              写入用户级全局指令文件中的托管区块（
              <code>GrokTask:begin</code>
              …
              <code>end</code>
              ），不会改动 AskHuman 或其它用户内容。
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
        <form
          class="history-form"
          data-testid="history-limit-form"
          @submit.prevent="onSaveHistoryLimit"
        >
          <label>
            <span class="label">历史条数上限</span>
            <input
              v-model="historyLimitInput"
              type="number"
              min="0"
              max="5000"
              step="1"
              inputmode="numeric"
              data-testid="history-limit-input"
            />
            <span class="hint">本地 SQLite 保留的任务数，范围 0–5000。</span>
          </label>
          <button
            type="submit"
            :disabled="historySaving"
            data-testid="save-history-limit"
          >
            {{ historySaving ? "保存中…" : "保存" }}
          </button>
        </form>
        <div class="danger-zone">
          <div>
            <strong>清空历史</strong>
            <span class="hint">删除已完成/失败/取消等可清理任务；运行中与受保护任务会保留。</span>
          </div>
          <button
            type="button"
            class="danger"
            :disabled="historyClearing"
            data-testid="clear-history"
            @click="onClearHistory"
          >
            {{ historyClearing ? "清空中…" : "清空历史" }}
          </button>
        </div>
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
.integrations-section {
  margin: 0 16px 16px;
}
.integrations-section h2 {
  margin-left: 2px;
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
.radio-list.compact {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
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
  background: var(--card);
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
.section > button,
.history-form button,
.danger-zone button {
  border: 1px solid var(--border);
  background: var(--card);
  color: var(--fg);
  font-size: 12px;
  padding: 6px 12px;
  border-radius: 8px;
  cursor: pointer;
}
.card-actions button:disabled,
.section > button:disabled,
.history-form button:disabled,
.danger-zone button:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
.card-actions button.danger,
.danger-zone button.danger {
  color: var(--danger);
}
.history-form {
  display: flex;
  align-items: flex-end;
  gap: 10px;
  flex-wrap: wrap;
  margin-bottom: 16px;
}
.history-form label {
  min-width: 220px;
}
.history-form .label {
  display: block;
  font-size: 11px;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  color: var(--subtle);
  margin-bottom: 4px;
}
.history-form input {
  width: 160px;
  border: 1px solid var(--control-border);
  border-radius: var(--radius-md);
  background: var(--control-bg);
  color: var(--fg);
  padding: 7px 10px;
}
.danger-zone {
  display: flex;
  justify-content: space-between;
  gap: 16px;
  align-items: center;
  border-top: 1px solid var(--border);
  padding-top: 14px;
}
.danger-zone strong {
  display: block;
  font-size: 13px;
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
