import { describe, expect, it } from "vitest";
import {
  isLightweightToolKind,
  toolPrimaryLine,
  toolStatusIcon,
  toolVisualStatus,
} from "../src/lib/toolDisplay";
import type { TimelineEvent } from "../src/lib/types";

function tool(partial: Partial<TimelineEvent>): TimelineEvent {
  return {
    itemId: "t1",
    kind: "tool_call",
    message: "",
    text: "",
    streaming: false,
    locations: [],
    firstSequence: 1,
    lastSequence: 1,
    ...partial,
  };
}

describe("tool display", () => {
  it("classifies lightweight kinds", () => {
    expect(isLightweightToolKind("read")).toBe(true);
    expect(isLightweightToolKind("search")).toBe(true);
    expect(isLightweightToolKind("edit")).toBe(false);
    expect(isLightweightToolKind("terminal")).toBe(false);
  });

  it("uses present tense while running and past when completed", () => {
    const running = tool({
      status: "running",
      toolKind: "read",
      locations: ["src/server.ts"],
    });
    expect(toolPrimaryLine(running)).toMatch(/正在读取/);
    expect(toolPrimaryLine(running)).toContain("src/server.ts");

    const done = tool({
      status: "completed",
      toolKind: "read",
      locations: ["src/server.ts"],
    });
    expect(toolPrimaryLine(done)).toMatch(/读取了/);
  });

  it("marks failed explicitly", () => {
    const failed = tool({
      status: "failed",
      toolKind: "terminal",
      message: "pnpm test",
      locations: [],
    });
    expect(toolPrimaryLine(failed)).toMatch(/失败|✕/);
    expect(toolStatusIcon("failed")).toBe("✕");
  });

  it("prefers human message over raw json", () => {
    const human = tool({
      status: "completed",
      toolKind: "read",
      message: "Read src/reducer.rs",
      locations: ["src/reducer.rs"],
    });
    expect(toolPrimaryLine(human)).toContain("Read src/reducer.rs");

    const raw = tool({
      status: "completed",
      toolKind: "read",
      message: `{"jsonrpc":"2.0","method":"session/update"}`,
      locations: ["a.ts"],
    });
    expect(toolPrimaryLine(raw)).not.toContain("session/update");
    expect(toolPrimaryLine(raw)).toContain("a.ts");
  });

  it("does not surface non-JSON raw ACP labels in the primary line", () => {
    const labelOnly = tool({
      status: "completed",
      toolKind: "read",
      message: "session/update",
      title: "tool_call_update",
      locations: ["src/safe.ts"],
    });
    const line = toolPrimaryLine(labelOnly);
    expect(line).not.toContain("session/update");
    expect(line).not.toContain("tool_call_update");
    expect(line).toContain("src/safe.ts");

    const xaiLabel = tool({
      status: "running",
      toolKind: "read",
      message: "ACP 通知：_x.ai/session_notification",
      locations: ["b.ts"],
    });
    const xaiLine = toolPrimaryLine(xaiLabel);
    expect(xaiLine).not.toContain("_x.ai");
    expect(xaiLine).not.toContain("session_notification");
    expect(xaiLine).toContain("b.ts");
  });

  it("maps visual statuses", () => {
    expect(toolVisualStatus("in_progress")).toBe("running");
    expect(toolVisualStatus("success")).toBe("completed");
    expect(toolVisualStatus("error")).toBe("failed");
  });
});
