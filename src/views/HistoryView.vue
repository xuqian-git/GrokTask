<script setup lang="ts">
import { onMounted, ref } from "vue";
import { fetchTaskList } from "@/lib/ipc";
import type { TaskListItem } from "@/lib/types";

const tasks = ref<TaskListItem[]>([]);
const loading = ref(true);

onMounted(async () => {
  tasks.value = await fetchTaskList();
  loading.value = false;
});
</script>

<template>
  <section class="panel history">
    <h2>History</h2>
    <p v-if="loading" class="hint">
      Loading…
    </p>
    <ul v-else class="task-list">
      <li v-for="t in tasks" :key="t.taskId">
        <strong>{{ t.title }}</strong>
        <span class="meta">{{ t.status }} · {{ t.mode }}</span>
        <span v-if="t.latestAction" class="action">{{ t.latestAction }}</span>
      </li>
      <li v-if="!tasks.length" class="hint">
        No tasks yet.
      </li>
    </ul>
  </section>
</template>

<style scoped>
.history {
  margin: 16px;
}
.task-list {
  list-style: none;
  margin: 12px 0 0;
  padding: 0;
}
.task-list li {
  padding: 10px 0;
  border-bottom: 1px solid var(--border);
  display: flex;
  flex-direction: column;
  gap: 2px;
  font-size: 13px;
}
.meta {
  color: var(--subtle);
  font-size: 12px;
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
