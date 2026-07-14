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
    <header class="app-header">
      <h1>{{ title }}</h1>
      <span class="badge">Phase 0–1</span>
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
  background: var(--bg);
  color: var(--fg);
}
.app-header {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px 16px;
  border-bottom: 1px solid var(--border);
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
.app-main {
  flex: 1;
  min-height: 0;
}
</style>
