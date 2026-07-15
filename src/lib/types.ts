/** Shared DTOs mirroring Rust `dto` module (camelCase JSON). */

export type TaskMode = "read" | "write";

export type TaskContainerStatus =
  | "queued"
  | "starting"
  | "running"
  | "cancelling"
  | "recovering"
  | "idle"
  | "cancelled"
  | "failed"
  | "interrupted";

export interface TaskListItem {
  taskId: string;
  title: string;
  cwd: string;
  mode: TaskMode;
  status: TaskContainerStatus;
  actualModel?: string;
  latestAction?: string;
  createdAt: string;
  updatedAt: string;
  finishedAt?: string;
}

export interface TaskStatus {
  taskId: string;
  status: TaskContainerStatus;
  mode: TaskMode;
  sessionState?: string;
  activeTurnId?: string;
  lastTurnId?: string;
  actualModel?: string;
  currentStep?: string;
  latestAction?: string;
  answerPreview?: string;
  createdAt: string;
  updatedAt: string;
  finishedAt?: string;
}

export interface StartResult {
  submissionId: string;
  taskId: string;
  turnId: string;
  turnOrdinal: number;
  status: string;
  mode: TaskMode;
  createdAt: string;
  taskDeleted?: boolean;
}

export interface PlanEntry {
  content: string;
  status?: string;
  priority?: string;
}

export interface TimelineEvent {
  itemId: string;
  kind: string;
  turnId?: string;
  /** Human-meaningful primary line — never raw ACP JSON. */
  message: string;
  text: string;
  title?: string;
  status?: string;
  streaming: boolean;
  answerMark?: string;
  stageTitle?: string;
  toolKind?: string;
  locations: string[];
  planEntries?: PlanEntry[];
  firstSequence: number;
  lastSequence: number;
}

export interface PlanDto {
  itemId: string;
  entries: PlanEntry[];
  currentStep?: string;
}

export interface TaskDetail {
  task: TaskStatus;
  title: string;
  cwd: string;
  requestedModel?: string;
  timeline: TimelineEvent[];
  activePlan?: PlanDto;
  lastSequence: number;
  timelineGeneration: number;
}

export type ExpansionState = "auto" | "user-expanded" | "user-collapsed";
