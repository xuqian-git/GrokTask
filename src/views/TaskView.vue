<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import ComposerPlaceholder from "@/components/composer/ComposerPlaceholder.vue";
import HistorySidebar from "@/components/history/HistorySidebar.vue";
import ActivePlanBar from "@/components/plan/ActivePlanBar.vue";
import TimelineView from "@/components/timeline/TimelineView.vue";
import { fetchTaskDetail, fetchTaskList, sendTaskMessage } from "@/lib/ipc";
import { getSharedExpansion, replaceSharedExpansionKey } from "@/lib/uiState";
import type { ExpansionMap } from "@/lib/expansion";
import type { TaskDetail, TaskListItem } from "@/lib/types";

const props = defineProps<{
  /** Prefer this task when opening from History / external navigation. */
  initialTaskId?: string;
}>();

const tasks = ref<TaskListItem[]>([]);
/** Empty until list loads — never force a demo id in real Tauri flows. */
const selectedTaskId = ref<string>("");
const detail = ref<TaskDetail | null>(null);
const expansion = ref<ExpansionMap>({});
const loading = ref(true);
const refreshing = ref(false);
const sending = ref(false);
const error = ref<string | null>(null);
const sidebarCollapsed = ref(false);
let refreshTimer: number | null = null;

const statusLabel = computed(() => detail.value?.task.status ?? "");
const modeLabel = computed(() =>
  (detail.value?.task.mode ?? "read").toUpperCase(),
);

async function loadDetail(taskId: string) {
  loading.value = true;
  error.value = null;
  try {
    detail.value = await fetchTaskDetail(taskId);
    expansion.value = { ...getSharedExpansion(taskId) };
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
    detail.value = null;
  } finally {
    loading.value = false;
  }
}

async function refreshDetail(taskId: string) {
  try {
    const next = await fetchTaskDetail(taskId);
    detail.value = next;
    error.value = null;
  } catch (e) {
    // Keep the last good detail visible during transient daemon restarts.
    if (!detail.value) {
      error.value = e instanceof Error ? e.message : String(e);
    }
  }
}

async function refreshList(opts: { selectIfEmpty?: boolean } = {}) {
  refreshing.value = true;
  try {
    const next = await fetchTaskList();
    tasks.value = next;
    error.value = null;
    if (opts.selectIfEmpty && !selectedTaskId.value) {
      const preferred = props.initialTaskId;
      const inList =
        preferred && next.some((t) => t.taskId === preferred) ? preferred : "";
      selectedTaskId.value = inList || next[0]?.taskId || preferred || "";
    }
  } catch (e) {
    if (!tasks.value.length) {
      error.value = e instanceof Error ? e.message : String(e);
    }
  } finally {
    refreshing.value = false;
  }
}

function onSelect(taskId: string) {
  selectedTaskId.value = taskId;
}

function onExpansion(map: ExpansionMap) {
  expansion.value = map;
  if (selectedTaskId.value) {
    replaceSharedExpansionKey(selectedTaskId.value, map);
  }
}

async function onSend(text: string) {
  if (!selectedTaskId.value || sending.value) return;
  sending.value = true;
  error.value = null;
  try {
    await sendTaskMessage(selectedTaskId.value, text);
    await refreshList();
    await refreshDetail(selectedTaskId.value);
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  } finally {
    sending.value = false;
  }
}

// Selection is the single source of truth for detail loads (no mount double-call).
watch(selectedTaskId, (id) => {
  if (id) void loadDetail(id);
});

// When parent navigates to another task (history click), update selection.
watch(
  () => props.initialTaskId,
  (id) => {
    if (id && id !== selectedTaskId.value) {
      selectedTaskId.value = id;
    }
  },
);

onMounted(async () => {
  loading.value = true;
  error.value = null;
  try {
    await refreshList({ selectIfEmpty: false });
    // Prefer deep-linked task, else first list entry; never force a demo id.
    const preferred = props.initialTaskId;
    const inList =
      preferred && tasks.value.some((t) => t.taskId === preferred)
        ? preferred
        : "";
    const nextId = inList || tasks.value[0]?.taskId || preferred || "";
    if (!nextId) {
      loading.value = false;
      return;
    }
    if (selectedTaskId.value === nextId) {
      await loadDetail(nextId);
    } else {
      // Assignment triggers the watcher → loadDetail (with its own try/finally).
      selectedTaskId.value = nextId;
    }
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
    detail.value = null;
    loading.value = false;
  }

  refreshTimer = window.setInterval(() => {
    void refreshList();
    if (selectedTaskId.value) {
      void refreshDetail(selectedTaskId.value);
    }
  }, 1500);

  window.addEventListener("focus", onWindowFocus);
});

function onWindowFocus() {
  void refreshList({ selectIfEmpty: true });
  if (selectedTaskId.value) {
    void refreshDetail(selectedTaskId.value);
  }
}

onUnmounted(() => {
  if (refreshTimer !== null) {
    window.clearInterval(refreshTimer);
    refreshTimer = null;
  }
  window.removeEventListener("focus", onWindowFocus);
});
</script>

<template>
  <section class="task-shell" data-testid="task-shell">
    <HistorySidebar
      :tasks="tasks"
      :selected-task-id="selectedTaskId"
      :collapsed="sidebarCollapsed"
      @select="onSelect"
      @update:collapsed="sidebarCollapsed = $event"
    />

    <div class="task-main">
      <header v-if="detail" class="task-header" data-testid="task-header">
        <div class="header-left">
          <h2 data-testid="task-title">
            {{ detail.title }}
          </h2>
          <p class="meta">
            <span class="mode" data-testid="task-mode">{{ modeLabel }}</span>
            <span class="status" data-testid="task-status">{{
              statusLabel
            }}</span>
            <span v-if="detail.task.actualModel" class="model">{{
              detail.task.actualModel
            }}</span>
            <span v-if="detail.cwd" class="cwd" :title="detail.cwd">{{
              detail.cwd
            }}</span>
          </p>
        </div>
        <p v-if="detail.task.latestAction" class="action">
          {{ detail.task.latestAction }}
        </p>
        <span
          v-if="refreshing"
          class="refreshing"
          data-testid="task-refreshing"
          aria-label="正在刷新任务"
        />
      </header>

      <p v-if="loading" class="hint">加载任务…</p>
      <p v-else-if="error" class="hint error" data-testid="task-error">
        {{ error }}
      </p>
      <p
        v-else-if="!detail && !selectedTaskId"
        class="hint"
        data-testid="task-empty"
      >
        暂无任务
      </p>

      <template v-else-if="detail">
        <TimelineView
          :events="detail.timeline"
          :expansion="expansion"
          :last-sequence="detail.lastSequence"
          @update:expansion="onExpansion"
        />
        <ActivePlanBar v-if="detail.activePlan" :plan="detail.activePlan" />
        <ComposerPlaceholder
          :status="detail.task.status"
          :disabled="sending"
          @send="onSend"
        />
      </template>
    </div>
  </section>
</template>

<style scoped>
.task-shell {
  display: flex;
  height: 100%;
  min-height: 0;
  min-width: 0;
  padding: 12px;
  gap: 12px;
}
.task-main {
  flex: 1;
  min-width: 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  border: 1px solid var(--border);
  border-radius: var(--radius-lg);
  background: var(--card);
  box-shadow: var(--shadow);
}
.task-header {
  flex-shrink: 0;
  padding: 14px 16px;
  border-bottom: 1px solid var(--border);
  display: flex;
  flex-wrap: wrap;
  justify-content: space-between;
  align-items: flex-start;
  gap: 10px;
  background: var(--card-strong);
  backdrop-filter: blur(14px) saturate(1.15);
}
.task-header h2 {
  margin: 0 0 4px;
  font-size: 16px;
  letter-spacing: -0.01em;
}
.meta {
  margin: 0;
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  font-size: 12px;
  color: var(--subtle);
}
.mode {
  font-weight: 600;
  color: var(--accent);
}
.status {
  text-transform: lowercase;
}
.cwd {
  max-width: 240px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.action {
  margin: 0;
  font-size: 12px;
  color: var(--subtle);
  max-width: 40%;
}
.refreshing {
  width: 7px;
  height: 7px;
  margin-top: 7px;
  border-radius: 999px;
  background: var(--accent-green);
  box-shadow: 0 0 0 4px color-mix(in srgb, var(--accent-green) 18%, transparent);
}
.hint {
  padding: 16px;
  color: var(--subtle);
  font-size: 13px;
}
.hint.error {
  color: #b91c1c;
}
</style>
