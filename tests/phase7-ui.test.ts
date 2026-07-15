import { mount } from "@vue/test-utils";
import { afterEach, describe, expect, it, vi } from "vitest";
import App from "../src/App.vue";
import PopoverView from "../src/views/PopoverView.vue";
import * as ipc from "../src/lib/ipc";
import * as settings from "../src/lib/settings";
import { mockTaskDetail } from "../src/lib/mockData";
import type { TaskListItem } from "../src/lib/types";
import { resetUiStateForTests } from "../src/lib/uiState";

const sampleList: TaskListItem[] = [
  {
    taskId: "task-a",
    title: "实现登录页",
    cwd: "/tmp/proj",
    mode: "write",
    status: "idle",
    actualModel: "grok-4",
    latestAction: "已完成",
    createdAt: "2026-07-15T00:00:00.000Z",
    updatedAt: "2026-07-15T00:10:00.000Z",
    finishedAt: "2026-07-15T00:10:00.000Z",
  },
  {
    taskId: "task-b",
    title: "审查 PR",
    cwd: "/tmp/other",
    mode: "read",
    status: "failed",
    actualModel: "grok-4",
    latestAction: "测试失败",
    createdAt: "2026-07-14T00:00:00.000Z",
    updatedAt: "2026-07-14T00:05:00.000Z",
    finishedAt: "2026-07-14T00:05:00.000Z",
  },
];

describe("Phase 7 Chinese shell + History + Popover", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    settings.resetSettingsMocksForTests();
    resetUiStateForTests();
    window.history.replaceState({}, "", "?");
  });

  it("App shell defaults to Chinese nav labels without Phase 5 badge", async () => {
    window.history.replaceState({}, "", "?view=task");
    const w = mount(App, { attachTo: document.body });
    await w.vm.$nextTick();

    expect(w.find('[data-testid="app-nav"]').text()).toMatch(/任务记录/);
    expect(w.find('[data-testid="app-nav"]').text()).not.toMatch(/ACP 记录/);
    expect(w.find('[data-testid="app-nav"]').text()).toMatch(/设置/);
    expect(w.text()).not.toMatch(/本地工具/);
    expect(w.text()).not.toMatch(/Phase 5/);
    expect(w.find('[data-testid="app-header"]').exists()).toBe(true);

    w.unmount();
  });

  it("legacy history route opens the unified task records page", async () => {
    vi.spyOn(ipc, "fetchTaskList").mockResolvedValue(sampleList);
    vi.spyOn(ipc, "fetchTaskDetail").mockImplementation(async (id) => {
      const d = mockTaskDetail();
      d.task.taskId = id ?? "task-a";
      d.title = id === "task-b" ? "审查 PR" : "实现登录页";
      return d;
    });
    window.history.replaceState({}, "", "?view=history");

    const w = mount(App, { attachTo: document.body });
    await new Promise((r) => setTimeout(r, 30));
    await w.vm.$nextTick();

    expect(w.find('[data-testid="task-shell"]').exists()).toBe(true);
    expect(w.text()).toContain("实现登录页");
    expect(w.text()).not.toContain("session/update");
    w.unmount();
  });

  it("unified task records empty state when daemon has no tasks", async () => {
    vi.spyOn(ipc, "fetchTaskList").mockResolvedValue([]);
    window.history.replaceState({}, "", "?view=task");
    const w = mount(App, { attachTo: document.body });
    await new Promise((r) => setTimeout(r, 20));
    await w.vm.$nextTick();
    expect(w.find('[data-testid="task-empty"]').exists()).toBe(true);
    expect(w.text()).toMatch(/暂无任务/);
    w.unmount();
  });

  it("Popover 完整窗口 opens full layout path", async () => {
    resetUiStateForTests();
    vi.spyOn(ipc, "fetchTaskList").mockResolvedValue(sampleList);
    vi.spyOn(ipc, "fetchTaskDetail").mockImplementation(async (id) => {
      const d = mockTaskDetail();
      d.task.taskId = id ?? "task-a";
      d.title = "实现登录页";
      return d;
    });

    const navSpy = vi.fn();
    window.addEventListener("groktask-navigate", navSpy as EventListener);

    const w = mount(PopoverView, { attachTo: document.body });
    await new Promise((r) => setTimeout(r, 30));
    await w.vm.$nextTick();

    expect(w.find('[data-testid="popover-shell"]').exists()).toBe(true);
    const btn = w.find('[data-testid="popover-open-full"]');
    expect(btn.exists()).toBe(true);
    expect(btn.text()).toMatch(/完整窗口/);

    await btn.trigger("click");
    await new Promise((r) => setTimeout(r, 20));
    await w.vm.$nextTick();

    expect(navSpy).toHaveBeenCalled();
    const detail = (navSpy.mock.calls[0][0] as CustomEvent).detail;
    expect(detail.view).toBe("task");

    window.removeEventListener("groktask-navigate", navSpy as EventListener);
    w.unmount();
  });

  it("App opens task when history dispatches open-task", async () => {
    window.history.replaceState({}, "", "?view=task");
    vi.spyOn(ipc, "fetchTaskList").mockResolvedValue(sampleList);
    vi.spyOn(ipc, "fetchTaskDetail").mockImplementation(async (id) => {
      const d = mockTaskDetail();
      d.task.taskId = id ?? "task-a";
      d.title = id === "task-b" ? "审查 PR" : "实现登录页";
      return d;
    });

    const w = mount(App, { attachTo: document.body });
    await new Promise((r) => setTimeout(r, 30));
    await w.vm.$nextTick();

    expect(w.find('[data-testid="task-shell"]').exists()).toBe(true);

    window.dispatchEvent(
      new CustomEvent("groktask-open-task", {
        detail: { taskId: "task-b" },
      }),
    );
    await new Promise((r) => setTimeout(r, 40));
    await w.vm.$nextTick();

    expect(w.find('[data-testid="task-shell"]').exists()).toBe(true);
    expect(window.location.search).toContain("view=task");
    expect(window.location.search).toContain("task=task-b");

    w.unmount();
  });
});
