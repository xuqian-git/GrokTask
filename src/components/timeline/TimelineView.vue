<script setup lang="ts">
import { nextTick, onMounted, ref, watch } from "vue";
import TimelineItemCard from "./TimelineItemCard.vue";
import { createScrollController } from "@/lib/scroll";
import type { ExpansionMap } from "@/lib/expansion";
import type { TimelineEvent } from "@/lib/types";

const props = defineProps<{
  events: TimelineEvent[];
  expansion: ExpansionMap;
}>();

const emit = defineEmits<{
  "update:expansion": [ExpansionMap];
}>();

const scroller = ref<HTMLElement | null>(null);
const scroll = createScrollController();
const showJump = ref(false);

function onWheel() {
  scroll.markUserIntent();
}

function onScroll() {
  if (!scroller.value) return;
  scroll.onScroll(scroller.value);
  showJump.value = scroll.state === "detached-by-user";
}

function jumpLatest() {
  if (!scroller.value) return;
  scroll.jumpToLatest(scroller.value);
  showJump.value = false;
}

watch(
  () => props.events,
  async () => {
    await nextTick();
    if (scroller.value) {
      scroll.maybeFollow(scroller.value);
      showJump.value = scroll.state === "detached-by-user";
    }
  },
  { deep: true },
);

onMounted(() => {
  if (scroller.value) {
    scroll.jumpToLatest(scroller.value);
  }
});

defineExpose({ scroll, scroller });
</script>

<template>
  <div class="timeline-wrap">
    <div
      ref="scroller"
      class="timeline-scroll"
      data-testid="timeline-scroll"
      @wheel="onWheel"
      @scroll="onScroll"
    >
      <TimelineItemCard
        v-for="ev in events"
        :key="ev.itemId"
        :event="ev"
        :expansion="expansion"
        @update:expansion="emit('update:expansion', $event)"
      />
      <p v-if="!events.length" class="empty">
        No activity yet.
      </p>
    </div>
    <button
      v-if="showJump"
      type="button"
      class="jump"
      data-testid="jump-latest"
      @click="jumpLatest"
    >
      Jump to latest
    </button>
  </div>
</template>

<style scoped>
.timeline-wrap {
  position: relative;
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}
.timeline-scroll {
  flex: 1;
  min-height: 200px;
  max-height: 100%;
  overflow: auto;
  padding: 12px 16px;
  overscroll-behavior: contain;
}
.empty {
  color: var(--subtle);
  font-size: 13px;
}
.jump {
  position: absolute;
  right: 20px;
  bottom: 16px;
  border: 1px solid var(--border);
  background: var(--card);
  color: var(--fg);
  border-radius: 999px;
  padding: 6px 12px;
  font-size: 12px;
  cursor: pointer;
  box-shadow: 0 2px 8px rgb(0 0 0 / 12%);
}
</style>
