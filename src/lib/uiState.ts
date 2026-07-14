/**
 * Shared UI state maps for disclosure expansion across full window / popover.
 * Scroll/follow remain surface-local; expansion is shared by taskId.
 */

import { reactive } from "vue";
import type { ExpansionMap } from "./expansion";
import type { ScrollFollowState } from "./scroll";
import type { ScrollAnchor } from "./scroll";

/** Per-task expansion map shared by all surfaces in this WebView. */
const expansionByTask = reactive<Record<string, ExpansionMap>>({});

export function getSharedExpansion(taskId: string): ExpansionMap {
  if (!expansionByTask[taskId]) {
    expansionByTask[taskId] = {};
  }
  return expansionByTask[taskId];
}

export function setSharedExpansion(taskId: string, map: ExpansionMap): void {
  expansionByTask[taskId] = map;
}

export function patchSharedExpansion(
  taskId: string,
  patch: ExpansionMap,
): ExpansionMap {
  const cur = getSharedExpansion(taskId);
  const next = { ...cur, ...patch };
  expansionByTask[taskId] = next;
  return next;
}

export function replaceSharedExpansionKey(
  taskId: string,
  map: ExpansionMap,
): void {
  expansionByTask[taskId] = { ...map };
}

/** Surface-local scroll restore (in-memory only). */
export interface SurfaceScrollState {
  state: ScrollFollowState;
  unreadCount: number;
  lastSeenSequence: number;
  anchor: ScrollAnchor;
}

const scrollBySurfaceTask = reactive<
  Record<string, SurfaceScrollState | undefined>
>({});

function surfaceKey(surfaceId: string, taskId: string): string {
  return `${surfaceId}::${taskId}`;
}

export function getSurfaceScroll(
  surfaceId: string,
  taskId: string,
): SurfaceScrollState | undefined {
  return scrollBySurfaceTask[surfaceKey(surfaceId, taskId)];
}

export function setSurfaceScroll(
  surfaceId: string,
  taskId: string,
  snap: SurfaceScrollState,
): void {
  scrollBySurfaceTask[surfaceKey(surfaceId, taskId)] = { ...snap, anchor: { ...snap.anchor } };
}

/** Test helper: clear all in-memory UI state. */
export function resetUiStateForTests(): void {
  for (const k of Object.keys(expansionByTask)) {
    delete expansionByTask[k];
  }
  for (const k of Object.keys(scrollBySurfaceTask)) {
    delete scrollBySurfaceTask[k];
  }
}
