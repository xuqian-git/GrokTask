<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import PopoverView from "./views/PopoverView.vue";
import TaskView from "./views/TaskView.vue";
import HistoryView from "./views/HistoryView.vue";
import SettingsView from "./views/SettingsView.vue";

type Surface = "popover" | "task" | "history" | "settings";

const surface = ref<Surface>("task");
/** Optional task id when navigating from history / external open. */
const routeTaskId = ref<string | undefined>(undefined);

const title = computed(() => {
  switch (surface.value) {
    case "popover":
      return "GrokTask";
    case "history":
      return "ACP 记录";
    case "settings":
      return "设置";
    default:
      return "任务";
  }
});

const showChrome = computed(() => surface.value !== "popover");

function applyRouteFromLocation() {
  const params = new URLSearchParams(window.location.search);
  const view = params.get("view");
  if (
    view === "popover" ||
    view === "task" ||
    view === "history" ||
    view === "settings"
  ) {
    surface.value = view;
  }
  const task = params.get("task");
  routeTaskId.value = task && task.length > 0 ? task : undefined;
}

function setSurface(next: Surface, opts?: { taskId?: string }) {
  surface.value = next;
  if (opts?.taskId !== undefined) {
    routeTaskId.value = opts.taskId || undefined;
  }
  // Keep URL in sync so shell and settings tabs stay single-click consistent.
  const params = new URLSearchParams(window.location.search);
  params.set("view", next);
  if (next !== "settings") {
    params.delete("section");
  }
  if (opts?.taskId) {
    params.set("task", opts.taskId);
  } else if (next !== "task") {
    params.delete("task");
  }
  const qs = params.toString();
  window.history.replaceState({}, "", qs ? `?${qs}` : "?");
}

function onNavigate(ev: Event) {
  const detail = (ev as CustomEvent<{ view?: string; taskId?: string }>).detail;
  if (!detail?.view) return;
  if (
    detail.view === "popover" ||
    detail.view === "task" ||
    detail.view === "history" ||
    detail.view === "settings"
  ) {
    setSurface(detail.view, { taskId: detail.taskId });
  }
}

function onOpenTask(ev: Event) {
  const detail = (ev as CustomEvent<{ taskId?: string }>).detail;
  if (detail?.taskId) {
    setSurface("task", { taskId: detail.taskId });
  }
}

onMounted(() => {
  applyRouteFromLocation();
  window.addEventListener("groktask-navigate", onNavigate);
  window.addEventListener("groktask-open-task", onOpenTask);
});

onUnmounted(() => {
  window.removeEventListener("groktask-navigate", onNavigate);
  window.removeEventListener("groktask-open-task", onOpenTask);
});
</script>

<template>
  <div class="app-shell" :data-surface="surface" data-testid="app-shell">
    <header v-if="showChrome" class="app-header" data-testid="app-header">
      <h1>{{ title }}</h1>
      <span class="badge">本地工具</span>
      <nav class="nav" aria-label="主导航" data-testid="app-nav">
        <button
          type="button"
          data-testid="nav-task"
          :class="{ active: surface === 'task' }"
          @click="setSurface('task')"
        >
          任务
        </button>
        <button
          type="button"
          data-testid="nav-history"
          :class="{ active: surface === 'history' }"
          @click="setSurface('history')"
        >
          ACP 记录
        </button>
        <button
          type="button"
          data-testid="nav-settings"
          :class="{ active: surface === 'settings' }"
          @click="setSurface('settings')"
        >
          设置
        </button>
      </nav>
    </header>
    <main class="app-main">
      <PopoverView v-if="surface === 'popover'" />
      <TaskView v-else-if="surface === 'task'" :initial-task-id="routeTaskId" />
      <HistoryView v-else-if="surface === 'history'" />
      <SettingsView v-else />
    </main>
  </div>
</template>

<style scoped>
.app-shell {
  display: flex;
  flex-direction: column;
  min-height: 100vh;
  height: 100vh;
  background: var(--bg);
  color: var(--fg);
}
.app-header {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 16px;
  border-bottom: 1px solid var(--border);
  flex-shrink: 0;
  background: var(--card);
}
.app-header h1 {
  margin: 0;
  font-size: 16px;
  font-weight: 600;
}
.badge {
  font-size: 11px;
  padding: 2px 8px;
  border-radius: 999px;
  background: var(--muted-bg);
  color: var(--muted-fg);
}
.nav {
  margin-left: auto;
  display: flex;
  gap: 4px;
}
.nav button {
  border: 1px solid transparent;
  background: transparent;
  color: var(--subtle);
  font-size: 12px;
  padding: 4px 10px;
  border-radius: 6px;
  cursor: pointer;
}
.nav button.active {
  background: var(--muted-bg);
  color: var(--muted-fg);
  font-weight: 600;
}
.app-main {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}
</style>
