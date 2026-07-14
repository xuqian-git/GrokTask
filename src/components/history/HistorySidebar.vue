<script setup lang="ts">
import { computed, ref } from "vue";
import type { TaskListItem } from "@/lib/types";

const props = defineProps<{
  tasks: TaskListItem[];
  selectedTaskId?: string;
  collapsed?: boolean;
}>();

const emit = defineEmits<{
  select: [taskId: string];
  "update:collapsed": [boolean];
}>();

const query = ref("");

function basename(cwd: string): string {
  if (!cwd) return "";
  const parts = cwd.replace(/\\/g, "/").split("/").filter(Boolean);
  return parts[parts.length - 1] || cwd;
}

function formatTime(iso?: string): string {
  if (!iso) return "";
  try {
    const d = new Date(iso);
    return d.toLocaleString(undefined, {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  } catch {
    return iso;
  }
}

function statusGlyph(status: string): string {
  switch (status) {
    case "running":
    case "starting":
    case "cancelling":
    case "recovering":
      return "●";
    case "idle":
      return "✓";
    case "failed":
      return "!";
    case "cancelled":
      return "–";
    case "interrupted":
      return "⚠";
    case "queued":
      return "○";
    default:
      return "·";
  }
}

const filtered = computed(() => {
  const q = query.value.trim().toLowerCase();
  if (!q) return props.tasks;
  return props.tasks.filter((t) => {
    const hay = [
      t.title,
      t.cwd,
      t.status,
      t.mode,
      t.actualModel,
      t.latestAction,
    ]
      .filter(Boolean)
      .join(" ")
      .toLowerCase();
    return hay.includes(q);
  });
});

/** Group by calendar day of updatedAt. */
const groups = computed(() => {
  const map = new Map<string, TaskListItem[]>();
  for (const t of filtered.value) {
    const day = dayLabel(t.updatedAt || t.finishedAt || t.createdAt);
    if (!map.has(day)) map.set(day, []);
    map.get(day)!.push(t);
  }
  return [...map.entries()];
});

function dayLabel(iso: string): string {
  try {
    const d = new Date(iso);
    const now = new Date();
    const startToday = new Date(
      now.getFullYear(),
      now.getMonth(),
      now.getDate(),
    );
    const startThat = new Date(d.getFullYear(), d.getMonth(), d.getDate());
    const diff = (startToday.getTime() - startThat.getTime()) / 86400000;
    if (diff === 0) return "今天";
    if (diff === 1) return "昨天";
    return d.toLocaleDateString(undefined, { month: "short", day: "numeric" });
  } catch {
    return "Earlier";
  }
}
</script>

<template>
  <aside
    class="history-sidebar"
    :class="{ collapsed }"
    data-testid="history-sidebar"
  >
    <div v-if="!collapsed" class="side-inner">
      <div class="side-head">
        <h2>最近任务</h2>
        <button
          type="button"
          class="collapse-btn"
          aria-label="Collapse history"
          data-testid="sidebar-collapse"
          @click="emit('update:collapsed', true)"
        >
          «
        </button>
      </div>
      <input
        v-model="query"
        type="search"
        class="search"
        placeholder="搜索任务…"
        data-testid="history-search"
      />
      <div class="task-groups">
        <section v-for="[day, items] in groups" :key="day" class="day-group">
          <h3 class="day-label">
            {{ day }}
          </h3>
          <ul class="task-list">
            <li
              v-for="t in items"
              :key="t.taskId"
              class="task-item"
              :class="{ selected: t.taskId === selectedTaskId }"
              :data-task-id="t.taskId"
              :data-status="t.status"
              @click="emit('select', t.taskId)"
            >
              <div class="row1">
                <span class="glyph" :data-status="t.status">{{
                  statusGlyph(t.status)
                }}</span>
                <strong class="title">{{ t.title }}</strong>
              </div>
              <div class="row2">
                <span class="cwd">{{ basename(t.cwd) }}</span>
                <span class="mode">{{ t.mode.toUpperCase() }}</span>
                <span v-if="t.actualModel" class="model">{{
                  t.actualModel
                }}</span>
              </div>
              <div class="row3">
                <span class="status">{{ t.status }}</span>
                <span class="time">{{
                  formatTime(t.finishedAt || t.updatedAt)
                }}</span>
              </div>
            </li>
          </ul>
        </section>
        <p v-if="!filtered.length" class="empty">无匹配任务</p>
      </div>
    </div>
    <button
      v-else
      type="button"
      class="expand-btn"
      aria-label="Expand history"
      data-testid="sidebar-expand"
      @click="emit('update:collapsed', false)"
    >
      »
    </button>
  </aside>
</template>

<style scoped>
.history-sidebar {
  width: 280px;
  flex-shrink: 0;
  border-right: 1px solid var(--border);
  background: var(--card);
  display: flex;
  flex-direction: column;
  min-height: 0;
  transition: width 0.15s ease;
}
.history-sidebar.collapsed {
  width: 36px;
}
.side-inner {
  display: flex;
  flex-direction: column;
  min-height: 0;
  height: 100%;
  padding: 10px 0;
}
.side-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 12px 8px;
}
.side-head h2 {
  margin: 0;
  font-size: 13px;
  font-weight: 600;
}
.collapse-btn,
.expand-btn {
  border: none;
  background: transparent;
  color: var(--subtle);
  cursor: pointer;
  font-size: 14px;
  padding: 4px 8px;
}
.expand-btn {
  width: 100%;
  height: 100%;
}
.search {
  margin: 0 12px 8px;
  padding: 6px 10px;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--bg);
  color: var(--fg);
  font-size: 12px;
}
.task-groups {
  flex: 1;
  overflow: auto;
  overscroll-behavior: contain;
  min-height: 0;
}
.day-label {
  margin: 8px 12px 4px;
  font-size: 11px;
  font-weight: 600;
  color: var(--subtle);
  text-transform: none;
}
.task-list {
  list-style: none;
  margin: 0;
  padding: 0;
}
.task-item {
  padding: 8px 12px;
  cursor: pointer;
  border-left: 3px solid transparent;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.task-item:hover {
  background: var(--bg);
}
.task-item.selected {
  background: var(--muted-bg);
  border-left-color: var(--muted-fg);
}
.row1 {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 13px;
}
.glyph {
  width: 1em;
  text-align: center;
  font-size: 10px;
  color: var(--muted-fg);
}
.glyph[data-status="failed"],
.glyph[data-status="interrupted"] {
  color: #b91c1c;
}
.glyph[data-status="running"],
.glyph[data-status="starting"] {
  color: #059669;
}
.title {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-weight: 600;
}
.row2,
.row3 {
  display: flex;
  gap: 8px;
  font-size: 11px;
  color: var(--subtle);
  padding-left: 1.4em;
}
.mode {
  font-weight: 600;
  color: var(--muted-fg);
}
.empty {
  padding: 16px;
  color: var(--subtle);
  font-size: 12px;
}
</style>
