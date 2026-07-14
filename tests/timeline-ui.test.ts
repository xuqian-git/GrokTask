import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";
import TimelineItemCard from "../src/components/timeline/TimelineItemCard.vue";
import TimelineView from "../src/components/timeline/TimelineView.vue";
import {
  mockLightweightTools,
  mockTaskDetail,
  mockThoughtToolThoughtReply,
} from "../src/lib/mockData";
import { disclosureKey } from "../src/lib/expansion";
import { projectTimeline, projectedKindOrder } from "../src/lib/timelineProjection";
import type { TimelineEvent } from "../src/lib/types";
import {
  getSharedExpansion,
  replaceSharedExpansionKey,
  resetUiStateForTests,
} from "../src/lib/uiState";

describe("timeline UI cards", () => {
  it("renders human message not raw JSON for tools", () => {
    const ev: TimelineEvent = {
      itemId: "tool:1",
      kind: "tool_call",
      message: "Read src/server.ts",
      text: "ok",
      streaming: false,
      status: "completed",
      toolKind: "read",
      locations: ["src/server.ts"],
      firstSequence: 1,
      lastSequence: 1,
    };
    const w = mount(TimelineItemCard, {
      props: { event: ev, expansion: {} },
    });
    expect(w.text()).toContain("Read src/server.ts");
    expect(w.text()).not.toContain("session/update");
    expect(w.text()).not.toContain("tool_call_update");
    expect(w.text()).not.toContain('"jsonrpc"');
  });

  it("replaces non-JSON raw ACP labels with semantic fallbacks in card DOM", () => {
    const notice: TimelineEvent = {
      itemId: "notice:1",
      kind: "context_notice",
      message: "ACP 通知：_x.ai/session_notification",
      text: "session/update",
      streaming: false,
      locations: [],
      firstSequence: 1,
      lastSequence: 1,
    };
    const noticeW = mount(TimelineItemCard, {
      props: { event: notice, expansion: {} },
    });
    expect(noticeW.text()).toContain("状态提示");
    expect(noticeW.text()).not.toContain("_x.ai");
    expect(noticeW.text()).not.toContain("session/update");
    expect(noticeW.text()).not.toContain("session_notification");

    const perm: TimelineEvent = {
      itemId: "perm:raw",
      kind: "permission_request",
      message: "tool_call_update",
      title: "session/update",
      text: "",
      streaming: false,
      status: "pending",
      locations: [],
      firstSequence: 2,
      lastSequence: 2,
    };
    const permW = mount(TimelineItemCard, {
      props: { event: perm, expansion: {} },
    });
    expect(permW.text()).toContain("权限请求");
    expect(permW.text()).not.toContain("tool_call_update");
    expect(permW.text()).not.toContain("session/update");

    const tool: TimelineEvent = {
      itemId: "tool:raw-label",
      kind: "tool_call",
      message: "session/update",
      title: "tool_call_update",
      text: "",
      streaming: false,
      status: "completed",
      toolKind: "read",
      locations: ["src/ok.ts"],
      firstSequence: 3,
      lastSequence: 3,
    };
    const toolW = mount(TimelineItemCard, {
      props: { event: tool, expansion: {} },
    });
    expect(toolW.text()).toContain("src/ok.ts");
    expect(toolW.text()).not.toContain("session/update");
    expect(toolW.text()).not.toContain("tool_call_update");
  });

  it("TimelineView DOM hides non-JSON ACP protocol labels", () => {
    const events: TimelineEvent[] = [
      {
        itemId: "notice:acp",
        kind: "context_notice",
        message: "ACP 通知：_x.ai/session_notification",
        text: "",
        streaming: false,
        locations: [],
        firstSequence: 1,
        lastSequence: 1,
      },
      {
        itemId: "perm:acp",
        kind: "permission_request",
        message: "session/update",
        text: "",
        streaming: false,
        status: "pending",
        locations: [],
        firstSequence: 2,
        lastSequence: 2,
      },
      {
        itemId: "tool:acp",
        kind: "tool_call",
        message: "tool_call_update",
        text: "",
        streaming: false,
        status: "completed",
        toolKind: "read",
        locations: ["src/a.ts"],
        firstSequence: 3,
        lastSequence: 3,
      },
    ];
    const w = mount(TimelineView, {
      props: { events, expansion: {} },
    });
    const text = w.text();
    expect(text).not.toContain("session/update");
    expect(text).not.toContain("tool_call_update");
    expect(text).not.toContain("_x.ai");
    expect(text).toContain("状态提示");
    expect(text).toContain("权限请求");
    expect(text).toContain("src/a.ts");
  });

  it("renders streaming reply as text fragments", () => {
    const ev: TimelineEvent = {
      itemId: "a1",
      kind: "assistant_segment",
      message: "Hello",
      text: "Hello **stream**",
      streaming: true,
      locations: [],
      firstSequence: 1,
      lastSequence: 1,
    };
    const w = mount(TimelineItemCard, {
      props: { event: ev, expansion: {} },
    });
    const stream = w.find('[data-testid="reply-stream"]');
    expect(stream.exists()).toBe(true);
    expect(stream.text()).toContain("Hello");
    expect(w.find('[data-testid="reply-final"]').exists()).toBe(false);
  });

  it("renders final assistant as markdown", () => {
    const ev: TimelineEvent = {
      itemId: "a2",
      kind: "assistant_segment",
      message: "Done",
      text: "Final **answer**",
      streaming: false,
      answerMark: "finalAnswer",
      locations: [],
      firstSequence: 1,
      lastSequence: 1,
    };
    const w = mount(TimelineItemCard, {
      props: { event: ev, expansion: {} },
    });
    const final = w.find('[data-testid="reply-final"]');
    expect(final.exists()).toBe(true);
    expect(final.html()).toContain("<strong>answer</strong>");
  });

  it("preserves user-expanded thought after re-render", async () => {
    const detail = mockTaskDetail();
    const thought = detail.timeline.find((e) => e.kind === "reasoning_segment")!;
    const w = mount(TimelineItemCard, {
      props: {
        event: thought,
        expansion: { [`item:${thought.itemId}:details`]: "user-expanded" },
      },
    });
    expect(w.find('[data-testid="thought-body"]').exists()).toBe(true);
    await w.setProps({
      event: { ...thought, streaming: false, text: thought.text + " more" },
      expansion: { [`item:${thought.itemId}:details`]: "user-expanded" },
    });
    expect(w.find('[data-testid="thought-body"]').exists()).toBe(true);
    expect(w.attributes("data-expansion")).toBe("user-expanded");
  });

  it("timeline view shows no raw ACP method names in normal cards", () => {
    const detail = mockTaskDetail();
    const w = mount(TimelineView, {
      props: { events: detail.timeline, expansion: {} },
    });
    const text = w.text();
    expect(text).not.toContain("session/update");
    expect(text).not.toContain("tool_call_update");
    expect(text).not.toContain("agent_thought_chunk");
    expect(text).not.toContain("_x.ai");
    expect(text).toContain("Read src/reducer.rs");
    expect(text).toContain("Explain the ACP reducer");
  });

  it("DOM order is Thought → Tool → Thought → Reply", () => {
    const events = mockThoughtToolThoughtReply();
    const w = mount(TimelineView, {
      props: { events, expansion: {} },
    });
    const items = w.findAll("[data-kind]");
    const kinds = items.map((n) => n.attributes("data-kind"));
    // user, thought, tool, thought, assistant
    expect(kinds).toEqual([
      "user_message",
      "reasoning_segment",
      "tool_call",
      "reasoning_segment",
      "assistant_segment",
    ]);
  });

  it("tool update remains one card (single toolCallId item)", () => {
    const tool: TimelineEvent = {
      itemId: "tool:sess:fx1",
      kind: "tool_call",
      message: "Read src/reducer.rs",
      text: "partial",
      status: "running",
      toolKind: "read",
      locations: ["src/reducer.rs"],
      streaming: false,
      firstSequence: 4,
      lastSequence: 4,
    };
    const updated: TimelineEvent = {
      ...tool,
      text: "complete body",
      status: "completed",
      lastSequence: 5,
    };
    const w = mount(TimelineView, {
      props: { events: [tool], expansion: {} },
    });
    expect(w.findAll('[data-kind="tool_call"]')).toHaveLength(1);
    // parent re-renders with updated same itemId
    return w
      .setProps({ events: [updated], expansion: {} })
      .then(() => {
        expect(w.findAll('[data-kind="tool_call"]')).toHaveLength(1);
        expect(w.find('[data-item-id="tool:sess:fx1"]').exists()).toBe(true);
      });
  });

  it("reasoning preview / expanded / collapsed modes", async () => {
    const thought: TimelineEvent = {
      itemId: "seg:think:1",
      kind: "reasoning_segment",
      stageTitle: "Checking order",
      message: "Checking order",
      text: "Line one of thought.\n\nLine two of thought.\n\nLine three of thought.\n\nLine four hidden from preview.",
      streaming: true,
      locations: [],
      firstSequence: 1,
      lastSequence: 1,
    };

    // auto + streaming → preview
    const streaming = mount(TimelineItemCard, {
      props: { event: thought, expansion: {} },
    });
    expect(streaming.find('[data-testid="thought-preview"]').exists()).toBe(
      true,
    );
    expect(streaming.find('[data-testid="thought-title"]').text()).toContain(
      "正在思考",
    );

    // auto + completed → summary title, no full body
    const completed = mount(TimelineItemCard, {
      props: {
        event: { ...thought, streaming: false },
        expansion: {},
      },
    });
    expect(completed.find('[data-testid="thought-body"]').exists()).toBe(false);
    expect(completed.find('[data-testid="thought-preview"]').exists()).toBe(
      false,
    );
    expect(completed.find('[data-testid="thought-title"]').text()).toContain(
      "Checking order",
    );

    // user-expanded → full body
    const expanded = mount(TimelineItemCard, {
      props: {
        event: { ...thought, streaming: false },
        expansion: { [disclosureKey(thought.itemId)]: "user-expanded" },
      },
    });
    expect(expanded.find('[data-testid="thought-body"]').exists()).toBe(true);
    expect(expanded.find('[data-testid="thought-body"]').text()).toContain(
      "Line four",
    );

    // user-collapsed → no preview even if streaming
    const collapsed = mount(TimelineItemCard, {
      props: {
        event: thought,
        expansion: { [disclosureKey(thought.itemId)]: "user-collapsed" },
      },
    });
    expect(collapsed.find('[data-testid="thought-preview"]').exists()).toBe(
      false,
    );
    expect(collapsed.find('[data-testid="thought-body"]').exists()).toBe(false);
  });

  it("lightweight aggregation does not hide a user-expanded member", () => {
    const tools = mockLightweightTools();
    // Without expansion: one aggregate header
    const collapsed = projectTimeline(tools, {});
    expect(projectedKindOrder(collapsed)).toEqual(["aggregate"]);
    expect(collapsed).toHaveLength(1);

    // Expand middle member → that member stands alone, neighbors may regroup
    const exp = {
      [disclosureKey(tools[1].itemId)]: "user-expanded" as const,
    };
    const rows = projectTimeline(tools, exp);
    const kinds = projectedKindOrder(rows);
    expect(kinds).toContain("tool_call");
    // Expanded member must appear as its own event row with full itemId
    const memberRow = rows.find((r) => r.event?.itemId === tools[1].itemId);
    expect(memberRow).toBeTruthy();
    expect(memberRow!.rowKind).toBe("event");

    const w = mount(TimelineView, {
      props: { events: tools, expansion: exp },
    });
    expect(w.find(`[data-item-id="${tools[1].itemId}"]`).exists()).toBe(true);
    expect(w.find(`[data-item-id="${tools[1].itemId}"]`).text()).toContain(
      "Read b.ts",
    );
  });

  it("does not aggregate edit / terminal / permission rows", () => {
    const events: TimelineEvent[] = [
      {
        itemId: "tool:r1",
        kind: "tool_call",
        message: "Read a",
        text: "",
        status: "completed",
        toolKind: "read",
        locations: ["a"],
        streaming: false,
        firstSequence: 1,
        lastSequence: 1,
      },
      {
        itemId: "tool:edit1",
        kind: "tool_call",
        message: "Edit a",
        text: "",
        status: "completed",
        toolKind: "edit",
        locations: ["a"],
        streaming: false,
        firstSequence: 2,
        lastSequence: 2,
      },
      {
        itemId: "tool:r2",
        kind: "tool_call",
        message: "Read b",
        text: "",
        status: "completed",
        toolKind: "read",
        locations: ["b"],
        streaming: false,
        firstSequence: 3,
        lastSequence: 3,
      },
      {
        itemId: "tool:term1",
        kind: "tool_call",
        message: "Run tests",
        text: "",
        status: "completed",
        toolKind: "terminal",
        locations: [],
        streaming: false,
        firstSequence: 4,
        lastSequence: 4,
      },
      {
        itemId: "perm:1",
        kind: "permission_request",
        message: "Grok 请求运行命令权限 · 已按 READ 模式拒绝",
        text: "",
        status: "rejected",
        locations: [],
        streaming: false,
        firstSequence: 5,
        lastSequence: 5,
      },
    ];
    const rows = projectTimeline(events, {});
    // No multi-member aggregate (reads not adjacent without non-light between)
    expect(projectedKindOrder(rows)).toEqual([
      "tool_call",
      "tool_call",
      "tool_call",
      "tool_call",
      "permission_request",
    ]);
  });

  it("popover and full window share the same disclosure map", () => {
    resetUiStateForTests();
    const taskId = "task-shared-1";
    const itemId = "seg:t1:1:thought";
    const key = disclosureKey(itemId);

    replaceSharedExpansionKey(taskId, { [key]: "user-expanded" });
    const fromFull = getSharedExpansion(taskId);
    expect(fromFull[key]).toBe("user-expanded");

    // Simulate popover reading same map
    const fromPopover = { ...getSharedExpansion(taskId) };
    expect(fromPopover[key]).toBe("user-expanded");

    // Popover toggles collapse into shared map
    replaceSharedExpansionKey(taskId, { [key]: "user-collapsed" });
    expect(getSharedExpansion(taskId)[key]).toBe("user-collapsed");
  });

  it("active plan bar is not in the timeline rows", () => {
    const detail = mockTaskDetail();
    const w = mount(TimelineView, {
      props: { events: detail.timeline, expansion: {} },
    });
    expect(w.find('[data-testid="active-plan-bar"]').exists()).toBe(false);
    // Plan content lives outside timeline projection
    const kinds = projectedKindOrder(projectTimeline(detail.timeline, {}));
    expect(kinds).not.toContain("plan");
  });
});
