<script setup lang="ts">
import {
  computed,
  nextTick,
  onBeforeUnmount,
  onMounted,
  ref,
  watch,
} from "vue";
import AggregateRow from "./AggregateRow.vue";
import TimelineItemCard from "./TimelineItemCard.vue";
import {
  attachScrollIntentListeners,
  createScrollController,
} from "@/lib/scroll";
import type { ExpansionMap } from "@/lib/expansion";
import { projectTimeline } from "@/lib/timelineProjection";
import type { TimelineEvent } from "@/lib/types";

const props = withDefaults(
  defineProps<{
    events: TimelineEvent[];
    expansion: ExpansionMap;
    compact?: boolean;
    lastSequence?: number;
  }>(),
  { compact: false, lastSequence: 0 },
);

const emit = defineEmits<{
  "update:expansion": [ExpansionMap];
}>();

const scroller = ref<HTMLElement | null>(null);
const scroll = createScrollController();
const showJump = ref(false);
const unread = ref(0);
let detachIntent: (() => void) | null = null;
let prevEventCount = 0;
let resizeObs: ResizeObserver | null = null;

const rows = computed(() => projectTimeline(props.events, props.expansion));

function syncJumpUi() {
  showJump.value = scroll.state === "detached-by-user";
  unread.value = scroll.unreadCount;
}

function onScroll() {
  if (!scroller.value) return;
  scroll.onScroll(scroller.value);
  syncJumpUi();
}

function jumpLatest() {
  if (!scroller.value) return;
  scroll.jumpToLatest(scroller.value);
  syncJumpUi();
}

function onExpansionUpdate(map: ExpansionMap) {
  emit("update:expansion", map);
}

watch(
  () => props.events,
  async (next, prev) => {
    const nextLen = next?.length ?? 0;
    const prevLen = prev?.length ?? prevEventCount;
    const grew = nextLen > prevLen;
    const seq = props.lastSequence;

    if (scroll.state === "detached-by-user" && grew) {
      scroll.notifyContentGrowth({
        newItemCount: nextLen - prevLen,
        lastSequence: seq,
      });
    } else if (scroll.state === "following-tail") {
      scroll.notifyContentGrowth({ newItemCount: 0, lastSequence: seq });
    }

    prevEventCount = nextLen;
    await nextTick();
    if (scroller.value) {
      scroll.maybeFollow(scroller.value);
      syncJumpUi();
    }
  },
  { deep: true },
);

watch(
  () => props.expansion,
  async () => {
    // Expansion/layout changes must not force jump when detached
    await nextTick();
    if (scroller.value) {
      scroll.maybeFollow(scroller.value);
      syncJumpUi();
    }
  },
  { deep: true },
);

onMounted(() => {
  prevEventCount = props.events.length;
  if (scroller.value) {
    detachIntent = attachScrollIntentListeners(scroller.value, scroll);
    scroll.jumpToLatest(scroller.value);
    syncJumpUi();

    if (typeof ResizeObserver !== "undefined") {
      resizeObs = new ResizeObserver(() => {
        if (scroller.value) {
          scroll.maybeFollow(scroller.value);
        }
      });
      resizeObs.observe(scroller.value);
      const content = scroller.value.firstElementChild;
      if (content) resizeObs.observe(content);
    }
  }
});

onBeforeUnmount(() => {
  detachIntent?.();
  resizeObs?.disconnect();
});

defineExpose({ scroll, scroller, rows });
</script>

<template>
  <div class="timeline-wrap" :class="{ compact }">
    <div
      ref="scroller"
      class="timeline-scroll"
      data-testid="timeline-scroll"
      @scroll="onScroll"
    >
      <div class="timeline-inner" data-testid="timeline-inner">
        <template v-for="row in rows" :key="row.key">
          <AggregateRow
            v-if="row.rowKind === 'aggregate' && row.aggregate"
            :members="row.aggregate.members"
            :primary-line="row.aggregate.primaryLine"
            :expansion="expansion"
            @update:expansion="onExpansionUpdate"
          />
          <TimelineItemCard
            v-else-if="row.event"
            :event="row.event"
            :expansion="expansion"
            :compact="compact"
            :class="{ 'agg-member': row.rowKind === 'aggregate_member' }"
            @update:expansion="onExpansionUpdate"
          />
        </template>
        <p v-if="!rows.length" class="empty" data-testid="timeline-empty">
          暂无活动
        </p>
      </div>
    </div>
    <button
      v-if="showJump"
      type="button"
      class="jump"
      data-testid="jump-latest"
      @click="jumpLatest"
    >
      <span v-if="unread > 0" class="jump-count" data-testid="unread-count">{{
        unread
      }}</span>
      回到最新
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
  min-height: 160px;
  max-height: 100%;
  overflow: auto;
  padding: 12px 16px;
  overscroll-behavior: contain;
}
.timeline-wrap.compact .timeline-scroll {
  padding: 8px 10px;
  min-height: 120px;
}
.timeline-inner {
  display: flex;
  flex-direction: column;
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
  display: inline-flex;
  align-items: center;
  gap: 6px;
  z-index: 2;
}
.jump-count {
  min-width: 18px;
  height: 18px;
  padding: 0 5px;
  border-radius: 999px;
  background: var(--muted-bg);
  color: var(--muted-fg);
  font-size: 11px;
  font-weight: 600;
  display: inline-flex;
  align-items: center;
  justify-content: center;
}
:deep(.agg-member) {
  margin-left: 12px;
  border-style: dashed;
}
</style>
