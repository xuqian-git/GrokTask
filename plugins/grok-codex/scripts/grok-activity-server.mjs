#!/usr/bin/env node

import { randomBytes } from "node:crypto";
import { readFileSync } from "node:fs";
import { createServer } from "node:http";
import { resolve } from "node:path";
import { createInterface } from "node:readline";
import { fileURLToPath, pathToFileURL } from "node:url";

import { ActivityError, GrokActivityManager } from "./grok-activity-core.mjs";

export const MCP_PROTOCOL_VERSION = "2025-06-18";
export const DEFAULT_BODY_LIMIT = 1024 * 1024;

const pluginRoot = resolve(fileURLToPath(new URL("..", import.meta.url)));
const defaultDashboardHtml = readFileSync(resolve(pluginRoot, "public/grok-activity.html"), "utf8");

const eventSchema = {
  type: "object",
  properties: {
    id: { type: "integer" },
    type: { type: "string" },
    summary: { type: "string" },
    at: { type: "string" },
    details: { type: "object" },
  },
  required: ["id", "type", "summary", "at"],
  additionalProperties: true,
};

export const activitySnapshotSchema = {
  type: "object",
  properties: {
    schemaVersion: { type: "integer", const: 1 },
    jobId: { type: "string" },
    status: { type: "string", enum: ["queued", "running", "completed", "failed", "cancelled"] },
    phase: { type: "string", enum: ["queued", "starting", "analyzing", "responding", "verifying", "done"] },
    summary: { type: "string" },
    mode: { type: "string", enum: ["read", "write"] },
    cwd: { type: "string" },
    startedAt: { type: "string" },
    updatedAt: { type: "string" },
    finishedAt: { type: ["string", "null"] },
    elapsedMs: { type: "number" },
    events: { type: "array", items: eventSchema },
    changedFiles: { type: "array", items: { type: "string" } },
    publicText: { type: "string" },
    sessionId: { type: ["string", "null"] },
    stopReason: { type: ["string", "null"] },
    usage: { type: ["object", "null"] },
    error: { type: ["string", "null"] },
    diagnostics: { type: ["string", "null"] },
    cancelRequested: { type: "boolean" },
  },
  required: [
    "schemaVersion",
    "jobId",
    "status",
    "phase",
    "summary",
    "mode",
    "cwd",
    "startedAt",
    "updatedAt",
    "finishedAt",
    "elapsedMs",
    "events",
    "changedFiles",
    "publicText",
    "sessionId",
    "stopReason",
    "usage",
    "error",
    "diagnostics",
    "cancelRequested"
  ],
  additionalProperties: false,
};

const outputSchema = {
  type: "object",
  properties: {
    activity: activitySnapshotSchema,
    dashboardUrl: { type: "string" },
  },
  required: ["activity", "dashboardUrl"],
  additionalProperties: false,
};

const noAuth = [{ type: "noauth" }];

export const toolDescriptors = [
  {
    name: "grok_activity_start",
    title: "Start Grok activity",
    description: "Start an asynchronous Grok Build task and return a live activity snapshot plus a secret localhost dashboard URL. Open or reuse that URL in the Codex in-app Browser. Always pass the current Codex workspace as cwd. Read mode is the default; write mode may modify that workspace.",
    inputSchema: {
      type: "object",
      properties: {
        prompt: { type: "string", minLength: 1, maxLength: 40_000 },
        mode: { type: "string", enum: ["read", "write"], default: "read" },
        cwd: { type: "string" },
        model: { type: "string", minLength: 1 },
        effort: { type: "string", minLength: 1 },
        check: { type: "boolean", default: false },
      },
      required: ["prompt"],
      additionalProperties: false,
    },
    outputSchema,
    securitySchemes: noAuth,
    annotations: {
      readOnlyHint: false,
      destructiveHint: true,
      openWorldHint: true,
      idempotentHint: false,
    },
    _meta: {
      securitySchemes: noAuth,
      "openai/visibility": "public",
      "openai/toolInvocation/invoking": "Starting Grok…",
      "openai/toolInvocation/invoked": "Grok activity started",
    },
  },
  {
    name: "grok_activity_status",
    title: "Refresh Grok activity",
    description: "Return the latest authoritative snapshot for one Grok activity job.",
    inputSchema: {
      type: "object",
      properties: { jobId: { type: "string", minLength: 1 } },
      required: ["jobId"],
      additionalProperties: false,
    },
    outputSchema,
    securitySchemes: noAuth,
    annotations: {
      readOnlyHint: true,
      destructiveHint: false,
      openWorldHint: false,
      idempotentHint: true,
    },
    _meta: {
      securitySchemes: noAuth,
      "openai/visibility": "public",
    },
  },
  {
    name: "grok_activity_wait",
    title: "Wait for Grok activity",
    description: "Wait up to 55 seconds for one Grok activity job to finish, then return its latest snapshot. Call again if it is still active.",
    inputSchema: {
      type: "object",
      properties: {
        jobId: { type: "string", minLength: 1 },
        timeoutMs: { type: "integer", minimum: 0, maximum: 55_000, default: 45_000 },
      },
      required: ["jobId"],
      additionalProperties: false,
    },
    outputSchema,
    securitySchemes: noAuth,
    annotations: {
      readOnlyHint: true,
      destructiveHint: false,
      openWorldHint: false,
      idempotentHint: true,
    },
    _meta: {
      securitySchemes: noAuth,
      "openai/visibility": "public",
      "openai/toolInvocation/invoking": "Waiting for Grok…",
      "openai/toolInvocation/invoked": "Grok status updated",
    },
  },
  {
    name: "grok_activity_cancel",
    title: "Cancel Grok activity",
    description: "Stop one active Grok Build task. Calling it again is safe.",
    inputSchema: {
      type: "object",
      properties: { jobId: { type: "string", minLength: 1 } },
      required: ["jobId"],
      additionalProperties: false,
    },
    outputSchema,
    securitySchemes: noAuth,
    annotations: {
      readOnlyHint: false,
      destructiveHint: false,
      openWorldHint: false,
      idempotentHint: true,
    },
    _meta: {
      securitySchemes: noAuth,
      "openai/visibility": "public",
    },
  },
];

function textResult(activity, dashboardUrl, message) {
  return {
    structuredContent: { activity, dashboardUrl },
    content: [{ type: "text", text: message }],
  };
}

function activityMessage(activity, dashboardUrl) {
  const prefix = `Grok activity ${activity.jobId}: ${activity.status} (${activity.phase}).`;
  const dashboard = dashboardUrl
    ? `\nLocal dashboard: ${dashboardUrl}\nOpen or reuse this URL in the Codex in-app Browser; do not launch an external browser.`
    : "";
  if (activity.status === "failed") return `${prefix} ${activity.error || activity.summary}${dashboard}`;
  if (activity.status === "completed" && activity.publicText) return `${prefix}\n\n${activity.publicText}${dashboard}`;
  return `${prefix} ${activity.summary}${dashboard}`;
}

function validateObject(value, label) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new ActivityError("INVALID_ARGUMENT", `${label} must be an object.`);
  }
  return value;
}

function jsonRpcError(id, code, message, data = undefined) {
  const error = { code, message };
  if (data !== undefined) error.data = data;
  return { jsonrpc: "2.0", id: id ?? null, error };
}

function jsonRpcResult(id, result) {
  return { jsonrpc: "2.0", id, result };
}

export function createMcpHandler({ manager, onActivityStart, getDashboardUrl = () => "" } = {}) {
  if (!manager) throw new TypeError("manager is required");

  return async function handle(message) {
    if (!message || typeof message !== "object" || Array.isArray(message)) {
      return jsonRpcError(null, -32600, "Invalid Request");
    }
    const { id, method } = message;
    const notification = id === undefined;
    if (message.jsonrpc !== "2.0" || typeof method !== "string") {
      return notification ? null : jsonRpcError(id, -32600, "Invalid Request");
    }

    try {
      let result;
      if (method === "initialize") {
        result = {
          protocolVersion: MCP_PROTOCOL_VERSION,
          capabilities: { tools: { listChanged: false } },
          serverInfo: { name: "grok-codex-activity", version: "0.2.0" },
          instructions: "Start Grok with grok_activity_start and always pass the current Codex workspace as cwd. Open or reuse the returned localhost dashboardUrl in the Codex in-app Browser; never launch an external browser. The page follows later jobs and displays the complete local ACP activity stream, including thought and tool payloads.",
        };
      } else if (method === "notifications/initialized" || method === "notifications/cancelled") {
        return null;
      } else if (method === "ping") {
        result = {};
      } else if (method === "tools/list") {
        result = { tools: toolDescriptors };
      } else if (method === "tools/call") {
        const params = validateObject(message.params, "params");
        const args = params.arguments === undefined ? {} : validateObject(params.arguments, "arguments");
        let activity;
        if (params.name === "grok_activity_start") {
          activity = manager.start(args);
          onActivityStart?.(activity);
        }
        else if (params.name === "grok_activity_status") activity = manager.status(args.jobId);
        else if (params.name === "grok_activity_wait") activity = await manager.wait(args.jobId, args.timeoutMs ?? 45_000);
        else if (params.name === "grok_activity_cancel") activity = manager.cancel(args.jobId);
        else throw new ActivityError("TOOL_NOT_FOUND", `Unknown tool: ${params.name || "(missing)"}`);
        const dashboardUrl = getDashboardUrl();
        result = textResult(activity, dashboardUrl, activityMessage(activity, dashboardUrl));
      } else {
        return notification ? null : jsonRpcError(id, -32601, "Method not found");
      }
      return notification ? null : jsonRpcResult(id, result);
    } catch (error) {
      if (method === "tools/call" && error instanceof ActivityError) {
        const result = {
          isError: true,
          content: [{ type: "text", text: `${error.code}: ${error.message}` }],
          _meta: { code: error.code },
        };
        return notification ? null : jsonRpcResult(id, result);
      }
      const code = error instanceof ActivityError ? -32602 : -32603;
      const data = error instanceof ActivityError ? { code: error.code } : undefined;
      return notification ? null : jsonRpcError(id, code, error.message || "Internal error", data);
    }
  };
}

export function startStdioServer(options = {}) {
  const manager = options.manager || new GrokActivityManager({
    transport: "stdio",
    root: options.root || process.cwd(),
    env: options.env || process.env,
  });
  const handle = createMcpHandler({
    manager,
    onActivityStart: options.onActivityStart,
    getDashboardUrl: options.getDashboardUrl,
  });
  const lines = createInterface({ input: options.input || process.stdin, crlfDelay: Infinity });
  const output = options.output || process.stdout;
  lines.on("line", async (line) => {
    if (!line.trim()) return;
    let request;
    try {
      request = JSON.parse(line);
    } catch {
      output.write(`${JSON.stringify(jsonRpcError(null, -32700, "Parse error"))}\n`);
      return;
    }
    const response = await handle(request);
    if (response) output.write(`${JSON.stringify(response)}\n`);
  });
  return { manager, lines };
}

function applyCors(res) {
  res.setHeader("Access-Control-Allow-Origin", "*");
  res.setHeader("Access-Control-Allow-Methods", "POST, OPTIONS");
  res.setHeader("Access-Control-Allow-Headers", "content-type, mcp-session-id, mcp-protocol-version");
  res.setHeader("Access-Control-Expose-Headers", "Mcp-Session-Id, MCP-Protocol-Version");
  res.setHeader("MCP-Protocol-Version", MCP_PROTOCOL_VERSION);
}

async function readJsonBody(req, limit) {
  const chunks = [];
  let size = 0;
  for await (const chunk of req) {
    size += chunk.length;
    if (size > limit) throw new ActivityError("BODY_TOO_LARGE", `Request body exceeds ${limit} bytes.`);
    chunks.push(chunk);
  }
  const raw = Buffer.concat(chunks).toString("utf8");
  try {
    return JSON.parse(raw);
  } catch {
    throw new ActivityError("INVALID_JSON", "Request body must be valid JSON.");
  }
}

export async function startHttpServer(options = {}) {
  const host = options.host || "127.0.0.1";
  const port = Number(options.port ?? 0);
  const token = options.token || randomBytes(32).toString("hex");
  if (!/^[A-Za-z0-9_-]{32,}$/.test(token)) {
    throw new ActivityError("INVALID_TOKEN", "HTTP MCP token must contain at least 32 URL-safe characters.");
  }
  const root = options.root || process.cwd();
  const manager = options.manager || new GrokActivityManager({
    transport: "http",
    root,
    env: options.env || process.env,
  });
  const dashboardHtml = options.dashboardHtml || defaultDashboardHtml;
  const mcpEnabled = options.mcpEnabled !== false;
  const endpointPath = `/${token}/mcp`;
  const healthPath = `/${token}/health`;
  const dashboardPath = `/${token}/activity`;
  const activityApiPath = `/${token}/api/activity`;
  const bodyLimit = options.bodyLimit ?? DEFAULT_BODY_LIMIT;
  let latestJobId = null;
  let dashboardUrl = "";

  const notifyActivityStart = (activity) => {
    latestJobId = activity.jobId;
    options.onActivityStart?.(activity);
  };
  const handle = createMcpHandler({
    manager,
    onActivityStart: notifyActivityStart,
    getDashboardUrl: () => dashboardUrl,
  });

  function sendActivity(res, activity, status = 200) {
    res.writeHead(status, { "content-type": "application/json", "cache-control": "no-store" });
    res.end(JSON.stringify({ activity }));
  }

  const server = createServer(async (req, res) => {
    const url = new URL(req.url || "/", `http://${req.headers.host || "localhost"}`);
    if (url.pathname === healthPath && req.method === "GET") {
      res.writeHead(200, { "content-type": "application/json", "cache-control": "no-store" });
      res.end(JSON.stringify({ ok: true, server: "grok-codex-activity" }));
      return;
    }
    if (url.pathname === dashboardPath && req.method === "GET") {
      res.writeHead(200, {
        "content-type": "text/html; charset=utf-8",
        "cache-control": "no-store",
        "content-security-policy": "default-src 'self'; connect-src 'self'; img-src 'self' data:; style-src 'unsafe-inline'; script-src 'unsafe-inline'; base-uri 'none'; frame-ancestors 'none'",
        "x-content-type-options": "nosniff",
        "x-frame-options": "DENY",
      });
      res.end(dashboardHtml);
      return;
    }
    if (url.pathname === `${activityApiPath}/latest` && req.method === "GET") {
      if (!latestJobId) {
        sendActivity(res, null);
        return;
      }
      try {
        sendActivity(res, manager.status(latestJobId));
      } catch (error) {
        latestJobId = null;
        sendActivity(res, null, error instanceof ActivityError ? 404 : 500);
      }
      return;
    }
    if (url.pathname.startsWith(`${activityApiPath}/`)) {
      const suffix = url.pathname.slice(activityApiPath.length + 1);
      const cancel = suffix.endsWith("/cancel");
      const encodedJobId = cancel ? suffix.slice(0, -"/cancel".length) : suffix;
      let jobId;
      try {
        jobId = decodeURIComponent(encodedJobId);
      } catch {
        sendActivity(res, null, 400);
        return;
      }
      try {
        if (req.method === "GET" && !cancel) sendActivity(res, manager.status(jobId));
        else if (req.method === "POST" && cancel) sendActivity(res, manager.cancel(jobId));
        else {
          res.writeHead(405, { "content-type": "application/json", allow: cancel ? "POST" : "GET" });
          res.end(JSON.stringify({ error: "Method Not Allowed" }));
        }
      } catch (error) {
        res.writeHead(error instanceof ActivityError ? 404 : 500, { "content-type": "application/json", "cache-control": "no-store" });
        res.end(JSON.stringify({ error: error.message || "Internal error" }));
      }
      return;
    }
    if (!mcpEnabled || url.pathname !== endpointPath) {
      res.writeHead(404, { "content-type": "text/plain", "cache-control": "no-store" });
      res.end("Not Found");
      return;
    }

    applyCors(res);
    res.setHeader("cache-control", "no-store");
    if (req.method === "OPTIONS") {
      res.writeHead(204).end();
      return;
    }
    if (req.method !== "POST") {
      res.writeHead(405, { "content-type": "application/json", allow: "POST, OPTIONS" });
      res.end(JSON.stringify(jsonRpcError(null, -32600, "Only POST is supported for stateless MCP.")));
      return;
    }
    const contentType = String(req.headers["content-type"] || "").toLowerCase();
    if (!contentType.includes("application/json")) {
      res.writeHead(415, { "content-type": "application/json" });
      res.end(JSON.stringify(jsonRpcError(null, -32600, "Content-Type must be application/json.")));
      return;
    }

    try {
      const request = await readJsonBody(req, bodyLimit);
      const requests = Array.isArray(request) ? request : [request];
      if (!requests.length) {
        res.writeHead(400, { "content-type": "application/json" });
        res.end(JSON.stringify(jsonRpcError(null, -32600, "Invalid Request")));
        return;
      }
      const responses = (await Promise.all(requests.map(handle))).filter(Boolean);
      if (!responses.length) {
        res.writeHead(202).end();
        return;
      }
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify(Array.isArray(request) ? responses : responses[0]));
    } catch (error) {
      const status = error instanceof ActivityError && error.code === "BODY_TOO_LARGE" ? 413 : 400;
      res.writeHead(status, { "content-type": "application/json" });
      res.end(JSON.stringify(jsonRpcError(null, -32700, error.message || "Parse error")));
    }
  });
  server.once("close", () => manager.shutdown?.());

  await new Promise((resolvePromise, reject) => {
    server.once("error", reject);
    server.listen(port, host, resolvePromise);
  });
  const address = server.address();
  const actualPort = typeof address === "object" && address ? address.port : port;
  dashboardUrl = `http://${host}:${actualPort}${dashboardPath}`;
  return {
    server,
    manager,
    token,
    endpointPath,
    healthPath,
    dashboardPath,
    activityApiPath,
    dashboardUrl,
    notifyActivityStart,
    host,
    port: actualPort,
    localUrl: `http://${host}:${actualPort}${endpointPath}`,
  };
}

function takeValue(argv, index, flag) {
  const value = argv[index + 1];
  if (!value || value.startsWith("--")) throw new ActivityError("INVALID_ARGUMENT", `${flag} requires a value.`);
  return value;
}

export function parseServerArgs(argv) {
  const options = { transport: "stdio", host: "127.0.0.1", port: 8787, root: process.cwd(), token: null };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--stdio") options.transport = "stdio";
    else if (arg === "--http") options.transport = "http";
    else if (["--host", "--port", "--root", "--token"].includes(arg)) {
      const value = takeValue(argv, index, arg);
      index += 1;
      if (arg === "--host") options.host = value;
      if (arg === "--port") options.port = Number(value);
      if (arg === "--root") options.root = resolve(value);
      if (arg === "--token") options.token = value;
    } else {
      throw new ActivityError("INVALID_ARGUMENT", `Unknown server flag: ${arg}`);
    }
  }
  if (!Number.isInteger(options.port) || options.port < 0 || options.port > 65_535) {
    throw new ActivityError("INVALID_ARGUMENT", "--port must be an integer from 0 to 65535.");
  }
  return options;
}

export async function main(argv = process.argv.slice(2)) {
  const options = parseServerArgs(argv);
  if (options.transport === "stdio") {
    const manager = new GrokActivityManager({
      transport: "stdio",
      root: options.root,
      env: process.env,
    });
    const dashboard = await startHttpServer({
      manager,
      host: "127.0.0.1",
      port: 0,
      mcpEnabled: false,
    });
    process.stderr.write(`Grok Activity dashboard: ${dashboard.dashboardUrl}\n`);
    startStdioServer({
      ...options,
      manager,
      onActivityStart: dashboard.notifyActivityStart,
      getDashboardUrl: () => dashboard.dashboardUrl,
    });
    return;
  }
  const result = await startHttpServer(options);
  process.stdout.write(`${JSON.stringify({ localUrl: result.localUrl, dashboardUrl: result.dashboardUrl, root: resolve(options.root) })}\n`);
}

const entry = process.argv[1] ? pathToFileURL(process.argv[1]).href : null;
if (entry === import.meta.url) {
  main().catch((error) => {
    process.stderr.write(`grok-activity-server: ${error.message}\n`);
    process.exitCode = 1;
  });
}
