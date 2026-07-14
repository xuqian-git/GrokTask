import { describe, expect, it } from "vitest";
import { disclosureKey } from "../src/lib/expansion";
import { mockLightweightTools, mockThoughtToolThoughtReply } from "../src/lib/mockData";
import {
  AGGREGATE_MAX_MEMBERS,
  aggregateDisclosureItemId,
  projectTimeline,
  projectedKindOrder,
} from "../src/lib/timelineProjection";
import type { TimelineEvent } from "../src/lib/types";

function makeRead(id: string, seq: number): TimelineEvent {
  return {
    itemId: id,
    kind: "tool_call",
    message: `Read ${id}`,
    text: "ok",
    status: "completed",
    toolKind: "read",
    locations: [`${id}.ts`],
    streaming: false,
    firstSequence: seq,
    lastSequence: seq,
  };
}

describe("timeline projection", () => {
  it("preserves Thought → Tool → Thought → Reply order", () => {
    const rows = projectTimeline(mockThoughtToolThoughtReply(), {});
    expect(projectedKindOrder(rows)).toEqual([
      "user_message",
      "reasoning_segment",
      "tool_call",
      "reasoning_segment",
      "assistant_segment",
    ]);
  });

  it("aggregates adjacent completed lightweight tools", () => {
    const tools = mockLightweightTools();
    const rows = projectTimeline(tools, {});
    expect(rows).toHaveLength(1);
    expect(rows[0].rowKind).toBe("aggregate");
    expect(rows[0].aggregate?.memberItemIds).toEqual([
      "tool:sess:r1",
      "tool:sess:r2",
      "tool:sess:s1",
    ]);
    expect(rows[0].key).toBe(aggregateDisclosureItemId("tool:sess:r1"));
  });

  it("never aggregates when a member is user-expanded", () => {
    const tools = mockLightweightTools();
    const exp = {
      [disclosureKey(tools[0].itemId)]: "user-expanded" as const,
    };
    const rows = projectTimeline(tools, exp);
    // first stands alone expanded; rest may aggregate
    expect(rows.some((r) => r.event?.itemId === tools[0].itemId)).toBe(true);
    expect(
      rows.find((r) => r.event?.itemId === tools[0].itemId)?.rowKind,
    ).toBe("event");
  });

  it("expands aggregate into flat member rows when aggregate is user-expanded", () => {
    const tools = mockLightweightTools();
    const first = tools[0].itemId;
    const exp = {
      [disclosureKey(aggregateDisclosureItemId(first))]:
        "user-expanded" as const,
    };
    const rows = projectTimeline(tools, exp);
    expect(rows[0].rowKind).toBe("aggregate");
    expect(rows.filter((r) => r.rowKind === "aggregate_member")).toHaveLength(
      3,
    );
  });

  it("caps aggregate groups at AGGREGATE_MAX_MEMBERS", () => {
    const many = Array.from({ length: AGGREGATE_MAX_MEMBERS + 5 }, (_, i) =>
      makeRead(`tool:r${i}`, i + 1),
    );
    const rows = projectTimeline(many, {});
    const aggregates = rows.filter((r) => r.rowKind === "aggregate");
    expect(aggregates.length).toBeGreaterThanOrEqual(2);
    expect(aggregates[0].aggregate!.members.length).toBe(AGGREGATE_MAX_MEMBERS);
  });

  it("does not aggregate non-completed tools", () => {
    const events: TimelineEvent[] = [
      makeRead("tool:a", 1),
      {
        ...makeRead("tool:b", 2),
        status: "running",
      },
      makeRead("tool:c", 3),
    ];
    const rows = projectTimeline(events, {});
    expect(projectedKindOrder(rows)).toEqual([
      "tool_call",
      "tool_call",
      "tool_call",
    ]);
  });
});
