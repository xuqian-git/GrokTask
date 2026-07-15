import { mount } from "@vue/test-utils";
import { afterEach, describe, expect, it, vi } from "vitest";
import ActivePlanBar from "../src/components/plan/ActivePlanBar.vue";
import TaskView from "../src/views/TaskView.vue";
import PopoverView from "../src/views/PopoverView.vue";
import * as ipc from "../src/lib/ipc";
import { mockTaskDetail } from "../src/lib/mockData";
import type { TaskListItem } from "../src/lib/types";
import { resetUiStateForTests } from "../src/lib/uiState";

describe("conversation shell layouts", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("TaskView has history sidebar, timeline, plan bar, composer", async () => {
    resetUiStateForTests();
    const w = mount(TaskView, {
      attachTo: document.body,
    });
    // wait for onMounted fetch
    await new Promise((r) => setTimeout(r, 0));
    await w.vm.$nextTick();

    expect(w.find('[data-testid="task-shell"]').exists()).toBe(true);
    expect(w.find('[data-testid="history-sidebar"]').exists()).toBe(true);
    expect(w.find('[data-testid="task-header"]').exists()).toBe(true);
    expect(w.find('[data-testid="timeline-scroll"]').exists()).toBe(true);
    expect(w.find('[data-testid="active-plan-bar"]').exists()).toBe(true);
    expect(w.find('[data-testid="composer"]').exists()).toBe(true);

    // DOM order fixture content present
    const text = w.text();
    expect(text).toContain("Explain the ACP reducer");
    expect(text).not.toContain("session/update");
    expect(text).not.toContain("tool_call_update");

    w.unmount();
  });

  it("TaskView surfaces list load errors and clears loading", async () => {
    resetUiStateForTests();
    vi.spyOn(ipc, "fetchTaskList").mockRejectedValue(
      new Error("tasks_list not allowed. Command not found"),
    );

    const w = mount(TaskView, {
      attachTo: document.body,
    });
    await new Promise((r) => setTimeout(r, 30));
    await w.vm.$nextTick();

    expect(w.text()).not.toContain("加载任务…");
    const err = w.find('[data-testid="task-error"]');
    expect(err.exists()).toBe(true);
    expect(err.text()).toContain("tasks_list not allowed");
    expect(w.find('[data-testid="task-header"]').exists()).toBe(false);

    w.unmount();
  });

  it("TaskView selects first list item and loads its detail", async () => {
    resetUiStateForTests();
    const list: TaskListItem[] = [
      {
        taskId: "task-real-1",
        title: "Real task one",
        cwd: "/tmp/a",
        mode: "read",
        status: "idle",
        actualModel: "grok-4",
        createdAt: "2026-07-15T00:00:00.000Z",
        updatedAt: "2026-07-15T00:01:00.000Z",
      },
      {
        taskId: "task-real-2",
        title: "Real task two",
        cwd: "/tmp/b",
        mode: "write",
        status: "running",
        actualModel: "grok-4",
        createdAt: "2026-07-15T00:02:00.000Z",
        updatedAt: "2026-07-15T00:03:00.000Z",
      },
    ];
    const listSpy = vi.spyOn(ipc, "fetchTaskList").mockResolvedValue(list);
    const detailSpy = vi
      .spyOn(ipc, "fetchTaskDetail")
      .mockImplementation(async (taskId?: string) => {
        const d = mockTaskDetail();
        const id = taskId ?? "task-real-1";
        d.task.taskId = id;
        d.title = id === "task-real-1" ? "Real task one" : "Real task two";
        return d;
      });

    const w = mount(TaskView, {
      attachTo: document.body,
    });
    await new Promise((r) => setTimeout(r, 30));
    await w.vm.$nextTick();

    expect(listSpy).toHaveBeenCalledTimes(1);
    expect(detailSpy).toHaveBeenCalledWith("task-real-1");
    expect(w.find('[data-testid="task-title"]').text()).toContain(
      "Real task one",
    );
    expect(w.text()).not.toContain("加载任务…");
    expect(w.find('[data-testid="task-error"]').exists()).toBe(false);

    w.unmount();
  });

  it("TaskView composer sends a follow-up turn for the selected task", async () => {
    resetUiStateForTests();
    const list: TaskListItem[] = [
      {
        taskId: "task-real-1",
        title: "Real task one",
        cwd: "/tmp/a",
        mode: "read",
        status: "idle",
        actualModel: "grok-4",
        createdAt: "2026-07-15T00:00:00.000Z",
        updatedAt: "2026-07-15T00:01:00.000Z",
      },
    ];
    vi.spyOn(ipc, "fetchTaskList").mockResolvedValue(list);
    vi.spyOn(ipc, "fetchTaskDetail").mockImplementation(async (taskId?: string) => {
      const d = mockTaskDetail();
      d.task.taskId = taskId ?? "task-real-1";
      d.title = "Real task one";
      d.task.status = "idle";
      return d;
    });
    const sendSpy = vi.spyOn(ipc, "sendTaskMessage").mockResolvedValue({
      submissionId: "sub-1",
      taskId: "task-real-1",
      turnId: "turn-2",
      turnOrdinal: 2,
      status: "queued",
      mode: "read",
      createdAt: "2026-07-15T00:02:00.000Z",
    });

    const w = mount(TaskView, {
      attachTo: document.body,
    });
    await new Promise((r) => setTimeout(r, 30));
    await w.vm.$nextTick();

    await w.find('[data-testid="composer-input"]').setValue("继续解释一下");
    await w.find('[data-testid="composer-send"]').trigger("click");
    await new Promise((r) => setTimeout(r, 30));
    await w.vm.$nextTick();

    expect(sendSpy).toHaveBeenCalledWith("task-real-1", "继续解释一下");

    w.unmount();
  });

  it("PopoverView uses compact timeline + plan + composer without sidebar", async () => {
    resetUiStateForTests();
    const w = mount(PopoverView, {
      attachTo: document.body,
    });
    await new Promise((r) => setTimeout(r, 0));
    await w.vm.$nextTick();

    expect(w.find('[data-testid="popover-shell"]').exists()).toBe(true);
    expect(w.find('[data-testid="history-sidebar"]').exists()).toBe(false);
    expect(w.find('[data-testid="timeline-scroll"]').exists()).toBe(true);
    expect(w.find('[data-testid="composer"]').exists()).toBe(true);

    w.unmount();
  });

  it("PopoverView loads task detail once on mount (no watcher double-fetch)", async () => {
    resetUiStateForTests();
    // Use non-demo-2 ids so both auto-select and user switch hit fetchTaskDetail.
    const list: TaskListItem[] = [
      {
        taskId: "task-demo-1",
        title: "Idle task",
        cwd: "/tmp/a",
        mode: "read",
        status: "idle",
        actualModel: "fixture",
        createdAt: "2026-07-14T00:00:00.000Z",
        updatedAt: "2026-07-14T00:01:00.000Z",
      },
      {
        taskId: "task-live-run",
        title: "Live running task",
        cwd: "/tmp/b",
        mode: "write",
        status: "running",
        actualModel: "grok-4",
        createdAt: "2026-07-15T08:00:00.000Z",
        updatedAt: "2026-07-15T08:12:00.000Z",
      },
    ];

    const listSpy = vi.spyOn(ipc, "fetchTaskList").mockResolvedValue(list);
    const detailSpy = vi
      .spyOn(ipc, "fetchTaskDetail")
      .mockImplementation(async (taskId?: string) => {
        const d = mockTaskDetail();
        const id = taskId ?? "task-demo-1";
        d.task.taskId = id;
        d.title = id === "task-live-run" ? "Live running task" : "Idle task";
        d.task.status = id === "task-live-run" ? "running" : "idle";
        return d;
      });

    const w = mount(PopoverView, {
      attachTo: document.body,
    });
    await new Promise((r) => setTimeout(r, 30));
    await w.vm.$nextTick();

    // Mount auto-selects the running task → exactly one detail load via watcher.
    expect(listSpy).toHaveBeenCalledTimes(1);
    expect(detailSpy).toHaveBeenCalledTimes(1);
    expect(detailSpy).toHaveBeenCalledWith("task-live-run");
    expect(w.find('[data-testid="popover-title"]').text()).toContain(
      "Live running task",
    );

    // Manual switch still loads once per selection change.
    const select = w.find('[data-testid="popover-task-switch"]');
    expect(select.exists()).toBe(true);
    await select.setValue("task-demo-1");
    await new Promise((r) => setTimeout(r, 30));
    await w.vm.$nextTick();
    expect(detailSpy).toHaveBeenCalledTimes(2);
    expect(detailSpy).toHaveBeenLastCalledWith("task-demo-1");

    w.unmount();
  });

  it("ActivePlanBar shows all steps and completed/total", () => {
    const w = mount(ActivePlanBar, {
      props: {
        plan: {
          itemId: "plan:1",
          currentStep: "Step B",
          entries: [
            { content: "Step A", status: "completed" },
            { content: "Step B", status: "running" },
            { content: "Step C", status: "pending" },
          ],
        },
      },
    });
    expect(w.find('[data-testid="plan-count"]').text()).toBe("1/3");
    expect(w.find('[data-testid="plan-current"]').text()).toContain("Step B");
    const steps = w.findAll('[data-testid="plan-steps"] li');
    expect(steps).toHaveLength(3);
    expect(steps[0].attributes("data-status")).toBe("completed");
    expect(steps[1].attributes("data-status")).toBe("running");
  });
});
