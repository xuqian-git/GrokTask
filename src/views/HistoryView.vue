<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { fetchTaskList } from "@/lib/ipc";
import type { TaskListItem } from "@/lib/types";

const tasks = ref<TaskListItem[]>([]);
const loading = ref(true);
const error = ref<string | null>(null);
const query = ref("");
const statusFilter = ref<string>("all");
const modeFilter = ref<string>("all");
const showFilters = ref(false);

function basename(cwd: string): string {
  if (!cwd) return "";
  const parts = cwd.replace(/\\/g, "/").split("/").filter(Boolean);
  return parts[parts.length - 1] || cwd;
}

function formatTime(iso?: string): string {
  if (!iso) return "";
  try {
    return new Date(iso).toLocaleString("zh-CN");
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
    if (s < 60) return `${s} 秒`;
    return `${Math.round(s / 60)} 分钟`;
  } catch {
    return "";
  }
}

function statusLabel(status: string): string {
  const map: Record<string, string> = {
    idle: "空闲",
    running: "运行中",
    starting: "启动中",
    cancelling: "取消中",
    recovering: "恢复中",
    interrupted: "已中断",
    failed: "失败",
    cancelled: "已取消",
  };
  return map[status] ?? status;
}

function modeLabel(mode: string): string {
  return mode === "write" ? "写入" : mode === "read" ? "只读" : mode;
}

function dayKey(iso?: string): string {
  if (!iso) return "未知时间";
  try {
    const d = new Date(iso);
    return d.toLocaleDateString("zh-CN", {
      year: "numeric",
      month: "long",
      day: "numeric",
    });
  } catch {
    return "未知时间";
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

/** Group by calendar day of finishedAt/updatedAt (newest first). */
const groups = computed(() => {
  const map = new Map<string, TaskListItem[]>();
  for (const t of filtered.value) {
    const key = dayKey(t.finishedAt || t.updatedAt);
    const list = map.get(key) ?? [];
    list.push(t);
    map.set(key, list);
  }
  return Array.from(map.entries()).map(([label, items]) => ({ label, items }));
});

const showFilterBar = computed(
  () => showFilters.value || tasks.value.length > 8 || query.value.length > 0,
);

function openTask(taskId: string) {
  window.dispatchEvent(
    new CustomEvent("groktask-open-task", { detail: { taskId } }),
  );
}

onMounted(async () => {
  loading.value = true;
  error.value = null;
  try {
    tasks.value = await fetchTaskList();
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
    tasks.value = [];
  } finally {
    loading.value = false;
  }
});
</script>

<template>
  <section class="panel history" data-testid="history-view">
    <header class="hist-head">
      <div class="title-row">
        <h2>任务记录</h2>
        <span class="count" data-testid="history-count">
          {{ loading ? "…" : `${tasks.length} 条任务` }}
        </span>
      </div>
      <p class="subtitle">
        浏览本地 daemon 中的 Grok 任务时间线（非原始 JSON）。
      </p>

      <div v-if="showFilterBar" class="filters" data-testid="history-filters">
        <input
          v-model="query"
          type="search"
          placeholder="搜索标题 / 目录 / 结果…"
          data-testid="history-page-search"
        />
        <select v-model="statusFilter" data-testid="history-status-filter">
          <option value="all">全部状态</option>
          <option value="idle">空闲</option>
          <option value="running">运行中</option>
          <option value="failed">失败</option>
          <option value="cancelled">已取消</option>
          <option value="interrupted">已中断</option>
        </select>
        <select v-model="modeFilter" data-testid="history-mode-filter">
          <option value="all">全部模式</option>
          <option value="read">只读</option>
          <option value="write">写入</option>
        </select>
      </div>
      <button
        v-else
        type="button"
        class="filter-toggle"
        data-testid="history-show-filters"
        @click="showFilters = true"
      >
        筛选…
      </button>
    </header>

    <p v-if="loading" class="hint" data-testid="history-loading">加载中…</p>
    <p v-else-if="error" class="hint error" data-testid="history-error">
      {{ error }}
    </p>
    <div v-else-if="!tasks.length" class="empty" data-testid="history-empty">
      <p>暂无任务记录</p>
      <p class="sub">通过 MCP 或 CLI 提交任务后，记录会出现在这里。</p>
    </div>
    <div v-else class="groups" data-testid="history-groups">
      <section
        v-for="g in groups"
        :key="g.label"
        class="day-group"
        data-testid="history-day-group"
      >
        <h3 class="day-label">{{ g.label }}</h3>
        <ul class="task-list">
          <li
            v-for="t in g.items"
            :key="t.taskId"
            class="task-row"
            :data-task-id="t.taskId"
            :data-status="t.status"
            role="button"
            tabindex="0"
            data-testid="history-task-row"
            @click="openTask(t.taskId)"
            @keydown.enter="openTask(t.taskId)"
          >
            <div class="row-main">
              <strong>{{ t.title }}</strong>
              <span class="badges">
                <span class="mode">{{ modeLabel(t.mode) }}</span>
                <span class="status">{{ statusLabel(t.status) }}</span>
                <span v-if="t.actualModel" class="model">{{
                  t.actualModel
                }}</span>
              </span>
            </div>
            <div class="row-meta">
              <span>{{ basename(t.cwd) }}</span>
              <span>{{ formatTime(t.finishedAt || t.updatedAt) }}</span>
              <span v-if="durationLabel(t)">{{ durationLabel(t) }}</span>
            </div>
            <span v-if="t.latestAction" class="action">{{
              t.latestAction
            }}</span>
          </li>
        </ul>
      </section>
      <p v-if="!filtered.length" class="hint" data-testid="history-no-match">
        无匹配任务
      </p>
    </div>
  </section>
</template>

<style scoped>
.history {
  margin: 16px;
  max-width: 760px;
  flex: 1;
  min-height: 0;
  overflow: auto;
}
.hist-head {
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin-bottom: 12px;
}
.title-row {
  display: flex;
  align-items: baseline;
  gap: 10px;
}
.hist-head h2 {
  margin: 0;
  font-size: 18px;
}
.count {
  font-size: 12px;
  color: var(--subtle);
}
.subtitle {
  margin: 0;
  font-size: 13px;
  color: var(--subtle);
}
.filters {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-top: 4px;
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
.filter-toggle {
  align-self: flex-start;
  border: 1px solid var(--border);
  background: var(--card);
  color: var(--subtle);
  font-size: 12px;
  padding: 4px 10px;
  border-radius: 6px;
  cursor: pointer;
}
.day-group {
  margin-bottom: 16px;
}
.day-label {
  margin: 0 0 6px;
  font-size: 12px;
  font-weight: 600;
  color: var(--subtle);
  text-transform: none;
}
.task-list {
  list-style: none;
  margin: 0;
  padding: 0;
}
.task-row {
  padding: 12px 10px;
  border: 1px solid var(--border);
  border-radius: 10px;
  margin-bottom: 8px;
  display: flex;
  flex-direction: column;
  gap: 4px;
  font-size: 13px;
  cursor: pointer;
  background: var(--card);
}
.task-row:hover {
  border-color: var(--muted-fg);
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
.hint.error {
  color: #b91c1c;
}
.empty {
  padding: 32px 12px;
  text-align: center;
  color: var(--subtle);
}
.empty .sub {
  font-size: 12px;
  margin-top: 6px;
}
</style>
