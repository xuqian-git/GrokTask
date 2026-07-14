<script setup lang="ts">
import { onMounted, ref } from "vue";
import TimelineView from "@/components/timeline/TimelineView.vue";
import { fetchTaskDetail } from "@/lib/ipc";
import type { ExpansionMap } from "@/lib/expansion";
import type { TaskDetail } from "@/lib/types";

const detail = ref<TaskDetail | null>(null);
const expansion = ref<ExpansionMap>({});
const loading = ref(true);
const error = ref<string | null>(null);

onMounted(async () => {
  try {
    detail.value = await fetchTaskDetail();
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  } finally {
    loading.value = false;
  }
});
</script>

<template>
  <section class="task-view">
    <header v-if="detail" class="task-header">
      <div>
        <h2>{{ detail.title }}</h2>
        <p class="meta">
          <span class="mode">{{ detail.task.mode.toUpperCase() }}</span>
          <span>{{ detail.task.status }}</span>
          <span v-if="detail.task.actualModel">{{ detail.task.actualModel }}</span>
        </p>
      </div>
      <p v-if="detail.task.latestAction" class="action">
        {{ detail.task.latestAction }}
      </p>
    </header>

    <p v-if="loading" class="hint">
      Loading task…
    </p>
    <p v-else-if="error" class="hint error">
      {{ error }}
    </p>

    <TimelineView
      v-else-if="detail"
      :events="detail.timeline"
      :expansion="expansion"
      @update:expansion="expansion = $event"
    />
  </section>
</template>

<style scoped>
.task-view {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
}
.task-header {
  padding: 12px 16px;
  border-bottom: 1px solid var(--border);
  display: flex;
  flex-wrap: wrap;
  justify-content: space-between;
  gap: 8px;
}
.task-header h2 {
  margin: 0 0 4px;
  font-size: 15px;
}
.meta {
  margin: 0;
  display: flex;
  gap: 10px;
  font-size: 12px;
  color: var(--subtle);
}
.mode {
  font-weight: 600;
  color: var(--muted-fg);
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
