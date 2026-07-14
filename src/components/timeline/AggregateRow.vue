<script setup lang="ts">
import { computed } from "vue";
import {
  disclosureKey,
  getExpansion,
  toggleUserExpansion,
} from "@/lib/expansion";
import type { ExpansionMap } from "@/lib/expansion";
import { aggregateDisclosureItemId } from "@/lib/timelineProjection";
import type { TimelineEvent } from "@/lib/types";

const props = defineProps<{
  members: TimelineEvent[];
  primaryLine: string;
  expansion: ExpansionMap;
}>();

const emit = defineEmits<{
  "update:expansion": [ExpansionMap];
}>();

const firstId = computed(() => props.members[0]?.itemId ?? "unknown");
const aggItemId = computed(() => aggregateDisclosureItemId(firstId.value));
const state = computed(() =>
  getExpansion(props.expansion, aggItemId.value, "details"),
);
const expanded = computed(() => state.value === "user-expanded");

function onToggle() {
  emit(
    "update:expansion",
    toggleUserExpansion(props.expansion, aggItemId.value, "details"),
  );
}
</script>

<template>
  <article
    class="agg-row"
    data-kind="aggregate"
    :data-item-id="aggItemId"
    :data-expanded="expanded ? '1' : '0'"
    :data-expansion="state"
    :data-disclosure-key="disclosureKey(aggItemId, 'details')"
    data-testid="aggregate-row"
  >
    <header class="agg-head" @click="onToggle">
      <span class="agg-icon" aria-hidden="true">✓</span>
      <span class="agg-message" data-testid="aggregate-title">{{
        primaryLine
      }}</span>
      <span class="agg-count">{{ members.length }}</span>
      <button
        type="button"
        class="agg-toggle"
        :aria-expanded="expanded"
        data-testid="aggregate-toggle"
        @click.stop="onToggle"
      >
        {{ expanded ? "收起" : "展开" }}
      </button>
    </header>
    <!-- Members rendered as flat sibling rows by TimelineView when expanded -->
  </article>
</template>

<style scoped>
.agg-row {
  border: 1px dashed var(--border);
  border-radius: 10px;
  padding: 8px 12px;
  background: color-mix(in srgb, var(--card) 94%, var(--muted-bg));
  margin-bottom: 8px;
}
.agg-head {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
  cursor: pointer;
}
.agg-icon {
  color: var(--muted-fg);
}
.agg-message {
  flex: 1;
  font-weight: 500;
  color: var(--fg);
}
.agg-count {
  font-size: 11px;
  color: var(--subtle);
  padding: 1px 6px;
  border-radius: 999px;
  background: var(--muted-bg);
}
.agg-toggle {
  border: 1px solid var(--border);
  background: transparent;
  color: var(--subtle);
  border-radius: 6px;
  padding: 2px 8px;
  font-size: 11px;
  cursor: pointer;
}
</style>
