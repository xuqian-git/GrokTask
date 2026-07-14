/**
 * Semantic tool-row display (conversation-stream.md §4.3).
 * Primary line: icon + verb + target + optional stats. No raw ACP JSON.
 */

import { isUnsafePrimaryUiText } from "./markdown";
import type { TimelineEvent } from "./types";

export type ToolVisualStatus =
  "pending" | "running" | "completed" | "failed" | "cancelled" | "unknown";

const LIGHTWEIGHT_KINDS = new Set([
  "read",
  "search",
  "explore",
  "grep",
  "glob",
  "list",
]);

export function normalizeToolKind(kind?: string): string {
  return (kind ?? "unknown").toLowerCase().trim();
}

export function isLightweightToolKind(kind?: string): boolean {
  return LIGHTWEIGHT_KINDS.has(normalizeToolKind(kind));
}

/** Kinds that must never enter lightweight aggregation. */
export function isAggregateForbiddenKind(kind?: string): boolean {
  const k = normalizeToolKind(kind);
  return (
    k === "edit" ||
    k === "write" ||
    k === "terminal" ||
    k === "execute" ||
    k === "bash" ||
    k === "shell" ||
    k === "error" ||
    k === "delete"
  );
}

export function toolVisualStatus(status?: string): ToolVisualStatus {
  const s = (status ?? "").toLowerCase();
  if (s === "pending" || s === "queued") return "pending";
  if (s === "running" || s === "in_progress" || s === "in-progress")
    return "running";
  if (s === "completed" || s === "success" || s === "ok" || s === "done")
    return "completed";
  if (s === "failed" || s === "error") return "failed";
  if (s === "cancelled" || s === "canceled") return "cancelled";
  return "unknown";
}

export function toolStatusIcon(status?: string): string {
  switch (toolVisualStatus(status)) {
    case "pending":
      return "◌";
    case "running":
      return "◉";
    case "completed":
      return "✓";
    case "failed":
      return "✕";
    case "cancelled":
      return "–";
    default:
      return "·";
  }
}

function primaryTarget(ev: TimelineEvent): string {
  if (ev.locations?.length === 1) return ev.locations[0];
  if (ev.locations?.length > 1) return ev.locations[0];
  const title = ev.title?.trim();
  if (title && !isUnsafePrimaryUiText(title)) {
    // Prefer bare path-like segment from title when present
    const pathish = title.match(/(?:^|[\s:])((?:[\w.-]+\/)+[\w.-]+)/);
    if (pathish) return pathish[1];
  }
  return "";
}

function verbFor(kind: string, tense: "present" | "past" | "failed"): string {
  const k = normalizeToolKind(kind);
  const table: Record<string, [string, string, string]> = {
    read: ["正在读取", "读取了", "读取失败"],
    search: ["正在搜索", "搜索了", "搜索失败"],
    grep: ["正在搜索", "搜索了", "搜索失败"],
    glob: ["正在查找", "查找了", "查找失败"],
    explore: ["正在探索", "探索了", "探索失败"],
    list: ["正在列出", "列出了", "列出失败"],
    edit: ["正在修改", "修改了", "修改失败"],
    write: ["正在写入", "写入了", "写入失败"],
    terminal: ["正在运行", "运行了", "运行失败"],
    execute: ["正在运行", "运行了", "运行失败"],
    bash: ["正在运行", "运行了", "运行失败"],
    shell: ["正在运行", "运行了", "运行失败"],
    web: ["正在访问", "访问了", "访问失败"],
    fetch: ["正在获取", "获取了", "获取失败"],
    delete: ["正在删除", "删除了", "删除失败"],
    error: ["错误", "错误", "错误"],
  };
  const row = table[k] ?? ["正在执行", "执行了", "执行失败"];
  if (tense === "present") return row[0];
  if (tense === "failed") return row[2];
  return row[1];
}

/**
 * One-line human title for a tool card.
 * Prefer ACP title/message when already human-readable; otherwise synthesize.
 */
export function toolPrimaryLine(ev: TimelineEvent): string {
  const status = toolVisualStatus(ev.status);
  const kind = normalizeToolKind(ev.toolKind);
  const icon = toolStatusIcon(ev.status);

  // Prefer clean human message/title when present (never protocol labels)
  const rawMsg = (ev.message || ev.title || "").trim();
  const humanMsg = rawMsg && !isUnsafePrimaryUiText(rawMsg) ? rawMsg : "";

  const target = primaryTarget(ev);
  let tense: "present" | "past" | "failed" = "past";
  if (status === "pending" || status === "running" || status === "unknown") {
    tense = "present";
  } else if (status === "failed") {
    tense = "failed";
  }

  // If message already looks like a full human phrase, keep it with status icon.
  if (humanMsg) {
    // Avoid double-prefixing if message already includes tense verbs
    const alreadyPrefixed =
      /^(正在|读取|搜索|修改|运行|写入|探索|查找|列出|访问|获取|删除|执行)/.test(
        humanMsg,
      );
    if (alreadyPrefixed || humanMsg.length > 8) {
      return `${icon} ${humanMsg}`.trim();
    }
  }

  const verb = verbFor(kind, tense);
  const parts = [icon, verb];
  if (target) parts.push(target);
  else if (humanMsg) parts.push(humanMsg);
  else if (kind && kind !== "unknown") parts.push(kind);

  return parts.filter(Boolean).join(" ").replace(/\s+/g, " ").trim();
}

export function toolDetailPaths(ev: TimelineEvent): string[] {
  return ev.locations ?? [];
}

/** Safe detail text for expanded tool rows (never raw protocol / ACP labels). */
export function toolDetailText(ev: TimelineEvent): string {
  const t = ev.text?.trim() ?? "";
  if (!t || isUnsafePrimaryUiText(t)) return "";
  return t;
}

/** Aggregate header for N lightweight completed tools. */
export function aggregatePrimaryLine(count: number, kinds: string[]): string {
  const set = new Set(kinds.map(normalizeToolKind));
  let noun = "个动作";
  if (set.size === 1) {
    const only = [...set][0];
    if (only === "read") noun = "个文件";
    else if (only === "search" || only === "grep") noun = "次搜索";
    else if (only === "explore" || only === "glob" || only === "list")
      noun = "个探索";
  } else if ([...set].every((k) => LIGHTWEIGHT_KINDS.has(k))) {
    noun = "个文件";
  }
  return `✓ 探索了 ${count} ${noun}`;
}
