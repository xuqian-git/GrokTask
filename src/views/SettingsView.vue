<script setup lang="ts">
import { onMounted, onUnmounted, ref, watch } from "vue";
import {
  type ActionResult,
  type AgentId,
  type AgentIntegrationStatus,
  type DoctorReport,
  type SettingsSnapshot,
  type TrayMode,
  fetchAgentsStatus,
  fetchDoctorReport,
  fetchSettings,
  installAgent,
  removeAgent,
  setTrayMode,
} from "@/lib/settings";

type Section = "general" | "integrations" | "diagnostics" | "history";

const section = ref<Section>("general");
const settings = ref<SettingsSnapshot | null>(null);
const agents = ref<AgentIntegrationStatus[]>([]);
const doctor = ref<DoctorReport | null>(null);
const loading = ref(true);
const busyAgent = ref<AgentId | null>(null);
const actionMessage = ref<string | null>(null);
const actionOk = ref<boolean | null>(null);
const traySaving = ref(false);

const trayOptions: { value: TrayMode; label: string; hint: string }[] = [
  {
    value: "off",
    label: "Off",
    hint: "No tray icon; open via CLI or windows only. Login item removed.",
  },
  {
    value: "active",
    label: "When active",
    hint: "Tray while tasks are active. No login item.",
  },
  {
    value: "always",
    label: "Always",
    hint: "Tray always on; installs user login item for --gui-host.",
  },
];

function statusLabel(s: string): string {
  switch (s) {
    case "not_installed":
      return "Not installed";
    case "installed":
      return "Installed";
    case "outdated":
      return "Outdated";
    case "invalid_config":
      return "Invalid config";
    case "unavailable":
      return "Unavailable";
    default:
      return s;
  }
}

function agentTitle(id: AgentId): string {
  return id === "codex" ? "Codex" : "Claude Code";
}

async function refreshAll() {
  loading.value = true;
  actionMessage.value = null;
  actionOk.value = null;
  try {
    const [s, a, d] = await Promise.all([
      fetchSettings(),
      fetchAgentsStatus(),
      fetchDoctorReport(),
    ]);
    settings.value = s;
    agents.value = a.agents;
    doctor.value = d;
  } finally {
    loading.value = false;
  }
}

async function onTrayModeChange(mode: TrayMode) {
  traySaving.value = true;
  actionMessage.value = null;
  try {
    settings.value = await setTrayMode(mode);
    actionOk.value = true;
    actionMessage.value = `Tray mode set to “${mode}”.`;
  } catch (e) {
    actionOk.value = false;
    actionMessage.value = e instanceof Error ? e.message : String(e);
  } finally {
    traySaving.value = false;
  }
}

async function onInstall(agent: AgentId) {
  const card = agents.value.find((a) => a.agent === agent);
  if (card && !card.canWrite) {
    actionOk.value = false;
    actionMessage.value =
      card.detail ??
      "Cannot write: config is invalid or unavailable. Fix the file manually first.";
    return;
  }
  busyAgent.value = agent;
  actionMessage.value = null;
  try {
    const result: ActionResult = await installAgent(agent);
    actionOk.value = result.ok;
    actionMessage.value =
      result.message ?? (result.ok ? "Done." : "Install failed.");
    if (result.status) {
      agents.value = agents.value.map((a) =>
        a.agent === agent ? result.status! : a,
      );
    } else {
      const report = await fetchAgentsStatus();
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
    actionMessage.value =
      card.detail ??
      "Cannot remove: config is invalid or unavailable.";
    return;
  }
  busyAgent.value = agent;
  actionMessage.value = null;
  try {
    const result: ActionResult = await removeAgent(agent);
    actionOk.value = result.ok;
    actionMessage.value =
      result.message ?? (result.ok ? "Done." : "Remove failed.");
    if (result.status) {
      agents.value = agents.value.map((a) =>
        a.agent === agent ? result.status! : a,
      );
    } else {
      const report = await fetchAgentsStatus();
      agents.value = report.agents;
    }
  } catch (e) {
    actionOk.value = false;
    actionMessage.value = e instanceof Error ? e.message : String(e);
  } finally {
    busyAgent.value = null;
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
    section.value = detail;
  }
}

onMounted(() => {
  applySectionFromQuery();
  window.addEventListener("groktask-settings-section", onSettingsSectionEvent);
  void refreshAll();
});

onUnmounted(() => {
  window.removeEventListener(
    "groktask-settings-section",
    onSettingsSectionEvent,
  );
});

watch(section, () => {
  actionMessage.value = null;
  actionOk.value = null;
});
</script>

<template>
  <section class="settings" data-testid="settings-shell">
    <nav class="tabs" aria-label="Settings sections" data-testid="settings-tabs">
      <button
        type="button"
        data-testid="tab-general"
        :class="{ active: section === 'general' }"
        @click="section = 'general'"
      >
        General
      </button>
      <button
        type="button"
        data-testid="tab-integrations"
        :class="{ active: section === 'integrations' }"
        @click="section = 'integrations'"
      >
        Integrations
      </button>
      <button
        type="button"
        data-testid="tab-diagnostics"
        :class="{ active: section === 'diagnostics' }"
        @click="section = 'diagnostics'"
      >
        Diagnostics
      </button>
      <button
        type="button"
        data-testid="tab-history"
        :class="{ active: section === 'history' }"
        @click="section = 'history'"
      >
        History
      </button>
    </nav>

    <p v-if="loading" class="hint" data-testid="settings-loading">
      Loading…
    </p>

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
        <h2>General</h2>
        <fieldset class="field">
          <legend>Tray / menu bar icon</legend>
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
              >
              <span>
                <strong>{{ opt.label }}</strong>
                <span class="hint">{{ opt.hint }}</span>
              </span>
            </label>
          </div>
        </fieldset>

        <div class="meta-grid">
          <div>
            <span class="label">Language</span>
            <span data-testid="language-value">{{ settings.language }}</span>
            <span class="hint">Placeholder — full i18n later</span>
          </div>
          <div>
            <span class="label">Theme</span>
            <span data-testid="theme-value">{{ settings.theme }}</span>
            <span class="hint">Placeholder — follows system</span>
          </div>
          <div>
            <span class="label">Popover size</span>
            <span data-testid="popover-size">
              {{ settings.popoverWidth }}×{{ settings.popoverHeight }}
            </span>
          </div>
          <div>
            <span class="label">Max concurrent tasks</span>
            <span>{{ settings.maxConcurrentTasks }}</span>
          </div>
          <div>
            <span class="label">Version</span>
            <span data-testid="app-version">{{ settings.version }}</span>
          </div>
        </div>
      </div>

      <!-- Integrations -->
      <div
        v-else-if="section === 'integrations'"
        class="panel section"
        data-testid="section-integrations"
      >
        <h2>Agent integrations</h2>
        <p class="intro">
          Manage user-level MCP entries for Codex and Claude Code. Writes only
          the
          <code>groktask</code>
          server block. Restart or reload MCP in the agent after changes.
        </p>

        <article
          v-for="card in agents"
          :key="card.agent"
          class="card"
          :data-testid="`agent-card-${card.agent}`"
          :data-status="card.status"
        >
          <header class="card-head">
            <h3>{{ agentTitle(card.agent) }}</h3>
            <span
              class="status-pill"
              :data-status="card.status"
              data-testid="agent-status"
            >
              {{ statusLabel(card.status) }}
            </span>
          </header>
          <dl class="card-meta">
            <div>
              <dt>Config path</dt>
              <dd data-testid="agent-config-path">
                {{ card.configPath }}
              </dd>
            </div>
            <div>
              <dt>Binary (will write)</dt>
              <dd data-testid="agent-binary-path">
                {{ card.binaryPath }}
              </dd>
            </div>
            <div v-if="card.detail">
              <dt>Detail</dt>
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
                  ? "Update"
                  : card.status === "installed"
                    ? "Reinstall"
                    : "Install"
              }}
            </button>
            <button
              type="button"
              class="danger"
              data-testid="agent-remove"
              :disabled="!card.canRemove || busyAgent === card.agent"
              @click="onRemove(card.agent)"
            >
              Remove
            </button>
          </div>
          <p
            v-if="!card.canWrite || !card.canRemove"
            class="hint warn"
            data-testid="agent-disabled-reason"
          >
            {{
              card.detail ||
                "Write actions disabled because the config cannot be safely edited."
            }}
          </p>
          <p class="hint reminder" data-testid="agent-reminder">
            After Install / Update / Remove, restart the agent or reload MCP so
            the new configuration is picked up.
          </p>
        </article>
      </div>

      <!-- Diagnostics -->
      <div
        v-else-if="section === 'diagnostics'"
        class="panel section"
        data-testid="section-diagnostics"
      >
        <h2>Diagnostics</h2>
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
              <span class="label">Tray</span>
              <span data-testid="tray-capability">
                available={{ doctor.tray.trayAvailable }} · click={{
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
              <span v-if="doctor.grok.version" class="hint">version {{ doctor.grok.version }}</span>
              <span v-if="doctor.grok.guidance" class="hint warn">{{
                doctor.grok.guidance
              }}</span>
            </div>
          </div>
          <p class="hint">
            GrokTask never reads or stores xAI tokens and never starts
            interactive
            <code>grok login</code>
            automatically.
          </p>
          <button type="button" data-testid="refresh-doctor" @click="refreshAll">
            Refresh
          </button>
        </template>
      </div>

      <!-- History -->
      <div
        v-else
        class="panel section"
        data-testid="section-history"
      >
        <h2>History</h2>
        <div class="meta-grid">
          <div>
            <span class="label">History limit</span>
            <span data-testid="history-limit">{{ settings.historyLimit }}</span>
            <span class="hint">Tasks retained in local SQLite history</span>
          </div>
        </div>
        <p class="hint">
          Clear-history actions will land when storage retention APIs are exposed
          safely. No destructive clear is offered in this build.
        </p>
        <button type="button" disabled data-testid="clear-history-disabled">
          Clear history (unavailable)
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
  margin: 0 0 16px;
  font-size: 13px;
  color: var(--subtle);
  line-height: 1.45;
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
  margin-bottom: 10px;
}
.card-head h3 {
  margin: 0;
  font-size: 14px;
}
.status-pill {
  font-size: 11px;
  padding: 2px 8px;
  border-radius: 999px;
  background: var(--muted-bg);
  color: var(--muted-fg);
}
.status-pill[data-status="invalid_config"],
.status-pill[data-status="unavailable"] {
  background: #fef2f2;
  color: var(--danger);
}
.status-pill[data-status="installed"] {
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
