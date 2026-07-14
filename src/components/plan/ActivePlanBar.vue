<script setup lang="ts">
import { computed } from "vue";
import type { PlanDto } from "@/lib/types";

const props = withDefaults(
  defineProps<{
    plan: PlanDto;
    /** popover uses tighter max height fraction */
    compact?: boolean;
  }>(),
  { compact: false },
);

const entries = computed(() => props.plan.entries ?? []);
const total = computed(() => entries.value.length);
const completed = computed(
  () =>
    entries.value.filter((e) => {
      const s = (e.status ?? "").toLowerCase();
      return s === "completed" || s === "done" || s === "complete";
    }).length,
);

const currentLabel = computed(() => {
  if (props.plan.currentStep) return props.plan.currentStep;
  const running = entries.value.find((e) => {
    const s = (e.status ?? "").toLowerCase();
    return s === "running" || s === "in_progress" || s === "in-progress";
  });
  if (running) return running.content;
  const pending = entries.value.find((e) => {
    const s = (e.status ?? "").toLowerCase();
    return !s || s === "pending" || s === "todo";
  });
  return pending?.content ?? entries.value[0]?.content ?? "Plan";
});

function stepStatus(status?: string): string {
  const s = (status ?? "pending").toLowerCase();
  if (s === "completed" || s === "done" || s === "complete") return "completed";
  if (s === "running" || s === "in_progress" || s === "in-progress")
    return "running";
  if (s === "failed" || s === "error") return "failed";
  return "pending";
}

function stepIcon(status?: string): string {
  switch (stepStatus(status)) {
    case "completed":
      return "✓";
    case "running":
      return "◉";
    case "failed":
      return "✕";
    default:
      return "○";
  }
}
</script>

<template>
  <section
    class="plan-bar"
    :class="{ compact }"
    data-testid="active-plan-bar"
    :data-plan-id="plan.itemId"
  >
    <header class="plan-head">
      <span class="plan-label">当前步骤</span>
      <span class="plan-current" data-testid="plan-current">{{
        currentLabel
      }}</span>
      <span class="plan-count" data-testid="plan-count">{{ completed }}/{{ total }}</span>
    </header>
    <ol class="plan-steps" data-testid="plan-steps">
      <li
        v-for="(step, idx) in entries"
        :key="idx"
        class="plan-step"
        :data-status="stepStatus(step.status)"
        :data-priority="step.priority"
      >
        <span class="step-icon" aria-hidden="true">{{
          stepIcon(step.status)
        }}</span>
        <span class="step-text">{{ step.content }}</span>
        <span v-if="step.priority" class="step-priority">{{
          step.priority
        }}</span>
      </li>
    </ol>
  </section>
</template>

<style scoped>
.plan-bar {
  flex-shrink: 0;
  border-top: 1px solid var(--border);
  background: var(--card);
  max-height: min(320px, 35vh);
  display: flex;
  flex-direction: column;
  min-height: 0;
}
.plan-bar.compact {
  max-height: min(200px, 35%);
}
.plan-head {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 12px 4px;
  font-size: 12px;
}
.plan-label {
  color: var(--subtle);
  flex-shrink: 0;
}
.plan-current {
  flex: 1;
  min-width: 0;
  font-weight: 600;
  color: var(--fg);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.plan-count {
  font-variant-numeric: tabular-nums;
  color: var(--muted-fg);
  background: var(--muted-bg);
  padding: 1px 8px;
  border-radius: 999px;
  font-size: 11px;
}
.plan-steps {
  list-style: none;
  margin: 0;
  padding: 4px 12px 10px;
  overflow: auto;
  overscroll-behavior: contain;
  min-height: 0;
}
.plan-step {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  padding: 4px 0;
  font-size: 12px;
  color: var(--fg);
}
.plan-step[data-status="completed"] {
  color: var(--subtle);
}
.plan-step[data-status="completed"] .step-text {
  text-decoration: line-through;
}
.plan-step[data-status="running"] {
  font-weight: 600;
  color: var(--muted-fg);
}
.plan-step[data-status="failed"] {
  color: #b91c1c;
}
.step-icon {
  flex-shrink: 0;
  width: 1em;
  text-align: center;
}
.step-text {
  flex: 1;
  min-width: 0;
  word-break: break-word;
}
.step-priority {
  font-size: 10px;
  color: var(--subtle);
  text-transform: uppercase;
}
</style>
