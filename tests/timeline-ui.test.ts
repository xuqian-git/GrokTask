import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";
import TimelineItemCard from "../src/components/timeline/TimelineItemCard.vue";
import TimelineView from "../src/components/timeline/TimelineView.vue";
import { mockTaskDetail } from "../src/lib/mockData";
import type { TimelineEvent } from "../src/lib/types";

describe("timeline UI cards", () => {
  it("renders human message not raw JSON for tools", () => {
    const ev: TimelineEvent = {
      itemId: "tool:1",
      kind: "tool_call",
      message: "Read src/server.ts",
      text: "ok",
      streaming: false,
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
    // streaming path is plain text, not full markdown strong tags as primary structure
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
    expect(text).toContain("Read src/reducer.rs");
    expect(text).toContain("Explain the ACP reducer");
  });
});
