<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import ComposerPlaceholder from "@/components/composer/ComposerPlaceholder.vue";
import HistorySidebar from "@/components/history/HistorySidebar.vue";
import ActivePlanBar from "@/components/plan/ActivePlanBar.vue";
import TimelineView from "@/components/timeline/TimelineView.vue";
import { fetchTaskDetail, fetchTaskList } from "@/lib/ipc";
import { getSharedExpansion, replaceSharedExpansionKey } from "@/lib/uiState";
import type { ExpansionMap } from "@/lib/expansion";
import type { TaskDetail, TaskListItem } from "@/lib/types";

const tasks = ref<TaskListItem[]>([]);
/** Empty until list loads — never force a demo id in real Tauri flows. */
const selectedTaskId = ref<string>("");
const detail = ref<TaskDetail | null>(null);
const expansion = ref<ExpansionMap>({});
const loading = ref(true);
const error = ref<string | null>(null);
const sidebarCollapsed = ref(false);

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

function onSelect(taskId: string) {
  selectedTaskId.value = taskId;
}

function onExpansion(map: ExpansionMap) {
  expansion.value = map;
  if (selectedTaskId.value) {
    replaceSharedExpansionKey(selectedTaskId.value, map);
  }
}

// Selection is the single source of truth for detail loads (no mount double-call).
watch(selectedTaskId, (id) => {
  if (id) void loadDetail(id);
});

onMounted(async () => {
  loading.value = true;
  error.value = null;
  try {
    tasks.value = await fetchTaskList();
    // Prefer first real task; never force a demo id when the list is empty.
    const nextId = tasks.value[0]?.taskId ?? "";
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
        <ComposerPlaceholder :status="detail.task.status" />
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
}
.task-main {
  flex: 1;
  min-width: 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
}
.task-header {
  flex-shrink: 0;
  padding: 12px 16px;
  border-bottom: 1px solid var(--border);
  display: flex;
  flex-wrap: wrap;
  justify-content: space-between;
  gap: 8px;
  background: var(--card);
}
.task-header h2 {
  margin: 0 0 4px;
  font-size: 15px;
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
  color: var(--muted-fg);
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
.hint {
  padding: 16px;
  color: var(--subtle);
  font-size: 13px;
}
.hint.error {
  color: #b91c1c;
}
</style>
