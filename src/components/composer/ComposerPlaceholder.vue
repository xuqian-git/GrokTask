<script setup lang="ts">
import { computed, ref } from "vue";
import type { TaskContainerStatus } from "@/lib/types";

const props = withDefaults(
  defineProps<{
    status?: TaskContainerStatus;
    compact?: boolean;
    disabled?: boolean;
  }>(),
  { status: "idle", compact: false, disabled: false },
);

const emit = defineEmits<{
  send: [text: string];
  cancel: [];
  "open-full": [];
}>();

const draft = ref("");

const canSend = computed(() => {
  if (props.disabled) return false;
  if (!draft.value.trim()) return false;
  // Phase 4: only idle enables send; other states show status
  return props.status === "idle";
});

const statusHint = computed(() => {
  switch (props.status) {
    case "queued":
      return "排队中…";
    case "starting":
      return "正在启动 Grok…";
    case "running":
      return "Grok 运行中…";
    case "cancelling":
      return "正在取消…";
    case "recovering":
      return "正在恢复会话…";
    case "interrupted":
      return "会话已中断 — 需显式恢复";
    case "failed":
      return "任务失败";
    case "cancelled":
      return "任务已取消";
    default:
      return "";
  }
});

function onKeydown(e: KeyboardEvent) {
  if (e.key === "Enter" && !e.shiftKey) {
    e.preventDefault();
    submit();
  }
}

function submit() {
  if (!canSend.value) return;
  const text = draft.value.trim();
  emit("send", text);
  draft.value = "";
}
</script>

<template>
  <footer class="composer" :class="{ compact }" data-testid="composer">
    <p v-if="statusHint" class="status-hint" data-testid="composer-status">
      {{ statusHint }}
    </p>
    <div class="row">
      <textarea
        v-model="draft"
        class="input"
        rows="2"
        :placeholder="
          canSend || status === 'idle'
            ? '继续让 Grok…'
            : '等待当前任务结束后再发送'
        "
        :disabled="!canSend && status !== 'idle'"
        data-testid="composer-input"
        @keydown="onKeydown"
      />
      <div class="actions">
        <button
          v-if="compact"
          type="button"
          class="btn ghost"
          data-testid="open-full"
          @click="emit('open-full')"
        >
          完整窗口
        </button>
        <button
          v-if="
            status === 'running' ||
            status === 'starting' ||
            status === 'cancelling'
          "
          type="button"
          class="btn ghost"
          data-testid="composer-cancel"
          @click="emit('cancel')"
        >
          取消
        </button>
        <button
          type="button"
          class="btn primary"
          :disabled="!canSend"
          data-testid="composer-send"
          @click="submit"
        >
          发送
        </button>
      </div>
    </div>
  </footer>
</template>

<style scoped>
.composer {
  flex-shrink: 0;
  border-top: 1px solid var(--border);
  background: var(--card);
  padding: 10px 12px;
  min-height: 96px;
}
.composer.compact {
  min-height: 80px;
  padding: 8px 10px;
}
.status-hint {
  margin: 0 0 6px;
  font-size: 11px;
  color: var(--subtle);
}
.row {
  display: flex;
  gap: 8px;
  align-items: flex-end;
}
.input {
  flex: 1;
  resize: none;
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 8px 10px;
  background: var(--bg);
  color: var(--fg);
  font-size: 13px;
  line-height: 1.4;
  min-height: 52px;
}
.input:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}
.actions {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.btn {
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 6px 12px;
  font-size: 12px;
  cursor: pointer;
  background: var(--bg);
  color: var(--fg);
  white-space: nowrap;
}
.btn:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}
.btn.primary {
  background: var(--muted-bg);
  color: var(--muted-fg);
  border-color: transparent;
  font-weight: 600;
}
.btn.ghost {
  background: transparent;
}
</style>
