import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

import {
  createMcpHandler,
  startHttpServer,
  toolDescriptors,
} from "../plugins/grok-codex/scripts/grok-activity-server.mjs";

function snapshot(overrides = {}) {
  return {
    schemaVersion: 1,
    jobId: "job-1",
    status: "running",
    phase: "analyzing",
    summary: "正在分析任务与约束",
    mode: "read",
    cwd: "/repo",
    startedAt: "2026-07-14T00:00:00.000Z",
    updatedAt: "2026-07-14T00:00:01.000Z",
    finishedAt: null,
    elapsedMs: 1000,
    events: [],
    changedFiles: [],
    publicText: "",
    sessionId: null,
    stopReason: null,
    usage: null,
    error: null,
    diagnostics: null,
    cancelRequested: false,
    ...overrides,
  };
}

function fakeManager() {
  return {
    start(args) { return snapshot({ mode: args.mode || "read" }); },
    status(jobId) { return snapshot({ jobId }); },
    wait(jobId) { return snapshot({ jobId, status: "completed", phase: "done", summary: "Grok 任务已完成", finishedAt: "2026-07-14T00:00:02.000Z" }); },
    cancel(jobId) { return snapshot({ jobId, cancelRequested: true, summary: "正在停止 Grok 任务" }); },
  };
}

function request(id, method, params = {}) {
  return { jsonrpc: "2.0", id, method, params };
}

test("MCP initialize advertises tools without an Apps resource", async () => {
  const handle = createMcpHandler({ manager: fakeManager() });
  const initialized = await handle(request(1, "initialize", { protocolVersion: "2025-06-18" }));
  assert.equal(initialized.result.serverInfo.name, "grok-codex-activity");
  assert.ok(initialized.result.capabilities.tools);
  assert.equal(initialized.result.capabilities.resources, undefined);

  const tools = await handle(request(2, "tools/list"));
  assert.deepEqual(tools.result.tools.map((tool) => tool.name), [
    "grok_activity_start",
    "grok_activity_status",
    "grok_activity_wait",
    "grok_activity_cancel",
  ]);
  assert.equal(toolDescriptors[0]._meta["openai/outputTemplate"], undefined);
  assert.equal(toolDescriptors[0]._meta.ui, undefined);
  assert.equal(toolDescriptors[1]._meta["openai/visibility"], "public");
  assert.equal(toolDescriptors[2]._meta["openai/visibility"], "public");
  assert.equal(toolDescriptors[3]._meta["openai/visibility"], "public");
});

test("dashboard polls localhost APIs without an MCP Apps bridge", () => {
  const dashboard = readFileSync(new URL("../plugins/grok-codex/public/grok-activity.html", import.meta.url), "utf8");
  assert.match(dashboard, /api\/activity/);
  assert.match(dashboard, /fetch\(`/);
  assert.doesNotMatch(dashboard, /window\.openai|postMessage|ui\/initialize/);
  const script = dashboard.match(/<script>([\s\S]*?)<\/script>/)?.[1];
  assert.ok(script);
  assert.doesNotThrow(() => new Function(script));
  assert.match(script, /groupStages/);
  assert.match(script, /autoOpenStageId/);
  assert.match(script, /renderMarkdown/);
  assert.match(script, /child\.type === "message_chunk"/);
  assert.match(script, /followTail/);
  assert.match(script, /readableEventMessage/);
  assert.match(script, /hiddenEventTypes/);
  assert.doesNotMatch(script, /JSON\.stringify\(child\.details/);
  assert.doesNotMatch(script, /manuallyOpenStages\.delete\(autoOpenStageId\)/);
});

test("MCP start, status, wait, and cancel return snapshots and dashboard URL", async () => {
  const handle = createMcpHandler({
    manager: fakeManager(),
    getDashboardUrl: () => "http://127.0.0.1:4321/token/activity",
  });
  const started = await handle(request(1, "tools/call", {
    name: "grok_activity_start",
    arguments: { prompt: "hello", mode: "read" },
  }));
  assert.equal(started.result.structuredContent.activity.jobId, "job-1");
  assert.equal(started.result.structuredContent.dashboardUrl, "http://127.0.0.1:4321/token/activity");
  assert.match(started.result.content[0].text, /Grok activity job-1/);
  assert.match(started.result.content[0].text, /Local dashboard:/);

  const status = await handle(request(2, "tools/call", {
    name: "grok_activity_status",
    arguments: { jobId: "job-2" },
  }));
  assert.equal(status.result.structuredContent.activity.jobId, "job-2");

  const waited = await handle(request(3, "tools/call", {
    name: "grok_activity_wait",
    arguments: { jobId: "job-2", timeoutMs: 5 },
  }));
  assert.equal(waited.result.structuredContent.activity.status, "completed");

  const cancelled = await handle(request(4, "tools/call", {
    name: "grok_activity_cancel",
    arguments: { jobId: "job-2" },
  }));
  assert.equal(cancelled.result.structuredContent.activity.cancelRequested, true);
});

test("invalid JSON-RPC and unknown tools return protocol-safe errors", async () => {
  const handle = createMcpHandler({ manager: fakeManager() });
  const invalid = await handle({ id: 1, method: "ping" });
  assert.equal(invalid.error.code, -32600);
  const missing = await handle(request(2, "missing/method"));
  assert.equal(missing.error.code, -32601);
  const tool = await handle(request(3, "tools/call", { name: "missing", arguments: {} }));
  assert.equal(tool.result.isError, true);
  assert.match(tool.result.content[0].text, /TOOL_NOT_FOUND/);
});

test("HTTP server protects the secret path and handles CORS", async (t) => {
  const token = "a".repeat(64);
  const running = await startHttpServer({
    manager: fakeManager(),
    token,
    port: 0,
    bodyLimit: 256,
  });
  t.after(() => new Promise((resolve) => running.server.close(resolve)));
  const base = `http://127.0.0.1:${running.port}`;

  const wrong = await fetch(`${base}/wrong/mcp`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(request(1, "ping")),
  });
  assert.equal(wrong.status, 404);

  const options = await fetch(`${base}${running.endpointPath}`, { method: "OPTIONS" });
  assert.equal(options.status, 204);
  assert.equal(options.headers.get("access-control-allow-methods"), "POST, OPTIONS");

  const ping = await fetch(`${base}${running.endpointPath}`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(request(2, "ping")),
  });
  assert.equal(ping.status, 200);
  assert.deepEqual((await ping.json()).result, {});

  const started = await fetch(`${base}${running.endpointPath}`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(request(3, "tools/call", {
      name: "grok_activity_start",
      arguments: { prompt: "hello", mode: "read" },
    })),
  });
  const startedBody = await started.json();
  assert.equal(startedBody.result.structuredContent.dashboardUrl, running.dashboardUrl);

  const dashboard = await fetch(`${base}${running.dashboardPath}`);
  assert.equal(dashboard.status, 200);
  assert.match(dashboard.headers.get("content-security-policy"), /frame-ancestors 'none'/);
  assert.match(await dashboard.text(), /Grok Activity/);

  const latest = await fetch(`${base}${running.activityApiPath}/latest`);
  assert.equal(latest.status, 200);
  assert.equal((await latest.json()).activity.jobId, "job-1");

  const cancelled = await fetch(`${base}${running.activityApiPath}/job-1/cancel`, { method: "POST" });
  assert.equal(cancelled.status, 200);
  assert.equal((await cancelled.json()).activity.cancelRequested, true);
});

test("dashboard-only HTTP server does not expose the MCP endpoint", async (t) => {
  const running = await startHttpServer({
    manager: fakeManager(),
    token: "c".repeat(64),
    port: 0,
    mcpEnabled: false,
  });
  t.after(() => new Promise((resolve) => running.server.close(resolve)));
  const response = await fetch(running.localUrl, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(request(1, "ping")),
  });
  assert.equal(response.status, 404);
});

test("HTTP server rejects invalid content, malformed JSON, and oversized bodies", async (t) => {
  const token = "b".repeat(64);
  const running = await startHttpServer({ manager: fakeManager(), token, port: 0, bodyLimit: 40 });
  t.after(() => new Promise((resolve) => running.server.close(resolve)));
  const url = `http://127.0.0.1:${running.port}${running.endpointPath}`;

  const media = await fetch(url, { method: "POST", body: "{}" });
  assert.equal(media.status, 415);

  const malformed = await fetch(url, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: "{",
  });
  assert.equal(malformed.status, 400);

  const oversized = await fetch(url, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ value: "x".repeat(100) }),
  });
  assert.equal(oversized.status, 413);
});
