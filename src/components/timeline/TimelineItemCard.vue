<script setup lang="ts">
import { computed } from "vue";
import { renderMarkdown, safeDisplayLine } from "@/lib/markdown";
import {
  disclosureKey,
  getExpansion,
  isExpanded,
  toggleUserExpansion,
} from "@/lib/expansion";
import type { ExpansionMap } from "@/lib/expansion";
import { thoughtPreviewLines, thoughtStageSummary } from "@/lib/thoughtSummary";
import {
  toolDetailPaths,
  toolDetailText,
  toolPrimaryLine,
  toolStatusIcon,
  toolVisualStatus,
} from "@/lib/toolDisplay";
import type { TimelineEvent } from "@/lib/types";

const props = withDefaults(
  defineProps<{
    event: TimelineEvent;
    expansion: ExpansionMap;
    /** Compact density for popover. */
    compact?: boolean;
  }>(),
  { compact: false },
);

const emit = defineEmits<{
  "update:expansion": [ExpansionMap];
}>();

const part = computed(() =>
  props.event.kind === "user_message"
    ? ("body" as const)
    : ("details" as const),
);

const state = computed(() =>
  getExpansion(props.expansion, props.event.itemId, part.value),
);

const expanded = computed(() =>
  isExpanded(state.value, {
    streaming: props.event.streaming,
    kind: props.event.kind,
  }),
);

/** Reasoning: auto streaming → 3-line preview; auto completed → summary only. */
const reasoningMode = computed(() => {
  if (props.event.kind !== "reasoning_segment") return null;
  if (state.value === "user-expanded") return "full" as const;
  if (state.value === "user-collapsed") return "summary" as const;
  // auto
  if (props.event.streaming) return "preview" as const;
  return "summary" as const;
});

const reasoningTitle = computed(() => {
  if (props.event.kind !== "reasoning_segment") return "";
  if (props.event.streaming && state.value !== "user-collapsed") {
    return "正在思考";
  }
  return thoughtStageSummary({
    stageTitle: props.event.stageTitle,
    message: props.event.message,
    text: props.event.text,
  });
});

const reasoningPreview = computed(() => {
  if (props.event.kind !== "reasoning_segment") return "";
  return thoughtPreviewLines(props.event.text, 3);
});

const toolLine = computed(() => {
  if (props.event.kind !== "tool_call") return "";
  return toolPrimaryLine(props.event);
});

const permissionLine = computed(() => {
  if (props.event.kind !== "permission_request") return "";
  return safeDisplayLine(props.event.message || props.event.title, "权限请求");
});

const permissionStatusLabel = computed(() => {
  if (props.event.kind !== "permission_request") return "";
  switch (props.event.status) {
    case "approved":
      return "自动允许";
    case "rejected":
      return "自动拒绝";
    case "requesting":
      return "处理中";
    case "pending":
      return "待处理";
    default:
      return props.event.status ?? "";
  }
});

const planLine = computed(() => {
  if (props.event.kind !== "plan_snapshot") return "";
  const n = props.event.planEntries?.length ?? 0;
  return safeDisplayLine(
    props.event.message || props.event.title,
    `计划 · ${n} 步`,
  );
});

const noticeLine = computed(() => {
  if (props.event.kind !== "context_notice") return "";
  return safeDisplayLine(props.event.message || props.event.text, "状态提示");
});

const primaryLine = computed(() => {
  switch (props.event.kind) {
    case "user_message":
      return "你";
    case "reasoning_segment":
      return reasoningTitle.value;
    case "assistant_segment":
      return props.event.streaming ? "回复中" : "回复";
    case "tool_call":
      return toolLine.value;
    case "permission_request":
      return permissionLine.value;
    case "plan_snapshot":
      return planLine.value;
    case "context_notice":
      return noticeLine.value;
    default: {
      return safeDisplayLine(
        props.event.message || props.event.title,
        props.event.kind.replace(/_/g, " ") || "Update",
      );
    }
  }
});

const bodyHtml = computed(() => {
  if (props.event.kind === "assistant_segment" && !props.event.streaming) {
    return renderMarkdown(props.event.text);
  }
  if (props.event.kind === "assistant_segment" && props.event.streaming) {
    return escapeText(props.event.text);
  }
  if (
    props.event.kind === "reasoning_segment" &&
    reasoningMode.value === "full"
  ) {
    return renderMarkdown(props.event.text);
  }
  if (
    props.event.kind === "reasoning_segment" &&
    reasoningMode.value === "preview"
  ) {
    return renderMarkdown(reasoningPreview.value);
  }
  return "";
});

const isFinal = computed(() => props.event.answerMark === "finalAnswer");
const isPartial = computed(() => props.event.answerMark === "partialAnswer");

const toolStatus = computed(() => toolVisualStatus(props.event.status));

function escapeText(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/\n/g, "<br>");
}

function onToggle() {
  emit(
    "update:expansion",
    toggleUserExpansion(props.expansion, props.event.itemId, part.value),
  );
}

const showUserBody = computed(() => {
  if (props.event.kind !== "user_message") return false;
  // Long user messages can collapse; short always show
  const long = (props.event.text?.length ?? 0) > 280;
  if (!long) return true;
  return expanded.value || state.value === "auto";
});

const userCollapsed = computed(() => {
  if (props.event.kind !== "user_message") return false;
  const long = (props.event.text?.length ?? 0) > 280;
  return long && state.value === "user-collapsed";
});

const disclosurePartKey = computed(() =>
  disclosureKey(props.event.itemId, part.value),
);
</script>

<template>
  <article
    class="tl-item"
    :class="[
      `kind-${event.kind}`,
      {
        compact,
        streaming: event.streaming,
        final: isFinal,
        'is-lightweight': !['user_message', 'assistant_segment'].includes(
          event.kind,
        ),
      },
    ]"
    :data-kind="event.kind"
    :data-item-id="event.itemId"
    :data-expanded="expanded ? '1' : '0'"
    :data-expansion="state"
    :data-disclosure-key="disclosurePartKey"
    :data-tool-status="event.kind === 'tool_call' ? toolStatus : undefined"
  >
    <!-- User message -->
    <template v-if="event.kind === 'user_message'">
      <div class="tl-user" data-testid="user-message">
        <div class="tl-user-meta">
          <span class="tl-role">你</span>
          <button
            v-if="(event.text?.length ?? 0) > 280"
            type="button"
            class="tl-toggle"
            :aria-expanded="!userCollapsed"
            @click="onToggle"
          >
            {{ userCollapsed ? "展开" : "收起" }}
          </button>
        </div>
        <p v-if="showUserBody && !userCollapsed" class="tl-user-text">
          {{ event.text }}
        </p>
        <p v-else-if="userCollapsed" class="tl-user-text collapsed">
          {{ event.text.slice(0, 120) }}…
        </p>
      </div>
    </template>

    <!-- Reasoning stage -->
    <template v-else-if="event.kind === 'reasoning_segment'">
      <header class="tl-head thought-head" @click="onToggle">
        <span class="tl-icon" aria-hidden="true">{{
          event.streaming ? "◎" : "💭"
        }}</span>
        <span class="tl-message" data-testid="thought-title">{{
          reasoningTitle
        }}</span>
        <span
          v-if="event.streaming"
          class="tl-stream"
          data-testid="thought-streaming"
          >streaming</span
        >
        <button
          type="button"
          class="tl-toggle"
          :aria-expanded="reasoningMode === 'full'"
          data-testid="thought-toggle"
          @click.stop="onToggle"
        >
          {{ reasoningMode === "full" ? "收起" : "展开" }}
        </button>
      </header>
      <!-- eslint-disable-next-line vue/no-v-html -->
      <div
        v-if="reasoningMode === 'preview'"
        class="tl-thought preview"
        data-testid="thought-preview"
        v-html="bodyHtml"
      />
      <!-- eslint-disable-next-line vue/no-v-html -->
      <div
        v-else-if="reasoningMode === 'full'"
        class="tl-thought full"
        data-testid="thought-body"
        v-html="bodyHtml"
      />
      <p v-else class="tl-thought-summary" data-testid="thought-summary">
        {{ reasoningTitle }}
      </p>
    </template>

    <!-- Tool call -->
    <template v-else-if="event.kind === 'tool_call'">
      <header class="tl-head tool-head" @click="onToggle">
        <span class="tl-icon status" aria-hidden="true">{{
          toolStatusIcon(event.status)
        }}</span>
        <span class="tl-message" data-testid="tool-title">{{ toolLine }}</span>
        <span
          v-if="toolStatus === 'running' || toolStatus === 'pending'"
          class="tl-stream"
          >{{ toolStatus }}</span
        >
        <button
          type="button"
          class="tl-toggle"
          :aria-expanded="expanded"
          data-testid="tool-toggle"
          @click.stop="onToggle"
        >
          {{ expanded ? "收起" : "详情" }}
        </button>
      </header>
      <div v-if="expanded" class="tl-tool" data-testid="tool-details">
        <p v-if="toolDetailPaths(event).length" class="tl-paths">
          <span
            v-for="p in toolDetailPaths(event)"
            :key="p"
            class="path-chip"
            >{{ p }}</span
          >
        </p>
        <pre v-if="toolDetailText(event)">{{ toolDetailText(event) }}</pre>
        <p v-else-if="!toolDetailPaths(event).length" class="tl-empty-detail">
          无额外详情
        </p>
      </div>
    </template>

    <!-- Assistant reply -->
    <template v-else-if="event.kind === 'assistant_segment'">
      <header class="tl-head reply-head">
        <span class="tl-role">Grok</span>
        <span v-if="event.streaming" class="tl-stream">streaming</span>
        <span v-if="isFinal" class="tl-badge" data-testid="final-badge"
          >final</span
        >
        <span v-if="isPartial" class="tl-badge partial">partial</span>
      </header>
      <!-- eslint-disable-next-line vue/no-v-html -->
      <div
        v-if="event.streaming"
        class="tl-reply stream"
        data-testid="reply-stream"
        v-html="bodyHtml"
      />
      <!-- eslint-disable-next-line vue/no-v-html -->
      <div
        v-else
        class="tl-reply markdown"
        data-testid="reply-final"
        v-html="bodyHtml"
      />
    </template>

    <!-- Permission -->
    <template v-else-if="event.kind === 'permission_request'">
      <div class="tl-permission" data-testid="permission-row">
        <span class="tl-icon" aria-hidden="true">🔒</span>
        <span class="tl-message">{{ permissionLine }}</span>
        <span v-if="permissionStatusLabel" class="tl-badge">{{
          permissionStatusLabel
        }}</span>
      </div>
    </template>

    <!-- Historical plan snapshot -->
    <template v-else-if="event.kind === 'plan_snapshot'">
      <header class="tl-head" @click="onToggle">
        <span class="tl-icon" aria-hidden="true">📋</span>
        <span class="tl-message">{{ planLine }}</span>
        <button
          type="button"
          class="tl-toggle"
          :aria-expanded="expanded"
          @click.stop="onToggle"
        >
          {{ expanded ? "收起" : "展开" }}
        </button>
      </header>
      <ol v-if="expanded && event.planEntries?.length" class="tl-plan-steps">
        <li
          v-for="(step, idx) in event.planEntries"
          :key="idx"
          :data-status="step.status"
        >
          {{ step.content }}
        </li>
      </ol>
    </template>

    <!-- Context notice -->
    <template v-else-if="event.kind === 'context_notice'">
      <div class="tl-notice" data-testid="context-notice">
        <span class="tl-icon" aria-hidden="true">ℹ</span>
        <span>{{ noticeLine }}</span>
      </div>
    </template>

    <!-- Fallback -->
    <template v-else>
      <header class="tl-head">
        <span class="tl-message">{{ primaryLine }}</span>
      </header>
    </template>
  </article>
</template>

<style scoped>
.tl-item {
  position: relative;
  padding: 10px 4px 12px 18px;
  margin-bottom: 6px;
  background: transparent;
}
.tl-item.compact {
  padding-top: 7px;
  padding-bottom: 9px;
  margin-bottom: 4px;
}
.tl-item.is-lightweight::before {
  content: "";
  position: absolute;
  left: 5px;
  top: 14px;
  bottom: -8px;
  width: 1px;
  background: color-mix(in srgb, var(--border) 72%, transparent);
}
.tl-item.is-lightweight::after {
  content: "";
  position: absolute;
  left: 2px;
  top: 17px;
  width: 7px;
  height: 7px;
  border-radius: 999px;
  background: color-mix(in srgb, var(--fg) 32%, var(--bg));
}
.tl-item.kind-user_message {
  padding-left: 0;
  padding-right: 0;
}
.tl-item.kind-reasoning_segment {
  color: var(--subtle);
}
.tl-item.kind-context_notice {
  color: var(--subtle);
}
.tl-item.kind-permission_request {
  color: var(--subtle);
}
.tl-item.kind-assistant_segment {
  padding-left: 0;
  padding-right: 0;
}
.tl-head {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  align-items: center;
  font-size: 13px;
  cursor: default;
}
.thought-head,
.tool-head {
  cursor: pointer;
}
.tl-icon {
  flex-shrink: 0;
  width: 1.1em;
  text-align: center;
  opacity: 0.58;
}
.tl-icon.status {
  font-size: 12px;
}
.tl-message {
  flex: 1;
  min-width: 0;
  color: var(--fg);
  font-weight: 450;
}
.tl-role {
  font-weight: 600;
  font-size: 12px;
  color: var(--muted-fg);
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
  border: 0;
  background: transparent;
  color: var(--subtle);
  border-radius: 999px;
  padding: 2px 8px;
  font-size: 11px;
  cursor: pointer;
}
.tl-toggle:hover {
  color: var(--muted-fg);
  background: var(--muted-bg);
}
.tl-user {
  padding: 10px 12px;
  background: var(--muted-bg);
  border-radius: 12px;
  font-size: 13px;
}
.tl-user-meta {
  display: flex;
  justify-content: space-between;
  margin-bottom: 4px;
}
.tl-user-text {
  margin: 0;
  color: var(--fg);
  white-space: pre-wrap;
  word-break: break-word;
}
.tl-user-text.collapsed {
  color: var(--subtle);
}
.tl-reply,
.tl-thought,
.tl-tool {
  margin-top: 6px;
  font-size: 13px;
  line-height: 1.55;
  color: var(--fg);
}
.tl-thought.preview {
  max-height: 4.8em;
  overflow: hidden;
  opacity: 0.9;
  font-size: 12px;
  color: var(--subtle);
  overscroll-behavior: contain;
}
.tl-thought.full {
  padding: 8px 10px;
  border-radius: 10px;
  background: color-mix(in srgb, var(--card) 72%, transparent);
  max-height: 320px;
  overflow: auto;
  overscroll-behavior: contain;
}
.tl-thought-summary {
  display: none; /* title already shows summary in auto/collapsed */
}
.tl-reply.stream {
  white-space: pre-wrap;
}
.tl-paths {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin: 0 0 6px;
}
.path-chip {
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 11px;
  padding: 1px 5px;
  border-radius: 4px;
  background: color-mix(in srgb, var(--muted-bg) 70%, transparent);
  color: var(--muted-fg);
}
.tl-tool pre {
  margin: 4px 0 0;
  padding: 8px 10px;
  border-radius: 10px;
  background: color-mix(in srgb, var(--card) 72%, transparent);
  font-size: 12px;
  overflow: auto;
  max-height: 200px;
  overscroll-behavior: contain;
  white-space: pre-wrap;
  word-break: break-word;
}
.tl-empty-detail {
  margin: 0;
  font-size: 12px;
  color: var(--subtle);
}
.tl-permission,
.tl-notice {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
}
.tl-plan-steps {
  margin: 8px 0 0;
  padding-left: 1.2em;
  font-size: 12px;
  color: var(--fg);
}
.tl-plan-steps li[data-status="completed"] {
  color: var(--subtle);
  text-decoration: line-through;
}
:deep(.tl-reply.markdown pre),
:deep(.tl-thought pre) {
  background: var(--muted-bg);
  padding: 8px;
  border-radius: 6px;
  overflow: auto;
}
:deep(.tl-reply.markdown code),
:deep(.tl-thought code) {
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 12px;
}
:deep(.tl-reply.markdown table),
:deep(.tl-thought table) {
  border-collapse: collapse;
  width: 100%;
  font-size: 12px;
  margin: 8px 0;
}
:deep(.tl-reply.markdown th),
:deep(.tl-reply.markdown td),
:deep(.tl-thought th),
:deep(.tl-thought td) {
  border: 1px solid var(--border);
  padding: 4px 8px;
  text-align: left;
}
:deep(.tl-reply.markdown blockquote),
:deep(.tl-thought blockquote) {
  margin: 8px 0;
  padding-left: 12px;
  border-left: 3px solid var(--border);
  color: var(--subtle);
}
:deep(ul.task-list) {
  list-style: none;
  padding-left: 0.25em;
}
:deep(.task-list-item input) {
  margin-right: 6px;
}
</style>
