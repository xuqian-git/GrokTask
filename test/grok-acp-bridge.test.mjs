import assert from "node:assert/strict";
import test from "node:test";

import {
  acpAgentArgs,
  normalizeAcpUpdate,
} from "../plugins/grok-codex/scripts/grok-acp-bridge.mjs";

test("ACP read mode is headless and read-only", () => {
  const args = acpAgentArgs({ mode: "read", model: "grok-code", effort: "high" });
  assert.deepEqual(args.slice(-2), ["agent", "stdio"]);
  assert.ok(args.includes("read-only"));
  assert.ok(args.includes("dontAsk"));
  assert.ok(args.includes("--disable-web-search"));
  assert.ok(args.includes("--no-subagents"));
  assert.ok(args.includes("Bash(git *)"));
  assert.ok(args.includes("--deny"));
  assert.ok(!args.includes("--always-approve"));
  assert.ok(args.includes("grok-code"));
  assert.ok(args.includes("high"));
});

test("ACP write mode automatically approves tool requests", () => {
  const args = acpAgentArgs({ mode: "write" });
  assert.ok(args.includes("--always-approve"));
  assert.ok(!args.includes("read-only"));
  assert.deepEqual(args.slice(-2), ["agent", "stdio"]);
});

test("ACP updates retain thought, plan, tool, usage, and unknown payloads", () => {
  const tools = new Map();
  const thought = normalizeAcpUpdate({
    sessionUpdate: "agent_thought_chunk",
    content: { type: "text", text: "full thought" },
  }, tools)[0];
  assert.equal(thought.type, "thought");
  assert.equal(thought.data, "full thought");
  assert.equal(thought.details.content.text, "full thought");

  const plan = normalizeAcpUpdate({
    sessionUpdate: "plan",
    entries: [{ content: "Inspect", status: "in_progress", priority: "high" }],
  }, tools)[0];
  assert.deepEqual(plan.entries, [{ content: "Inspect", status: "in_progress", priority: "high" }]);

  const tool = normalizeAcpUpdate({
    sessionUpdate: "tool_call",
    toolCallId: "call-1",
    title: "Read file",
    kind: "read",
    status: "in_progress",
    rawInput: { path: "/repo/a.js" },
  }, tools)[0];
  assert.equal(tool.details.rawInput.path, "/repo/a.js");

  const update = normalizeAcpUpdate({
    sessionUpdate: "tool_call_update",
    toolCallId: "call-1",
    status: "completed",
    rawOutput: { content: "source" },
  }, tools)[0];
  assert.equal(update.title, "Read file");
  assert.equal(update.details.rawOutput.content, "source");

  assert.deepEqual(
    normalizeAcpUpdate({ sessionUpdate: "usage_update", used: 42, size: 128, cost: { amount: 1 } }, tools)[0].context,
    { used: 42, size: 128, cost: { amount: 1 } },
  );
  const unknown = normalizeAcpUpdate({ sessionUpdate: "available_commands_update", commands: [{ name: "x" }] }, tools)[0];
  assert.equal(unknown.type, "acp_update");
  assert.equal(unknown.details.commands[0].name, "x");
});
