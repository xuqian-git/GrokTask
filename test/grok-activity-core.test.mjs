import assert from "node:assert/strict";
import { chmodSync, mkdtempSync, mkdirSync, realpathSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import {
  ActivityError,
  GrokActivityManager,
  defaultCommandFactory,
  isTerminalStatus,
} from "../plugins/grok-codex/scripts/grok-activity-core.mjs";

function fakeGrokScript(directory) {
  const path = join(directory, "fake-grok.mjs");
  writeFileSync(path, `#!/usr/bin/env node
const scenario = process.argv[2];
const send = value => process.stdout.write(typeof value === "string" ? value + "\\n" : JSON.stringify(value) + "\\n");
if (scenario === "success") {
  send({ type: "thought", data: "PRIVATE_REASONING_DO_NOT_EXPOSE" });
  setTimeout(() => send({ type: "text", data: "hel" }), 12);
  setTimeout(() => send({ type: "text", data: "lo" }), 24);
  setTimeout(() => send({ type: "end", stopReason: "EndTurn", sessionId: "session-1", usage: { total_tokens: 9 } }), 36);
  setTimeout(() => process.exit(0), 48);
} else if (scenario === "malformed") {
  send("not-json");
  send({ type: "text", data: "safe" });
  send({ type: "end", stopReason: "EndTurn" });
} else if (scenario === "hang") {
  send({ type: "thought", data: "ANOTHER_PRIVATE_THOUGHT" });
  setInterval(() => {}, 1000);
} else {
  process.stderr.write("boom\\n");
  process.exit(7);
}
`);
  chmodSync(path, 0o755);
  return path;
}

function createManager(root, script, scenario, options = {}) {
  return new GrokActivityManager({
    root,
    transport: options.transport || "stdio",
    pollIntervalMs: options.pollIntervalMs ?? 5,
    killGraceMs: options.killGraceMs ?? 30,
    retentionMs: options.retentionMs,
    now: options.now,
    maxPromptChars: options.maxPromptChars,
    statusProvider: options.statusProvider || (() => []),
    commandFactory: () => ({ command: process.execPath, args: [script, scenario] }),
  });
}

async function waitFor(manager, jobId, predicate, timeoutMs = 3_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const snapshot = manager.status(jobId);
    if (predicate(snapshot)) return snapshot;
    await new Promise((resolve) => setTimeout(resolve, 8));
  }
  throw new Error(`Timed out waiting for ${jobId}`);
}

test("activity exposes public output and full ACP thought events", async () => {
  const root = mkdtempSync(join(tmpdir(), "grok-activity-success-"));
  const script = fakeGrokScript(root);
  let polls = 0;
  const manager = createManager(root, script, "success", {
    statusProvider: () => (++polls > 1 ? ["changed.txt"] : []),
  });

  const initial = manager.start({ prompt: "say hello", mode: "read", cwd: root });
  assert.equal(initial.status, "queued");
  const analyzing = await waitFor(manager, initial.jobId, (snapshot) => snapshot.phase === "analyzing" || snapshot.phase === "responding");
  assert.match(JSON.stringify(analyzing), /PRIVATE_REASONING_DO_NOT_EXPOSE/);

  const final = await waitFor(manager, initial.jobId, (snapshot) => isTerminalStatus(snapshot.status));
  assert.equal(final.status, "completed");
  assert.equal(final.publicText, "hello");
  assert.equal(final.sessionId, "session-1");
  assert.equal(final.usage.total_tokens, 9);
  assert.deepEqual(final.changedFiles, ["changed.txt"]);
  assert.match(JSON.stringify(final), /PRIVATE_REASONING_DO_NOT_EXPOSE/);
  assert.ok(final.events.some((event) => event.type === "analyzing"));
  assert.deepEqual(
    final.events.filter((event) => event.type === "thinking").map((event) => event.details),
    [{
      text: "PRIVATE_REASONING_DO_NOT_EXPOSE",
      chunkCount: 1,
      sequenceStart: 1,
      sequenceEnd: 1,
      acp: {
        sessionUpdate: "agent_thought_chunk",
        content: { type: "text", text: "PRIVATE_REASONING_DO_NOT_EXPOSE" },
        chunkCount: 1,
      },
    }],
  );
  assert.ok(final.events.some((event) => event.type === "workspace_changed"));
});

test("malformed NDJSON is bounded to a diagnostic event", async () => {
  const root = mkdtempSync(join(tmpdir(), "grok-activity-malformed-"));
  const script = fakeGrokScript(root);
  const manager = createManager(root, script, "malformed");
  const job = manager.start({ prompt: "test", cwd: root });
  const final = await waitFor(manager, job.jobId, (snapshot) => isTerminalStatus(snapshot.status));
  assert.equal(final.status, "completed");
  assert.equal(final.publicText, "safe");
  assert.ok(final.events.some((event) => event.type === "diagnostic"));
});

test("bounded wait returns the terminal snapshot without blocking polling", async () => {
  const root = mkdtempSync(join(tmpdir(), "grok-activity-wait-"));
  const script = fakeGrokScript(root);
  const manager = createManager(root, script, "success");
  const job = manager.start({ prompt: "wait for hello", cwd: root });
  const final = await manager.wait(job.jobId, 1_000);
  assert.equal(final.status, "completed");
  assert.equal(final.publicText, "hello");
  await assert.rejects(() => manager.wait(job.jobId, 55_001), /timeoutMs/);
});

test("spawn failures become failed activity snapshots", async () => {
  const root = mkdtempSync(join(tmpdir(), "grok-activity-spawn-"));
  const manager = new GrokActivityManager({
    root,
    statusProvider: () => [],
    commandFactory: () => ({ command: join(root, "missing-command"), args: [] }),
  });
  const job = manager.start({ prompt: "test", cwd: root });
  const final = await waitFor(manager, job.jobId, (snapshot) => isTerminalStatus(snapshot.status));
  assert.equal(final.status, "failed");
  assert.match(final.error, /Failed to launch Grok Build/);
});

test("active activity can be cancelled idempotently", async () => {
  const root = mkdtempSync(join(tmpdir(), "grok-activity-cancel-"));
  const script = fakeGrokScript(root);
  const manager = createManager(root, script, "hang");
  const job = manager.start({ prompt: "wait", cwd: root });
  await waitFor(manager, job.jobId, (snapshot) => snapshot.phase === "analyzing");
  const cancelling = manager.cancel(job.jobId);
  assert.equal(cancelling.cancelRequested, true);
  const final = await waitFor(manager, job.jobId, (snapshot) => snapshot.status === "cancelled");
  assert.equal(final.status, "cancelled");
  assert.equal(manager.cancel(job.jobId).status, "cancelled");
  assert.match(JSON.stringify(final), /ANOTHER_PRIVATE_THOUGHT/);
});

test("HTTP jobs cannot escape the configured root", () => {
  const root = mkdtempSync(join(tmpdir(), "grok-activity-root-"));
  const child = join(root, "child");
  mkdirSync(child);
  const script = fakeGrokScript(root);
  const manager = createManager(root, script, "success", { transport: "http" });
  assert.equal(manager.normalizeInput({ prompt: "ok", cwd: "child" }).cwd, realpathSync(child));
  assert.throws(
    () => manager.normalizeInput({ prompt: "bad", cwd: ".." }),
    (error) => error instanceof ActivityError && error.code === "CWD_OUTSIDE_ROOT",
  );
});

test("prompt limits and invalid modes are rejected before launch", () => {
  const root = mkdtempSync(join(tmpdir(), "grok-activity-input-"));
  const script = fakeGrokScript(root);
  const manager = createManager(root, script, "success", { maxPromptChars: 5 });
  assert.throws(() => manager.start({ prompt: "123456" }), /exceeds 5/);
  assert.throws(() => manager.start({ prompt: "ok", mode: "admin" }), /read or write/);
  assert.throws(() => manager.start({ prompt: "ok" }), /cwd is required/);
});

test("activity launches the ACP bridge with the requested mode", () => {
  const { command, args, stdin } = defaultCommandFactory({
    prompt: "review",
    mode: "read",
    model: null,
    effort: null,
    check: false,
  }, "/repo", {});
  assert.equal(command, process.execPath);
  assert.equal(args.length, 1);
  assert.match(args[0], /grok-acp-bridge\.mjs$/);
  assert.deepEqual(JSON.parse(stdin), {
    grokBin: "grok",
    prompt: "review",
    mode: "read",
    cwd: "/repo",
    model: null,
    effort: null,
    check: false,
  });
});

test("default activity runtime completes an ACP session end to end", async () => {
  const root = mkdtempSync(join(tmpdir(), "grok-activity-acp-"));
  const grok = join(root, "fake-acp-grok.mjs");
  writeFileSync(grok, `#!/usr/bin/env node
import { createInterface } from "node:readline";
const lines = createInterface({ input: process.stdin });
const send = value => process.stdout.write(JSON.stringify(value) + "\\n");
lines.on("line", line => {
  const message = JSON.parse(line);
  if (message.method === "initialize") send({ jsonrpc: "2.0", id: message.id, result: { protocolVersion: 1, authMethods: [{ id: "cached_token" }] } });
  else if (message.method === "authenticate") send({ jsonrpc: "2.0", id: message.id, result: {} });
  else if (message.method === "session/new") send({ jsonrpc: "2.0", id: message.id, result: { sessionId: "acp-session" } });
  else if (message.method === "session/prompt") {
    const sessionId = message.params.sessionId;
    const update = value => send({ jsonrpc: "2.0", method: "session/update", params: { sessionId, update: value } });
    send({ jsonrpc: "2.0", method: "_x.ai/settings/update", params: { theme: "dark" } });
    update({ sessionUpdate: "plan", entries: [{ content: "Inspect", status: "in_progress", priority: "high" }] });
    update({ sessionUpdate: "agent_thought_chunk", content: { type: "text", text: "ACP_FULL_THOUGHT" } });
    update({ sessionUpdate: "tool_call", toolCallId: "tool-1", title: "Read file", kind: "read", status: "in_progress", rawInput: { path: "a.js" } });
    update({ sessionUpdate: "tool_call_update", toolCallId: "tool-1", status: "completed", rawOutput: { text: "contents" } });
    update({ sessionUpdate: "agent_message_chunk", content: { type: "text", text: "hello from acp" } });
    update({ sessionUpdate: "usage_update", used: 77 });
    send({ jsonrpc: "2.0", id: message.id, result: { stopReason: "end_turn", _meta: { usage: { inputTokens: 60, outputTokens: 17, totalTokens: 77 } } } });
  }
});
`);
  chmodSync(grok, 0o755);
  const manager = new GrokActivityManager({
    root,
    env: { ...process.env, GROK_BIN: grok },
    statusProvider: () => [],
    pollIntervalMs: 5,
  });
  const job = manager.start({ prompt: "hello", mode: "read", cwd: root });
  const final = await waitFor(manager, job.jobId, (snapshot) => isTerminalStatus(snapshot.status));
  assert.equal(final.status, "completed");
  assert.equal(final.publicText, "hello from acp");
  assert.equal(final.sessionId, "acp-session");
  assert.equal(final.usage.totalTokens, 77);
  assert.match(JSON.stringify(final), /ACP_FULL_THOUGHT/);
  assert.match(JSON.stringify(final), /rawInput/);
  assert.match(JSON.stringify(final), /contents/);
  assert.ok(final.events.some((event) => event.type === "plan"));
  assert.ok(final.events.some((event) => event.type === "tool_call"));
  assert.ok(final.events.some((event) => event.type === "acp_notification" && event.summary.includes("_x.ai/settings/update")));
});

test("finished jobs expire after the configured retention period", async () => {
  const root = mkdtempSync(join(tmpdir(), "grok-activity-ttl-"));
  const script = fakeGrokScript(root);
  let now = Date.now();
  const manager = createManager(root, script, "malformed", {
    now: () => now,
    retentionMs: 20,
  });
  const job = manager.start({ prompt: "expire", cwd: root });
  await waitFor(manager, job.jobId, (snapshot) => isTerminalStatus(snapshot.status));
  now += 21;
  assert.throws(
    () => manager.status(job.jobId),
    (error) => error instanceof ActivityError && error.code === "JOB_NOT_FOUND",
  );
});
