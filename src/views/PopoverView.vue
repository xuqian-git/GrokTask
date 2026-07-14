<script setup lang="ts">
import { onMounted, ref } from "vue";
import TimelineView from "@/components/timeline/TimelineView.vue";
import { fetchTaskDetail } from "@/lib/ipc";
import type { ExpansionMap } from "@/lib/expansion";
import type { TaskDetail } from "@/lib/types";

const detail = ref<TaskDetail | null>(null);
const expansion = ref<ExpansionMap>({});

onMounted(async () => {
  detail.value = await fetchTaskDetail();
});
</script>

<template>
  <section class="popover">
    <header v-if="detail" class="pop-head">
      <strong>{{ detail.title }}</strong>
      <span>{{ detail.task.status }} · {{ detail.task.mode }}</span>
    </header>
    <TimelineView
      v-if="detail"
      :events="detail.timeline"
      :expansion="expansion"
      @update:expansion="expansion = $event"
    />
    <p v-else class="hint">
      Loading…
    </p>
  </section>
</template>

<style scoped>
.popover {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
}
.pop-head {
  display: flex;
  justify-content: space-between;
  gap: 8px;
  padding: 10px 12px;
  border-bottom: 1px solid var(--border);
  font-size: 12px;
  color: var(--subtle);
}
.pop-head strong {
  color: var(--fg);
}
.hint {
  padding: 12px;
  color: var(--subtle);
  font-size: 13px;
}
</style>
