import { afterEach, describe, expect, it, vi } from "vitest";
import * as ipc from "../src/lib/ipc";
import type { TaskDetail, TaskListItem } from "../src/lib/types";

const realListItem: TaskListItem = {
  taskId: "2e79aa9c-09e7-409b-9048-f24890a763f9",
  title: "Reply with exactly: hello",
  cwd: "/tmp/demo",
  mode: "read",
  status: "idle",
  actualModel: "grok-4",
  latestAction: "Replying: hello",
  createdAt: "2026-07-15T00:00:00.000Z",
  updatedAt: "2026-07-15T00:01:00.000Z",
  finishedAt: "2026-07-15T00:01:00.000Z",
};

const realDetail: TaskDetail = {
  task: {
    taskId: realListItem.taskId,
    status: "idle",
    mode: "read",
    actualModel: "grok-4",
    latestAction: "Replying: hello",
    answerPreview: "hello",
    createdAt: realListItem.createdAt,
    updatedAt: realListItem.updatedAt,
    finishedAt: realListItem.finishedAt,
  },
  title: realListItem.title,
  cwd: realListItem.cwd,
  timeline: [
    {
      itemId: "seg:t1:1:agent",
      kind: "agent_message_chunk",
      message: "hello",
      text: "hello",
      streaming: false,
      locations: [],
      firstSequence: 1,
      lastSequence: 1,
    },
  ],
  lastSequence: 1,
  timelineGeneration: 1,
};

describe("ipc mock mode (non-Tauri)", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    delete (window as any).__TAURI_INTERNALS__;
  });

  it("is not Tauri in vitest / jsdom", () => {
    expect(ipc.isTauriRuntime()).toBe(false);
  });

  it("returns mock task list", async () => {
    const list = await ipc.fetchTaskList();
    expect(list.length).toBeGreaterThan(0);
    expect(list[0]?.taskId).toMatch(/^task-demo-/);
  });

  it("returns mock detail variants by id", async () => {
    const idle = await ipc.fetchTaskDetail("task-demo-1");
    expect(idle.task.taskId).toBe("task-demo-1");
    expect(idle.title).toContain("Demo task");

    const running = await ipc.fetchTaskDetail("task-demo-2");
    expect(running.task.taskId).toBe("task-demo-2");
    expect(running.task.status).toBe("running");

    const failed = await ipc.fetchTaskDetail("task-demo-3");
    expect(failed.task.taskId).toBe("task-demo-3");
    expect(failed.task.status).toBe("failed");
    expect(failed.title).toBe("Fix build");
  });
});

describe("ipc tauri invoke wiring", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.resetModules();
    vi.doUnmock("@tauri-apps/api/core");
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    delete (window as any).__TAURI_INTERNALS__;
  });

  it("calls tasks_list and tasks_show with expected args", async () => {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    (window as any).__TAURI_INTERNALS__ = { mock: true };

    const invoke = vi.fn(
      async (cmd: string, args?: Record<string, unknown>) => {
        if (cmd === "tasks_list") return [realListItem];
        if (cmd === "tasks_show") return realDetail;
        throw new Error(`unexpected ${cmd} ${JSON.stringify(args)}`);
      },
    );

    vi.doMock("@tauri-apps/api/core", () => ({
      invoke,
    }));

    const mod = await import("../src/lib/ipc");
    expect(mod.isTauriRuntime()).toBe(true);

    const list = await mod.fetchTaskList();
    expect(invoke).toHaveBeenCalledWith("tasks_list", { limit: null });
    expect(list).toEqual([realListItem]);
    expect(list[0]?.title).toBe("Reply with exactly: hello");

    const detail = await mod.fetchTaskDetail(realListItem.taskId);
    expect(invoke).toHaveBeenCalledWith("tasks_show", {
      taskId: realListItem.taskId,
    });
    expect(detail.timeline[0]?.text).toBe("hello");
  });

  it("fetchTaskDetail requires taskId in Tauri runtime", async () => {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    (window as any).__TAURI_INTERNALS__ = { mock: true };
    vi.doMock("@tauri-apps/api/core", () => ({
      invoke: vi.fn(),
    }));
    const mod = await import("../src/lib/ipc");
    await expect(mod.fetchTaskDetail()).rejects.toThrow(/taskId is required/);
  });
});
