<script setup lang="ts">
import { computed } from "vue";
import { renderMarkdown, looksLikeRawAcpJson } from "@/lib/markdown";
import { getExpansion, isExpanded, toggleUserExpansion } from "@/lib/expansion";
import type { ExpansionMap } from "@/lib/expansion";
import type { TimelineEvent } from "@/lib/types";

const props = defineProps<{
  event: TimelineEvent;
  expansion: ExpansionMap;
}>();

const emit = defineEmits<{
  "update:expansion": [ExpansionMap];
}>();

const state = computed(() => getExpansion(props.expansion, props.event.itemId));
const expanded = computed(() =>
  isExpanded(state.value, {
    streaming: props.event.streaming,
    kind: props.event.kind,
  }),
);

const primaryLine = computed(() => {
  const msg = props.event.message || props.event.stageTitle || props.event.title || "";
  if (looksLikeRawAcpJson(msg)) {
    return props.event.kind === "tool_call" ? "Working…" : "Update";
  }
  return msg || humanKind(props.event.kind);
});

const bodyHtml = computed(() => {
  if (props.event.kind === "assistant_segment" && !props.event.streaming) {
    return renderMarkdown(props.event.text);
  }
  // Streaming assistant: plain text (no full markdown reparse flicker)
  if (props.event.kind === "assistant_segment" && props.event.streaming) {
    return escapeText(props.event.text);
  }
  if (props.event.kind === "reasoning_segment" && expanded.value) {
    return renderMarkdown(props.event.text);
  }
  return "";
});

const isFinal = computed(() => props.event.answerMark === "finalAnswer");
const isPartial = computed(() => props.event.answerMark === "partialAnswer");

function humanKind(kind: string): string {
  switch (kind) {
    case "user_message":
      return "You";
    case "reasoning_segment":
      return props.event.streaming ? "Thinking…" : "Thought";
    case "assistant_segment":
      return "Reply";
    case "tool_call":
      return "Tool";
    case "permission_request":
      return "Permission";
    case "plan_snapshot":
      return "Plan";
    default:
      return kind.replace(/_/g, " ");
  }
}

function escapeText(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/\n/g, "<br>");
}

function onToggle() {
  emit("update:expansion", toggleUserExpansion(props.expansion, props.event.itemId));
}

const canToggle = computed(() =>
  ["reasoning_segment", "tool_call", "user_message", "plan_snapshot"].includes(
    props.event.kind,
  ),
);
</script>

<template>
  <article
    class="tl-item"
    :data-kind="event.kind"
    :data-item-id="event.itemId"
    :data-expanded="expanded ? '1' : '0'"
    :data-expansion="state"
  >
    <header class="tl-head" @click="canToggle && onToggle()">
      <span class="tl-kind">{{ humanKind(event.kind) }}</span>
      <span class="tl-message">{{ primaryLine }}</span>
      <span v-if="event.streaming" class="tl-stream">streaming</span>
      <span v-if="isFinal" class="tl-badge">final</span>
      <span v-if="isPartial" class="tl-badge partial">partial</span>
      <button
        v-if="canToggle"
        type="button"
        class="tl-toggle"
        :aria-expanded="expanded"
        @click.stop="onToggle"
      >
        {{ expanded ? "Collapse" : "Expand" }}
      </button>
    </header>

    <!-- User message -->
    <div v-if="event.kind === 'user_message'" class="tl-user">
      <p>{{ event.text }}</p>
    </div>

    <!-- Streaming assistant as plain text fragments (safe HTML from our renderer) -->
    <!-- eslint-disable-next-line vue/no-v-html -->
    <div
      v-else-if="event.kind === 'assistant_segment' && event.streaming"
      class="tl-reply stream"
      data-testid="reply-stream"
      v-html="bodyHtml"
    />

    <!-- Final markdown reply -->
    <!-- eslint-disable-next-line vue/no-v-html -->
    <div
      v-else-if="event.kind === 'assistant_segment'"
      class="tl-reply markdown"
      data-testid="reply-final"
      v-html="bodyHtml"
    />

    <!-- Thought body when expanded -->
    <!-- eslint-disable-next-line vue/no-v-html -->
    <div
      v-else-if="event.kind === 'reasoning_segment' && expanded"
      class="tl-thought"
      data-testid="thought-body"
      v-html="bodyHtml"
    />

    <!-- Tool details when expanded -->
    <div v-else-if="event.kind === 'tool_call' && expanded" class="tl-tool">
      <p v-if="event.locations?.length">
        {{ event.locations.join(", ") }}
      </p>
      <pre v-if="event.text">{{ event.text }}</pre>
    </div>
  </article>
</template>

<style scoped>
.tl-item {
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 10px 12px;
  background: var(--card);
  margin-bottom: 8px;
}
.tl-head {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  align-items: center;
  font-size: 13px;
}
.tl-kind {
  font-weight: 600;
  color: var(--muted-fg);
  text-transform: capitalize;
}
.tl-message {
  flex: 1;
  min-width: 0;
  color: var(--fg);
}
.tl-stream {
  font-size: 11px;
  color: var(--subtle);
}
.tl-badge {
  font-size: 10px;
  padding: 1px 6px;
  border-radius: 999px;
  background: var(--muted-bg);
  color: var(--muted-fg);
}
.tl-badge.partial {
  opacity: 0.8;
}
.tl-toggle {
  border: 1px solid var(--border);
  background: transparent;
  color: var(--subtle);
  border-radius: 6px;
  padding: 2px 8px;
  font-size: 11px;
  cursor: pointer;
}
.tl-user {
  margin-top: 8px;
  padding: 8px;
  background: var(--muted-bg);
  border-radius: 8px;
  font-size: 13px;
}
.tl-user p {
  margin: 0;
  color: var(--fg);
}
.tl-reply,
.tl-thought,
.tl-tool {
  margin-top: 8px;
  font-size: 13px;
  line-height: 1.5;
  color: var(--fg);
}
.tl-reply.stream {
  white-space: pre-wrap;
}
.tl-tool pre {
  margin: 4px 0 0;
  font-size: 12px;
  overflow: auto;
  max-height: 200px;
}
:deep(.tl-reply.markdown pre) {
  background: var(--muted-bg);
  padding: 8px;
  border-radius: 6px;
  overflow: auto;
}
:deep(.tl-reply.markdown code) {
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 12px;
}
</style>
