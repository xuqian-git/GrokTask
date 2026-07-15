<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import PopoverView from "./views/PopoverView.vue";
import TaskView from "./views/TaskView.vue";
import SettingsView from "./views/SettingsView.vue";

type Surface = "popover" | "task" | "settings";

const surface = ref<Surface>("task");
/** Optional task id when navigating from history / external open. */
const routeTaskId = ref<string | undefined>(undefined);

const title = computed(() => {
  switch (surface.value) {
    case "popover":
      return "GrokTask";
    case "settings":
      return "设置";
    default:
      return "任务记录";
  }
});

const showChrome = computed(() => surface.value !== "popover");

function applyRouteFromLocation() {
  const params = new URLSearchParams(window.location.search);
  const view = params.get("view");
  if (view === "popover" || view === "task" || view === "settings") {
    surface.value = view;
  } else if (view === "history") {
    // ACP records are now folded into the task timeline page.
    surface.value = "task";
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
    detail.view === "settings"
  ) {
    setSurface(detail.view, { taskId: detail.taskId });
  } else if (detail.view === "history") {
    setSurface("task", { taskId: detail.taskId });
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
      <nav class="nav" aria-label="主导航" data-testid="app-nav">
        <button
          type="button"
          data-testid="nav-task"
          :class="{ active: surface === 'task' }"
          @click="setSurface('task')"
        >
          任务记录
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
  background:
    radial-gradient(circle at 20% 0%, rgba(10, 132, 255, 0.1), transparent 28%),
    var(--bg);
  color: var(--fg);
}
.app-header {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px 18px;
  border-bottom: 1px solid var(--border);
  flex-shrink: 0;
  background: var(--card-strong);
  backdrop-filter: blur(18px) saturate(1.2);
}
.app-header h1 {
  margin: 0;
  font-size: 17px;
  font-weight: 600;
  letter-spacing: -0.01em;
}
.nav {
  margin-left: auto;
  display: flex;
  gap: 6px;
  padding: 3px;
  border: 1px solid var(--border);
  border-radius: 10px;
  background: var(--card);
}
.nav button {
  border: 1px solid transparent;
  background: transparent;
  color: var(--subtle);
  font-size: 13px;
  padding: 5px 12px;
  border-radius: 7px;
  cursor: pointer;
}
.nav button.active {
  background: var(--control-bg);
  color: var(--fg);
  border-color: var(--border);
  font-weight: 600;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.08);
}
.app-main {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}
</style>
