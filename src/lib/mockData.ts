import type { TaskDetail, TaskListItem, TimelineEvent } from "./types";

/** Classic Thought → Tool → Thought → Reply order fixture. */
export function mockThoughtToolThoughtReply(): TimelineEvent[] {
  return [
    {
      itemId: "seg:t1:0:user",
      kind: "user_message",
      turnId: "t1",
      message: "Explain the ACP reducer",
      text: "Explain the ACP reducer",
      streaming: false,
      locations: [],
      firstSequence: 1,
      lastSequence: 1,
    },
    {
      itemId: "seg:t1:1:thought",
      kind: "reasoning_segment",
      turnId: "t1",
      message: "Checking event order",
      stageTitle: "Checking event order",
      text: "I need to verify Thought → Tool → Reply order.\n\nSecond line of thinking.\n\nThird line of thinking.\n\nFourth line stays out of preview.",
      streaming: false,
      locations: [],
      firstSequence: 2,
      lastSequence: 3,
    },
    {
      itemId: "tool:sess:fx1",
      kind: "tool_call",
      turnId: "t1",
      message: "Read src/reducer.rs",
      title: "Read src/reducer.rs",
      text: "fn reduce() { /* ok */ }",
      status: "completed",
      toolKind: "read",
      locations: ["src/reducer.rs"],
      streaming: false,
      firstSequence: 4,
      lastSequence: 5,
    },
    {
      itemId: "seg:t1:2:thought",
      kind: "reasoning_segment",
      turnId: "t1",
      message: "Drafting answer",
      stageTitle: "Drafting answer",
      text: "Will summarize the reducer rules.",
      streaming: false,
      locations: [],
      firstSequence: 6,
      lastSequence: 6,
    },
    {
      itemId: "seg:t1:3:assistant",
      kind: "assistant_segment",
      turnId: "t1",
      message: "The reducer preserves arrival order",
      text: "The **reducer** preserves arrival order.\n\n- Thought stages stay in place\n- Tools merge by `toolCallId`\n- Final replies use Markdown\n\n```ts\nreduce(event)\n```\n\n| Rule | Behavior |\n| --- | --- |\n| Order | Arrival |\n| Merge | toolCallId |\n\n- [x] Ordered\n- [ ] Done\n",
      streaming: false,
      answerMark: "finalAnswer",
      locations: [],
      firstSequence: 7,
      lastSequence: 8,
    },
  ];
}

/** Adjacent completed lightweight tools for aggregation tests. */
export function mockLightweightTools(): TimelineEvent[] {
  return [
    {
      itemId: "tool:sess:r1",
      kind: "tool_call",
      message: "Read a.ts",
      title: "Read a.ts",
      text: "a",
      status: "completed",
      toolKind: "read",
      locations: ["a.ts"],
      streaming: false,
      firstSequence: 1,
      lastSequence: 1,
    },
    {
      itemId: "tool:sess:r2",
      kind: "tool_call",
      message: "Read b.ts",
      title: "Read b.ts",
      text: "b",
      status: "completed",
      toolKind: "read",
      locations: ["b.ts"],
      streaming: false,
      firstSequence: 2,
      lastSequence: 2,
    },
    {
      itemId: "tool:sess:s1",
      kind: "tool_call",
      message: "Search session/load",
      title: "Search session/load",
      text: "3 hits",
      status: "completed",
      toolKind: "search",
      locations: [],
      streaming: false,
      firstSequence: 3,
      lastSequence: 3,
    },
  ];
}

/** Fixture task detail for web test mode / offline UI. */
export function mockTaskDetail(): TaskDetail {
  return {
    task: {
      taskId: "task-demo-1",
      status: "idle",
      mode: "read",
      actualModel: "fixture",
      latestAction: "Replying: Hello from mock",
      createdAt: "2026-07-14T00:00:00.000Z",
      updatedAt: "2026-07-14T00:01:00.000Z",
      finishedAt: "2026-07-14T00:01:00.000Z",
    },
    title: "Demo task — ACP reducer",
    cwd: "/tmp/demo",
    timeline: mockThoughtToolThoughtReply(),
    activePlan: {
      itemId: "plan:t1:1",
      currentStep: "Draft reducer rules",
      entries: [
        {
          content: "Inspect event order",
          status: "completed",
          priority: "high",
        },
        { content: "Draft reducer rules", status: "running", priority: "high" },
        {
          content: "Write acceptance tests",
          status: "pending",
          priority: "medium",
        },
      ],
    },
    lastSequence: 8,
    timelineGeneration: 1,
  };
}

/** Richer multi-task history for sidebar. */
export function mockTaskList(): TaskListItem[] {
  return [
    {
      taskId: "task-demo-1",
      title: "Demo task — ACP reducer",
      cwd: "/tmp/demo",
      mode: "read",
      status: "idle",
      actualModel: "fixture",
      latestAction: "Replying: Hello from mock",
      createdAt: "2026-07-14T00:00:00.000Z",
      updatedAt: "2026-07-14T00:01:00.000Z",
      finishedAt: "2026-07-14T00:01:00.000Z",
    },
    {
      taskId: "task-demo-2",
      title: "Refactor ACP UI",
      cwd: "/tmp/fixture-workspace",
      mode: "write",
      status: "running",
      actualModel: "grok-4",
      latestAction: "Editing TimelineView.vue",
      createdAt: "2026-07-15T08:00:00.000Z",
      updatedAt: "2026-07-15T08:12:00.000Z",
    },
    {
      taskId: "task-demo-3",
      title: "Fix build",
      cwd: "/tmp/other",
      mode: "write",
      status: "failed",
      actualModel: "fixture",
      latestAction: "cargo clippy failed",
      createdAt: "2026-07-13T18:00:00.000Z",
      updatedAt: "2026-07-13T18:20:00.000Z",
      finishedAt: "2026-07-13T18:20:00.000Z",
    },
  ];
}

/** Simulate streaming reply fragments (text, not JSON). */
export function mockStreamingFragments(): string[] {
  return ["Hello", " from", " **stream**", " fragment"];
}

/** Running task detail with streaming thought + tool for live UI demos. */
export function mockRunningTaskDetail(): TaskDetail {
  return {
    task: {
      taskId: "task-demo-2",
      status: "running",
      mode: "write",
      actualModel: "grok-4",
      latestAction: "Reading files",
      createdAt: "2026-07-15T08:00:00.000Z",
      updatedAt: "2026-07-15T08:12:00.000Z",
    },
    title: "Refactor ACP UI",
    cwd: "/tmp/fixture-workspace",
    timeline: [
      {
        itemId: "seg:t2:0:user",
        kind: "user_message",
        turnId: "t2",
        message: "Improve conversation UI",
        text: "Improve conversation UI following open-source ACP clients",
        streaming: false,
        locations: [],
        firstSequence: 1,
        lastSequence: 1,
      },
      {
        itemId: "seg:t2:1:thought",
        kind: "reasoning_segment",
        turnId: "t2",
        message: "Inspecting layout",
        stageTitle: "Inspecting layout",
        text: "Looking at TaskView shell.\n\nNeed history sidebar.\n\nPlan bar at bottom.",
        streaming: true,
        locations: [],
        firstSequence: 2,
        lastSequence: 4,
      },
      {
        itemId: "tool:sess:run1",
        kind: "tool_call",
        turnId: "t2",
        message: "正在读取 src/views/TaskView.vue",
        title: "Read TaskView.vue",
        text: "",
        status: "running",
        toolKind: "read",
        locations: ["src/views/TaskView.vue"],
        streaming: false,
        firstSequence: 5,
        lastSequence: 5,
      },
    ],
    activePlan: {
      itemId: "plan:t2:1",
      currentStep: "Build timeline projection",
      entries: [
        { content: "Build timeline projection", status: "running" },
        { content: "Wire full-window shell", status: "pending" },
        { content: "Add focused tests", status: "pending" },
      ],
    },
    lastSequence: 5,
    timelineGeneration: 1,
  };
}
