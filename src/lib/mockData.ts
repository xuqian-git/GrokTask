import type { TaskDetail, TaskListItem } from "./types";

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
    title: "Demo task",
    cwd: "/tmp/demo",
    timeline: [
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
        text: "I need to verify Thought → Tool → Reply order.\n\nSecond line of thinking.\n\nThird line of thinking.",
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
        text: "ok",
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
        text: "The **reducer** preserves arrival order.\n\n- Thought stages stay in place\n- Tools merge by `toolCallId`\n- Final replies use Markdown\n\n```ts\nreduce(event)\n```\n",
        streaming: false,
        answerMark: "finalAnswer",
        locations: [],
        firstSequence: 7,
        lastSequence: 8,
      },
    ],
    activePlan: undefined,
    lastSequence: 8,
    timelineGeneration: 1,
  };
}

export function mockTaskList(): TaskListItem[] {
  return [
    {
      taskId: "task-demo-1",
      title: "Demo task",
      cwd: "/tmp/demo",
      mode: "read",
      status: "idle",
      actualModel: "fixture",
      latestAction: "Replying: Hello from mock",
      createdAt: "2026-07-14T00:00:00.000Z",
      updatedAt: "2026-07-14T00:01:00.000Z",
      finishedAt: "2026-07-14T00:01:00.000Z",
    },
  ];
}

/** Simulate streaming reply fragments (text, not JSON). */
export function mockStreamingFragments(): string[] {
  return ["Hello", " from", " **stream**", " fragment"];
}
