<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import ComposerPlaceholder from "@/components/composer/ComposerPlaceholder.vue";
import ActivePlanBar from "@/components/plan/ActivePlanBar.vue";
import TimelineView from "@/components/timeline/TimelineView.vue";
import { fetchTaskDetail, fetchTaskList } from "@/lib/ipc";
import { getSharedExpansion, replaceSharedExpansionKey } from "@/lib/uiState";
import type { ExpansionMap } from "@/lib/expansion";
import type { TaskDetail, TaskListItem } from "@/lib/types";

const tasks = ref<TaskListItem[]>([]);
/** Empty until list loads — never force a demo id in real Tauri flows. */
const selectedTaskId = ref("");
const detail = ref<TaskDetail | null>(null);
const expansion = ref<ExpansionMap>({});
const loading = ref(true);
/** Monotonic token so only the latest loadDetail wins (async race guard). */
let detailLoadToken = 0;

const modeLabel = computed(() =>
  (detail.value?.task.mode ?? "read").toUpperCase(),
);

async function loadDetail(taskId: string) {
  const token = ++detailLoadToken;
  loading.value = true;
  try {
    const next = await fetchTaskDetail(taskId);
    // Drop stale responses if selection changed while awaiting IPC.
    if (token !== detailLoadToken) return;
    detail.value = next;
    // Same disclosure map as full window for this task
    expansion.value = { ...getSharedExpansion(taskId) };
  } finally {
    if (token === detailLoadToken) loading.value = false;
  }
}

function onExpansion(map: ExpansionMap) {
  expansion.value = map;
  if (selectedTaskId.value) {
    replaceSharedExpansionKey(selectedTaskId.value, map);
  }
}

function onSelectTask(id: string) {
  selectedTaskId.value = id;
}

// Selection is the single source of truth for detail loads (no mount double-call).
watch(selectedTaskId, (id) => {
  if (id) void loadDetail(id);
});

onMounted(async () => {
  tasks.value = await fetchTaskList();
  // Prefer a running/active task when present; else first list entry.
  const running = tasks.value.find((t) =>
    ["running", "starting", "cancelling", "recovering", "interrupted"].includes(
      t.status,
    ),
  );
  const nextId = running?.taskId ?? tasks.value[0]?.taskId ?? "";
  if (!nextId) {
    loading.value = false;
    return;
  }
  if (selectedTaskId.value === nextId) {
    // Value unchanged → watcher does not re-fire; load once explicitly.
    await loadDetail(nextId);
  } else {
    // Assignment triggers the watcher exactly once.
    selectedTaskId.value = nextId;
  }
});
</script>

<template>
  <section class="popover" data-testid="popover-shell">
    <header v-if="detail" class="pop-head" data-testid="popover-header">
      <div class="titles">
        <strong data-testid="popover-title">{{ detail.title }}</strong>
        <span class="meta">
          <span class="mode">{{ modeLabel }}</span>
          ·
          <span>{{ detail.task.status }}</span>
          <template v-if="detail.task.actualModel">
            · {{ detail.task.actualModel }}
          </template>
        </span>
      </div>
      <select
        v-if="tasks.length > 1"
        class="task-switch"
        data-testid="popover-task-switch"
        :value="selectedTaskId"
        @change="onSelectTask(($event.target as HTMLSelectElement).value)"
      >
        <option v-for="t in tasks" :key="t.taskId" :value="t.taskId">
          {{ t.title }}
        </option>
      </select>
    </header>

    <p v-if="loading" class="hint">加载中…</p>

    <template v-else-if="detail">
      <TimelineView
        :events="detail.timeline"
        :expansion="expansion"
        :last-sequence="detail.lastSequence"
        compact
        @update:expansion="onExpansion"
      />
      <ActivePlanBar
        v-if="detail.activePlan"
        :plan="detail.activePlan"
        compact
      />
      <ComposerPlaceholder :status="detail.task.status" compact />
    </template>

    <div v-else class="hint empty-state" data-testid="popover-empty">
      <p>暂无活动任务</p>
      <p class="sub">打开历史或设置继续</p>
    </div>
  </section>
</template>

<style scoped>
.popover {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
  max-width: 520px;
}
.pop-head {
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 10px 12px;
  border-bottom: 1px solid var(--border);
  background: var(--card);
}
.titles {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.titles strong {
  font-size: 13px;
  color: var(--fg);
}
.meta {
  font-size: 11px;
  color: var(--subtle);
}
.mode {
  font-weight: 600;
  color: var(--muted-fg);
}
.task-switch {
  font-size: 11px;
  border: 1px solid var(--border);
  border-radius: 6px;
  padding: 4px 6px;
  background: var(--bg);
  color: var(--fg);
}
.hint {
  padding: 12px;
  color: var(--subtle);
  font-size: 13px;
}
.empty-state {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  text-align: center;
}
.empty-state .sub {
  font-size: 12px;
  opacity: 0.8;
}
</style>
