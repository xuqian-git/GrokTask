/**
 * Timeline → render-row projection (conversation-stream.md §4).
 * Pure transform: preserves event order and item IDs; aggregation is render-only.
 */

import { disclosureKey, type ExpansionMap } from "./expansion";
import {
  aggregatePrimaryLine,
  isAggregateForbiddenKind,
  isLightweightToolKind,
  normalizeToolKind,
  toolVisualStatus,
} from "./toolDisplay";
import type { TimelineEvent } from "./types";

export const AGGREGATE_MAX_MEMBERS = 100;

export type RenderRowKind = "event" | "aggregate" | "aggregate_member";

export interface TimelineRenderRow {
  /** Stable key for v-for / virtual list. */
  key: string;
  rowKind: RenderRowKind;
  /** Underlying timeline event (single event or aggregate member). */
  event?: TimelineEvent;
  /** Aggregate header meta. */
  aggregate?: {
    memberItemIds: string[];
    members: TimelineEvent[];
    primaryLine: string;
  };
  /** When true, member is shown flat (group expanded). */
  isFlatMember?: boolean;
}

function isCompletedLightweightTool(ev: TimelineEvent): boolean {
  if (ev.kind !== "tool_call") return false;
  if (isAggregateForbiddenKind(ev.toolKind)) return false;
  if (!isLightweightToolKind(ev.toolKind)) return false;
  return toolVisualStatus(ev.status) === "completed";
}

function memberExpanded(map: ExpansionMap, itemId: string): boolean {
  return map[disclosureKey(itemId, "details")] === "user-expanded";
}

function aggregateExpanded(map: ExpansionMap, firstMemberId: string): boolean {
  const key = disclosureKey(`aggregate:${firstMemberId}`, "details");
  // Also accept key without item: prefix used as row key
  const alt = map[`item:aggregate:${firstMemberId}:details`];
  const state = map[key] ?? alt ?? "auto";
  return state === "user-expanded";
}

export function aggregateDisclosureItemId(firstMemberItemId: string): string {
  return `aggregate:${firstMemberItemId}`;
}

/**
 * Project timeline events into ordered render rows.
 * Adjacent completed lightweight read/search/explore tools may collapse into
 * an aggregate header unless any member is user-expanded.
 */
export function projectTimeline(
  events: TimelineEvent[],
  expansion: ExpansionMap = {},
): TimelineRenderRow[] {
  const rows: TimelineRenderRow[] = [];
  let i = 0;

  while (i < events.length) {
    const ev = events[i];

    if (!isCompletedLightweightTool(ev)) {
      rows.push({
        key: ev.itemId,
        rowKind: "event",
        event: ev,
      });
      i += 1;
      continue;
    }

    // Collect adjacent eligible tools
    const group: TimelineEvent[] = [];
    let j = i;
    while (j < events.length && isCompletedLightweightTool(events[j])) {
      group.push(events[j]);
      j += 1;
      if (group.length >= AGGREGATE_MAX_MEMBERS) break;
    }

    // Single item: never aggregate alone
    if (group.length < 2) {
      rows.push({
        key: group[0].itemId,
        rowKind: "event",
        event: group[0],
      });
      i = j;
      continue;
    }

    // Split group when a member is user-expanded (protected: never hide it)
    const segments = splitByUserExpanded(group, expansion);
    for (const seg of segments) {
      if (seg.mode === "single") {
        rows.push({
          key: seg.members[0].itemId,
          rowKind: "event",
          event: seg.members[0],
        });
      } else if (seg.members.length < 2) {
        rows.push({
          key: seg.members[0].itemId,
          rowKind: "event",
          event: seg.members[0],
        });
      } else {
        const firstId = seg.members[0].itemId;
        const aggItemId = aggregateDisclosureItemId(firstId);
        const kinds = seg.members.map((m) => normalizeToolKind(m.toolKind));
        const primaryLine = aggregatePrimaryLine(seg.members.length, kinds);
        const expanded = aggregateExpanded(expansion, firstId);

        rows.push({
          key: aggItemId,
          rowKind: "aggregate",
          aggregate: {
            memberItemIds: seg.members.map((m) => m.itemId),
            members: seg.members,
            primaryLine,
          },
        });

        if (expanded) {
          for (const m of seg.members) {
            rows.push({
              key: `${aggItemId}::${m.itemId}`,
              rowKind: "aggregate_member",
              event: m,
              isFlatMember: true,
            });
          }
        }
      }
    }

    i = j;
  }

  return rows;
}

type Seg =
  | { mode: "single"; members: TimelineEvent[] }
  | { mode: "group"; members: TimelineEvent[] };

/**
 * Split a run so user-expanded members become standalone rows and do not
 * disappear into a collapsed aggregate.
 */
function splitByUserExpanded(
  group: TimelineEvent[],
  expansion: ExpansionMap,
): Seg[] {
  const out: Seg[] = [];
  let buf: TimelineEvent[] = [];

  const flushBuf = () => {
    if (!buf.length) return;
    if (buf.length === 1) {
      out.push({ mode: "single", members: buf });
    } else {
      out.push({ mode: "group", members: buf });
    }
    buf = [];
  };

  for (const m of group) {
    if (memberExpanded(expansion, m.itemId)) {
      flushBuf();
      out.push({ mode: "single", members: [m] });
    } else {
      buf.push(m);
    }
  }
  flushBuf();
  return out;
}

/** DOM-order kind sequence for tests (ignores aggregate flattening headers). */
export function projectedKindOrder(rows: TimelineRenderRow[]): string[] {
  const kinds: string[] = [];
  for (const r of rows) {
    if (r.rowKind === "aggregate") {
      kinds.push("aggregate");
    } else if (r.event) {
      kinds.push(r.event.kind);
    }
  }
  return kinds;
}

/** Flatten projected rows to underlying item IDs in visual order. */
export function projectedItemIds(rows: TimelineRenderRow[]): string[] {
  const ids: string[] = [];
  for (const r of rows) {
    if (r.rowKind === "aggregate" && r.aggregate) {
      if (!r.aggregate) continue;
      // collapsed: only header key conceptually; expanded members appear as flat rows after
      ids.push(r.key);
    } else if (r.event) {
      ids.push(r.event.itemId);
    }
  }
  return ids;
}
