<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { fetchTaskList } from "@/lib/ipc";
import type { TaskListItem } from "@/lib/types";

const tasks = ref<TaskListItem[]>([]);
const loading = ref(true);
const query = ref("");
const statusFilter = ref<string>("all");
const modeFilter = ref<string>("all");

function basename(cwd: string): string {
  if (!cwd) return "";
  const parts = cwd.replace(/\\/g, "/").split("/").filter(Boolean);
  return parts[parts.length - 1] || cwd;
}

function formatTime(iso?: string): string {
  if (!iso) return "";
  try {
    return new Date(iso).toLocaleString();
  } catch {
    return iso;
  }
}

function durationLabel(t: TaskListItem): string {
  if (!t.finishedAt || !t.createdAt) return "";
  try {
    const ms =
      new Date(t.finishedAt).getTime() - new Date(t.createdAt).getTime();
    if (ms < 0) return "";
    const s = Math.round(ms / 1000);
    if (s < 60) return `${s}s`;
    return `${Math.round(s / 60)}m`;
  } catch {
    return "";
  }
}

const filtered = computed(() => {
  const q = query.value.trim().toLowerCase();
  return tasks.value.filter((t) => {
    if (statusFilter.value !== "all" && t.status !== statusFilter.value)
      return false;
    if (modeFilter.value !== "all" && t.mode !== modeFilter.value) return false;
    if (!q) return true;
    const hay = [t.title, t.cwd, t.latestAction, t.actualModel]
      .filter(Boolean)
      .join(" ")
      .toLowerCase();
    return hay.includes(q);
  });
});

onMounted(async () => {
  tasks.value = await fetchTaskList();
  loading.value = false;
});
</script>

<template>
  <section class="panel history" data-testid="history-view">
    <header class="hist-head">
      <h2>历史</h2>
      <div class="filters">
        <input
          v-model="query"
          type="search"
          placeholder="搜索 title / cwd / 结果…"
          data-testid="history-page-search"
        >
        <select v-model="statusFilter" data-testid="history-status-filter">
          <option value="all">
            全部状态
          </option>
          <option value="idle">
            idle
          </option>
          <option value="running">
            running
          </option>
          <option value="failed">
            failed
          </option>
          <option value="cancelled">
            cancelled
          </option>
          <option value="interrupted">
            interrupted
          </option>
        </select>
        <select v-model="modeFilter" data-testid="history-mode-filter">
          <option value="all">
            全部模式
          </option>
          <option value="read">
            read
          </option>
          <option value="write">
            write
          </option>
        </select>
      </div>
    </header>

    <p v-if="loading" class="hint">
      加载中…
    </p>
    <ul v-else class="task-list">
      <li
        v-for="t in filtered"
        :key="t.taskId"
        class="task-row"
        :data-task-id="t.taskId"
        :data-status="t.status"
      >
        <div class="row-main">
          <strong>{{ t.title }}</strong>
          <span class="badges">
            <span class="mode">{{ t.mode.toUpperCase() }}</span>
            <span class="status">{{ t.status }}</span>
            <span v-if="t.actualModel" class="model">{{ t.actualModel }}</span>
          </span>
        </div>
        <div class="row-meta">
          <span>{{ basename(t.cwd) }}</span>
          <span>{{ formatTime(t.finishedAt || t.updatedAt) }}</span>
          <span v-if="durationLabel(t)">{{ durationLabel(t) }}</span>
        </div>
        <span v-if="t.latestAction" class="action">{{ t.latestAction }}</span>
      </li>
      <li v-if="!filtered.length" class="hint">
        无匹配任务
      </li>
    </ul>
  </section>
</template>

<style scoped>
.history {
  margin: 16px;
  max-width: 720px;
}
.hist-head {
  display: flex;
  flex-direction: column;
  gap: 10px;
  margin-bottom: 8px;
}
.hist-head h2 {
  margin: 0;
}
.filters {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}
.filters input,
.filters select {
  font-size: 12px;
  padding: 6px 8px;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--bg);
  color: var(--fg);
}
.filters input {
  flex: 1;
  min-width: 160px;
}
.task-list {
  list-style: none;
  margin: 12px 0 0;
  padding: 0;
}
.task-row {
  padding: 12px 0;
  border-bottom: 1px solid var(--border);
  display: flex;
  flex-direction: column;
  gap: 4px;
  font-size: 13px;
}
.row-main {
  display: flex;
  flex-wrap: wrap;
  justify-content: space-between;
  gap: 8px;
}
.badges {
  display: flex;
  gap: 6px;
  font-size: 11px;
}
.mode {
  font-weight: 600;
  color: var(--muted-fg);
}
.status,
.model {
  color: var(--subtle);
}
.row-meta {
  display: flex;
  gap: 12px;
  font-size: 12px;
  color: var(--subtle);
}
.action {
  color: var(--muted-fg);
  font-size: 12px;
}
.hint {
  color: var(--subtle);
  font-size: 13px;
}
</style>
