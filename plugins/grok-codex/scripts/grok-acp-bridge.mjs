#!/usr/bin/env node

import { spawn } from "node:child_process";
import { createInterface } from "node:readline";
import { pathToFileURL } from "node:url";

const MAX_CONFIG_BYTES = 256 * 1024;
const MAX_TEXT_CHARS = 120_000;
const MAX_TITLE_CHARS = 500;
const MAX_EVENT_CHARS = 160_000;

function emit(value) {
  process.stdout.write(`${JSON.stringify(value)}\n`);
}

function boundedText(value, limit = MAX_TITLE_CHARS) {
  return typeof value === "string" ? value.slice(0, limit) : "";
}

function boundedValue(value, limit = MAX_EVENT_CHARS) {
  if (value === undefined) return null;
  try {
    const json = JSON.stringify(value);
    if (json.length <= limit) return JSON.parse(json);
    return { truncated: true, preview: json.slice(0, limit) };
  } catch {
    return { unserializable: true, preview: String(value).slice(0, MAX_TITLE_CHARS) };
  }
}

async function readConfig(input = process.stdin) {
  const chunks = [];
  let size = 0;
  for await (const chunk of input) {
    size += chunk.length;
    if (size > MAX_CONFIG_BYTES) throw new Error("ACP bridge configuration is too large.");
    chunks.push(chunk);
  }
  const config = JSON.parse(Buffer.concat(chunks).toString("utf8"));
  if (!config || typeof config !== "object" || Array.isArray(config)) throw new Error("ACP bridge configuration must be an object.");
  if (typeof config.prompt !== "string" || !config.prompt.trim()) throw new Error("ACP bridge prompt is required.");
  if (typeof config.cwd !== "string" || !config.cwd) throw new Error("ACP bridge cwd is required.");
  if (!new Set(["read", "write"]).has(config.mode)) throw new Error("ACP bridge mode must be read or write.");
  return config;
}

export function acpAgentArgs(config) {
  const args = ["--no-auto-update"];
  if (config.mode === "read") {
    args.push(
      "--sandbox", "read-only",
      "--permission-mode", "dontAsk",
      "--disable-web-search",
      "--no-subagents",
      "--allow", "Read",
      "--allow", "Grep",
      "--allow", "Bash(git *)",
      "--deny", "Edit",
      "--deny", "WebFetch",
    );
  } else {
    args.push("--always-approve");
  }
  if (config.model) args.push("--model", config.model);
  if (config.effort) args.push("--reasoning-effort", config.effort);
  args.push("agent", "stdio");
  return args;
}

function safePlanEntries(entries) {
  if (!Array.isArray(entries)) return [];
  return entries.slice(0, 50).flatMap((entry) => {
    if (!entry || typeof entry !== "object") return [];
    const content = boundedText(entry.content, 1_000);
    if (!content) return [];
    return [{
      content,
      status: boundedText(entry.status, 40) || "pending",
      priority: boundedText(entry.priority, 40) || null,
    }];
  });
}

function safeToolUpdate(update, previous = {}) {
  return {
    toolCallId: boundedText(update.toolCallId || previous.toolCallId, 200),
    title: boundedText(update.title || previous.title) || "Grok tool",
    kind: boundedText(update.kind || previous.kind, 40) || "other",
    status: boundedText(update.status || previous.status, 40) || "pending",
    details: boundedValue(update),
  };
}

export function normalizeAcpUpdate(update, toolCalls = new Map()) {
  if (!update || typeof update !== "object") return [];
  const type = update.sessionUpdate;
  if (type === "agent_thought_chunk") {
    const text = boundedText(update.content?.text, MAX_TEXT_CHARS);
    return [{ type: "thought", data: text, details: boundedValue(update) }];
  }
  if (type === "agent_message_chunk") {
    const text = boundedText(update.content?.text, MAX_TEXT_CHARS);
    return [{ type: "text", data: text, details: boundedValue(update) }];
  }
  if (type === "plan") {
    const entries = safePlanEntries(update.entries);
    return [{ type: "plan", entries, details: boundedValue(update) }];
  }
  if (type === "tool_call" || type === "tool_call_update") {
    const prior = toolCalls.get(update.toolCallId) || {};
    const safe = safeToolUpdate(update, prior);
    if (safe.toolCallId) toolCalls.set(safe.toolCallId, safe);
    return [{ type: "tool", ...safe }];
  }
  if (type === "usage_update") {
    const used = Number(update.used);
    const size = Number(update.size);
    return [{
      type: "context_usage",
      context: {
        used: Number.isFinite(used) ? used : null,
        size: Number.isFinite(size) ? size : null,
        cost: update.cost ?? null,
      },
      details: boundedValue(update),
    }];
  }
  if (type === "current_mode_update") {
    const mode = boundedText(update.currentModeId || update.modeId, 100);
    return [{ type: "lifecycle", summary: mode ? `ACP 模式：${mode}` : "ACP 模式已更新", details: boundedValue(update) }];
  }
  return [{ type: "acp_update", sessionUpdate: boundedText(type, 100) || "unknown", details: boundedValue(update) }];
}

export class AcpJsonRpcClient {
  constructor(proc, options = {}) {
    this.proc = proc;
    this.nextId = 1;
    this.pending = new Map();
    this.toolCalls = new Map();
    this.onEvent = options.onEvent || (() => {});
    this.mode = options.mode || "read";
    this.lines = createInterface({ input: proc.stdout, crlfDelay: Infinity });
    this.lines.on("line", (line) => this.handleLine(line));
    proc.once("close", (code, signal) => {
      const error = new Error(`Grok ACP agent exited before completion (${code ?? signal ?? "unknown"}).`);
      for (const request of this.pending.values()) request.reject(error);
      this.pending.clear();
    });
  }

  send(message) {
    this.proc.stdin.write(`${JSON.stringify(message)}\n`);
  }

  request(method, params, timeoutMs = 30_000) {
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      let timer = null;
      if (timeoutMs > 0) {
        timer = setTimeout(() => {
          this.pending.delete(id);
          reject(new Error(`${method} timed out`));
        }, timeoutMs);
        timer.unref?.();
      }
      this.pending.set(id, {
        resolve(value) { if (timer) clearTimeout(timer); resolve(value); },
        reject(error) { if (timer) clearTimeout(timer); reject(error); },
      });
      this.send({ jsonrpc: "2.0", id, method, params });
    });
  }

  notify(method, params) {
    this.send({ jsonrpc: "2.0", method, params });
  }

  handleLine(line) {
    let message;
    try {
      message = JSON.parse(line);
    } catch {
      this.onEvent({ type: "diagnostic", summary: "收到无法解析的 ACP 消息" });
      return;
    }
    if (message.method === "session/update") {
      for (const event of normalizeAcpUpdate(message.params?.update, this.toolCalls)) this.onEvent(event);
      return;
    }
    if (message.method && message.id !== undefined) {
      this.handleAgentRequest(message);
      return;
    }
    if (message.method) {
      this.onEvent({
        type: "acp_notification",
        summary: `ACP 通知：${boundedText(message.method, 200)}`,
        method: boundedText(message.method, 200),
        details: boundedValue(message.params),
      });
      return;
    }
    const request = this.pending.get(message.id);
    if (!request) return;
    this.pending.delete(message.id);
    if (message.error) request.reject(new Error(message.error.message || JSON.stringify(message.error)));
    else request.resolve(message.result ?? {});
  }

  handleAgentRequest(message) {
    if (message.method !== "session/request_permission") {
      this.send({ jsonrpc: "2.0", id: message.id, error: { code: -32601, message: "Method not supported by grok-codex ACP bridge" } });
      return;
    }
    const options = Array.isArray(message.params?.options) ? message.params.options : [];
    const allow = this.mode === "write"
      ? options.find((option) => ["allow_once", "allow_always"].includes(option.kind))
      : null;
    const reject = options.find((option) => ["reject_once", "reject_always"].includes(option.kind));
    const selected = allow || reject;
    const outcome = selected
      ? { outcome: "selected", optionId: selected.optionId }
      : { outcome: "cancelled" };
    this.onEvent({
      type: "permission",
      summary: this.mode === "write" ? "ACP 权限请求已自动批准" : "ACP 权限请求已拒绝",
      details: boundedValue({ request: message.params, response: outcome }),
    });
    this.send({ jsonrpc: "2.0", id: message.id, result: { outcome } });
  }

  close() {
    this.lines.close();
  }
}

export async function runAcpBridge(config, options = {}) {
  const spawnImpl = options.spawnImpl || spawn;
  const emitEvent = options.onEvent || emit;
  const childEnv = options.env || process.env;
  const grokBin = config.grokBin || process.env.GROK_BIN?.trim() || "grok";
  const proc = spawnImpl(grokBin, acpAgentArgs(config), {
    cwd: config.cwd,
    env: childEnv,
    stdio: ["pipe", "pipe", "pipe"],
  });
  proc.stderr.setEncoding?.("utf8");
  proc.stderr.on("data", (chunk) => process.stderr.write(chunk));

  const client = new AcpJsonRpcClient(proc, { mode: config.mode, onEvent: emitEvent });
  let sessionId = null;
  const stop = () => {
    if (sessionId) client.notify("session/cancel", { sessionId });
    proc.kill("SIGTERM");
    setTimeout(() => proc.kill("SIGKILL"), 500).unref?.();
  };
  process.once("SIGTERM", stop);
  process.once("SIGINT", stop);

  try {
    const initialized = await client.request("initialize", {
      protocolVersion: 1,
      clientCapabilities: {},
      clientInfo: { name: "grok-codex", title: "Grok for Codex", version: "0.3.0" },
    });
    if (initialized.protocolVersion !== 1) throw new Error(`Unsupported ACP protocol version: ${initialized.protocolVersion}`);
    const authMethods = new Set((initialized.authMethods || []).map((method) => method.id));
    emitEvent({ type: "lifecycle", summary: "ACP 初始化完成", details: boundedValue(initialized) });
    const methodId = childEnv.XAI_API_KEY && authMethods.has("xai.api_key")
      ? "xai.api_key"
      : authMethods.has("cached_token") ? "cached_token" : null;
    if (!methodId) throw new Error("Run `grok login` first, or set XAI_API_KEY.");
    await client.request("authenticate", { methodId, _meta: { headless: true } });
    const session = await client.request("session/new", { cwd: config.cwd, mcpServers: [] });
    ({ sessionId } = session);
    emitEvent({ type: "lifecycle", summary: "ACP 会话已建立", sessionId, details: boundedValue(session) });
    const promptText = config.check
      ? `${config.prompt}\n\nBefore finishing, perform a concise self-verification pass.`
      : config.prompt;
    const result = await client.request("session/prompt", {
      sessionId,
      prompt: [{ type: "text", text: promptText }],
    }, 0);
    await new Promise((resolve) => setTimeout(resolve, 300));
    const meta = result?._meta && typeof result._meta === "object" ? result._meta : {};
    const usage = meta.usage && typeof meta.usage === "object"
      ? meta.usage
      : {
          inputTokens: meta.inputTokens,
          outputTokens: meta.outputTokens,
          reasoningTokens: meta.reasoningTokens,
          cacheReadInputTokens: meta.cacheReadInputTokens,
          totalTokens: meta.totalTokens,
        };
    emitEvent({ type: "end", stopReason: result.stopReason || "end_turn", sessionId, usage, details: boundedValue(result) });
  } finally {
    process.removeListener("SIGTERM", stop);
    process.removeListener("SIGINT", stop);
    client.close();
    proc.kill("SIGTERM");
  }
}

export async function main() {
  const config = await readConfig();
  await runAcpBridge(config);
}

const entry = process.argv[1] ? pathToFileURL(process.argv[1]).href : null;
if (entry === import.meta.url) {
  main().catch((error) => {
    process.stderr.write(`grok-acp-bridge: ${error.message}\n`);
    process.exitCode = 1;
  });
}
