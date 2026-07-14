<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import PopoverView from "./views/PopoverView.vue";
import TaskView from "./views/TaskView.vue";
import HistoryView from "./views/HistoryView.vue";
import SettingsView from "./views/SettingsView.vue";

type Surface = "popover" | "task" | "history" | "settings";

const surface = ref<Surface>("task");

const title = computed(() => {
  switch (surface.value) {
    case "popover":
      return "GrokTask";
    case "history":
      return "History";
    case "settings":
      return "Settings";
    default:
      return "Task";
  }
});

const showChrome = computed(() => surface.value !== "popover");

onMounted(() => {
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
});
</script>

<template>
  <div class="app-shell" :data-surface="surface">
    <header v-if="showChrome" class="app-header">
      <h1>{{ title }}</h1>
      <span class="badge">Phase 5</span>
      <nav class="nav" aria-label="Surfaces">
        <button
          type="button"
          :class="{ active: surface === 'task' }"
          @click="surface = 'task'"
        >
          Task
        </button>
        <button
          type="button"
          :class="{ active: surface === 'history' }"
          @click="surface = 'history'"
        >
          History
        </button>
        <button
          type="button"
          :class="{ active: surface === 'settings' }"
          @click="surface = 'settings'"
        >
          Settings
        </button>
        <button
          type="button"
          :class="{ active: surface === 'popover' }"
          @click="surface = 'popover'"
        >
          Popover
        </button>
      </nav>
    </header>
    <main class="app-main">
      <PopoverView v-if="surface === 'popover'" />
      <TaskView v-else-if="surface === 'task'" />
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
