/**
 * Disclosure expansion: auto | user-expanded | user-collapsed.
 * Automatic logic must never override user-* states.
 */

import type { ExpansionState } from "./types";

export type ExpansionMap = Record<string, ExpansionState>;

export function disclosureKey(
  itemId: string,
  part: "body" | "details" = "details",
): string {
  return `item:${itemId}:${part}`;
}

export function getExpansion(
  map: ExpansionMap,
  itemId: string,
  part: "body" | "details" = "details",
): ExpansionState {
  return map[disclosureKey(itemId, part)] ?? "auto";
}

/**
 * User click toggles between expanded and collapsed user states.
 * From auto: first click → user-expanded.
 */
export function toggleUserExpansion(
  map: ExpansionMap,
  itemId: string,
  part: "body" | "details" = "details",
): ExpansionMap {
  const key = disclosureKey(itemId, part);
  const cur = map[key] ?? "auto";
  let next: ExpansionState;
  if (cur === "user-expanded") {
    next = "user-collapsed";
  } else {
    // auto or user-collapsed → expand
    next = "user-expanded";
  }
  return { ...map, [key]: next };
}

/**
 * Whether the item body/details should render expanded.
 * auto: streaming → preview/expanded light; completed thought → collapsed summary.
 */
export function isExpanded(
  state: ExpansionState,
  opts: { streaming?: boolean; kind?: string } = {},
): boolean {
  if (state === "user-expanded") return true;
  if (state === "user-collapsed") return false;
  // auto
  if (opts.streaming) return true; // show preview content while streaming
  if (opts.kind === "assistant_segment") return true;
  return false;
}

/**
 * Apply server snapshot without clobbering local user-* choices.
 * Server values fill missing keys and may overwrite local `auto` only.
 * Local `user-expanded` / `user-collapsed` always win.
 */
export function mergeServerExpansions(
  local: ExpansionMap,
  server: ExpansionMap,
): ExpansionMap {
  const out: ExpansionMap = { ...local };
  for (const [key, serverState] of Object.entries(server)) {
    const localState = local[key];
    if (localState === "user-expanded" || localState === "user-collapsed") {
      continue;
    }
    // missing or auto → take server
    out[key] = serverState;
  }
  return out;
}

/**
 * Automatic complete/stream events must not change user-* keys.
 */
export function applyAutoOnly(
  map: ExpansionMap,
  itemId: string,
  autoState: "auto",
  part: "body" | "details" = "details",
): ExpansionMap {
  const key = disclosureKey(itemId, part);
  const cur = map[key];
  if (cur === "user-expanded" || cur === "user-collapsed") {
    return map;
  }
  return { ...map, [key]: autoState };
}
