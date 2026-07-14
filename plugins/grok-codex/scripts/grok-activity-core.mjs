import { spawn, spawnSync } from "node:child_process";
import { randomUUID } from "node:crypto";
import { realpathSync, statSync } from "node:fs";
import { isAbsolute, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const ACTIVITY_SCHEMA_VERSION = 1;
export const DEFAULT_RETENTION_MS = 30 * 60 * 1000;
export const DEFAULT_MAX_EVENTS = 2_000;
export const DEFAULT_MAX_OUTPUT_CHARS = 1_000_000;
export const DEFAULT_MAX_PROMPT_CHARS = 40_000;
export const DEFAULT_MAX_CONCURRENT = 2;
export const DEFAULT_MAX_STREAM_BUFFER = 2 * 1024 * 1024;

const TERMINAL_STATUSES = new Set(["completed", "failed", "cancelled"]);
const ANSI_PATTERN = /\u001b\[[0-?]*[ -\/]*[@-~]/g;
const ACP_BRIDGE_PATH = fileURLToPath(new URL("./grok-acp-bridge.mjs", import.meta.url));

export class ActivityError extends Error {
  constructor(code, message) {
    super(message);
    this.name = "ActivityError";
    this.code = code;
  }
}

function iso(ms) {
  return new Date(ms).toISOString();
}

function isInside(root, candidate) {
  const path = relative(root, candidate);
  return path === "" || (!path.startsWith("..") && !isAbsolute(path));
}

function requireDirectory(path, label = "Working directory") {
  let resolved;
  try {
    resolved = realpathSync(path);
    if (!statSync(resolved).isDirectory()) throw new Error("not a directory");
  } catch {
    throw new ActivityError("INVALID_CWD", `${label} does not exist or is not a directory: ${path}`);
  }
  return resolved;
}

function safeScalarMetadata(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)) return {};
  const safe = {};
  for (const [key, item] of Object.entries(value)) {
    if (["data", "thought", "content", "text", "prompt"].includes(key)) continue;
    if (["string", "number", "boolean"].includes(typeof item)) safe[key] = item;
  }
  return safe;
}

function safeUsage(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  const allowed = new Set([
    "input_tokens",
    "output_tokens",
    "reasoning_tokens",
    "cache_read_input_tokens",
    "total_tokens",
    "inputTokens",
    "outputTokens",
    "reasoningTokens",
    "cacheReadInputTokens",
    "totalTokens",
  ]);
  const usage = {};
  for (const [key, item] of Object.entries(value)) {
    if (allowed.has(key) && typeof item === "number" && Number.isFinite(item)) usage[key] = item;
  }
  return Object.keys(usage).length ? usage : null;
}

function redactDiagnostics(value) {
  return String(value || "")
    .replace(ANSI_PATTERN, "")
    .replace(/\b(Bearer)\s+[A-Za-z0-9._~+\/-]+=*/gi, "$1 [REDACTED]")
    .replace(/\b((?:XAI|OPENAI|ANTHROPIC)_API_KEY\s*[=:]\s*)\S+/gi, "$1[REDACTED]")
    .replace(/\b(xai-[A-Za-z0-9_-]{12,})\b/g, "[REDACTED]")
    .slice(-8_000);
}

function parseGitStatusPaths(output) {
  const paths = [];
  const records = String(output || "").split("\0").filter(Boolean);
  for (let index = 0; index < records.length; index += 1) {
    const record = records[index];
    if (record.length < 4) continue;
    const code = record.slice(0, 2);
    const path = record.slice(3);
    if (path) paths.push(path);
    if ((code.includes("R") || code.includes("C")) && records[index + 1]) {
      paths.push(records[index + 1]);
      index += 1;
    }
  }
  return paths;
}

function defaultStatusProvider(cwd) {
  const result = spawnSync("git", ["status", "--porcelain=v1", "-z", "--untracked-files=all"], {
    cwd,
    encoding: "utf8",
    maxBuffer: 4 * 1024 * 1024,
  });
  if (result.error || result.status !== 0) return null;
  return parseGitStatusPaths(result.stdout);
}

export function defaultCommandFactory(input, cwd, env) {
  return {
    command: process.execPath,
    args: [ACP_BRIDGE_PATH],
    stdin: JSON.stringify({
      grokBin: env.GROK_BIN?.trim() || "grok",
      prompt: input.prompt,
      mode: input.mode,
      cwd,
      model: input.model,
      effort: input.effort,
      check: input.check,
    }),
  };
}

function clone(value) {
  return JSON.parse(JSON.stringify(value));
}

export class GrokActivityManager {
  constructor(options = {}) {
    this.transport = options.transport || "stdio";
    this.root = requireDirectory(options.root || process.cwd(), "Server root");
    this.env = options.env || process.env;
    this.now = options.now || Date.now;
    this.spawnImpl = options.spawnImpl || spawn;
    this.commandFactory = options.commandFactory || defaultCommandFactory;
    this.statusProvider = options.statusProvider || defaultStatusProvider;
    this.pollIntervalMs = options.pollIntervalMs ?? 1_000;
    this.killGraceMs = options.killGraceMs ?? 1_500;
    this.retentionMs = options.retentionMs ?? DEFAULT_RETENTION_MS;
    this.maxEvents = options.maxEvents ?? DEFAULT_MAX_EVENTS;
    this.maxOutputChars = options.maxOutputChars ?? DEFAULT_MAX_OUTPUT_CHARS;
    this.maxPromptChars = options.maxPromptChars ?? DEFAULT_MAX_PROMPT_CHARS;
    this.maxStreamBuffer = options.maxStreamBuffer ?? DEFAULT_MAX_STREAM_BUFFER;
    this.maxConcurrent = options.maxConcurrent ?? DEFAULT_MAX_CONCURRENT;
    this.jobs = new Map();
  }

  resolveCwd(inputCwd) {
    if (this.transport === "http") {
      const candidate = requireDirectory(resolve(this.root, inputCwd || "."));
      if (!isInside(this.root, candidate)) {
        throw new ActivityError("CWD_OUTSIDE_ROOT", "HTTP activity jobs must stay inside the configured server root.");
      }
      return candidate;
    }
    if (!inputCwd) {
      throw new ActivityError("INVALID_CWD", "cwd is required for local STDIO activity jobs.");
    }
    return requireDirectory(inputCwd ? resolve(inputCwd) : this.root);
  }

  normalizeInput(input = {}) {
    const prompt = typeof input.prompt === "string" ? input.prompt.trim() : "";
    if (!prompt) throw new ActivityError("INVALID_PROMPT", "prompt is required.");
    if (prompt.length > this.maxPromptChars) {
      throw new ActivityError("PROMPT_TOO_LONG", `prompt exceeds ${this.maxPromptChars} characters.`);
    }
    const mode = input.mode || "read";
    if (!new Set(["read", "write"]).has(mode)) {
      throw new ActivityError("INVALID_MODE", "mode must be read or write.");
    }
    for (const key of ["model", "effort"]) {
      if (input[key] !== undefined && (typeof input[key] !== "string" || !input[key].trim())) {
        throw new ActivityError("INVALID_ARGUMENT", `${key} must be a non-empty string when provided.`);
      }
    }
    if (input.check !== undefined && typeof input.check !== "boolean") {
      throw new ActivityError("INVALID_ARGUMENT", "check must be a boolean when provided.");
    }
    return {
      prompt,
      mode,
      cwd: this.resolveCwd(input.cwd),
      model: input.model?.trim() || null,
      effort: input.effort?.trim() || null,
      check: Boolean(input.check),
    };
  }

  start(input) {
    this.cleanup();
    const normalized = this.normalizeInput(input);
    const active = [...this.jobs.values()].filter((job) => !TERMINAL_STATUSES.has(job.status)).length;
    if (active >= this.maxConcurrent) {
      throw new ActivityError("BUSY", `At most ${this.maxConcurrent} Grok activity jobs may run at once.`);
    }

    const now = this.now();
    const job = {
      schemaVersion: ACTIVITY_SCHEMA_VERSION,
      jobId: randomUUID(),
      status: "queued",
      phase: "queued",
      summary: "等待 Grok 启动",
      mode: normalized.mode,
      cwd: normalized.cwd,
      startedAt: iso(now),
      updatedAt: iso(now),
      finishedAt: null,
      events: [],
      changedFiles: new Set(),
      publicText: "",
      sessionId: null,
      stopReason: null,
      usage: null,
      error: null,
      diagnostics: null,
      cancelRequested: false,
      _startedMs: now,
      _finishedMs: null,
      _sequence: 0,
      _thoughtChunks: 0,
      _stdoutBuffer: "",
      _stderr: "",
      _sawEnd: false,
      _child: null,
      _poller: null,
      _killTimer: null,
      _waiters: new Set(),
    };
    this.jobs.set(job.jobId, job);
    this.addEvent(job, "queued", job.summary);
    queueMicrotask(() => this.launch(job, normalized));
    return this.snapshot(job);
  }

  launch(job, input) {
    if (job.cancelRequested) {
      this.finish(job, "cancelled", "任务已取消");
      return;
    }
    job.status = "running";
    this.setPhase(job, "starting", "正在启动 Grok Build");

    let command;
    let args;
    let stdin;
    try {
      ({ command, args, stdin } = this.commandFactory(input, job.cwd, this.env));
      job._child = this.spawnImpl(command, args, {
        cwd: job.cwd,
        env: this.env,
        stdio: [stdin === undefined ? "ignore" : "pipe", "pipe", "pipe"],
      });
    } catch (error) {
      this.fail(job, `Failed to launch Grok Build: ${error.message}`);
      return;
    }

    const child = job._child;
    if (!child || !child.stdout || !child.stderr || typeof child.on !== "function") {
      this.fail(job, "Failed to launch Grok Build: invalid child process handle.");
      return;
    }

    child.stdout.setEncoding?.("utf8");
    child.stderr.setEncoding?.("utf8");
    child.stdout.on("data", (chunk) => this.consumeStdout(job, chunk));
    child.stderr.on("data", (chunk) => {
      job._stderr = `${job._stderr}${chunk}`.slice(-16_000);
      this.touch(job);
    });
    child.once("error", (error) => this.fail(job, `Failed to launch Grok Build: ${error.message}`));
    child.once("close", (code, signal) => this.handleClose(job, code, signal));
    if (stdin !== undefined) {
      if (!child.stdin || typeof child.stdin.end !== "function") {
        this.fail(job, "Failed to launch Grok Build: child stdin is unavailable.");
        child.kill?.("SIGTERM");
        return;
      }
      child.stdin.end(stdin);
    }

    this.pollWorkspace(job);
    job._poller = setInterval(() => this.pollWorkspace(job), this.pollIntervalMs);
    job._poller.unref?.();
  }

  consumeStdout(job, chunk) {
    job._stdoutBuffer += String(chunk);
    const lines = job._stdoutBuffer.split(/\r?\n/);
    job._stdoutBuffer = lines.pop() || "";
    for (const line of lines) this.consumeLine(job, line);
    if (job._stdoutBuffer.length > this.maxStreamBuffer) {
      job._stdoutBuffer = "";
      this.addEvent(job, "diagnostic", "单个 Grok 流事件超过大小限制，已丢弃");
    }
  }

  consumeLine(job, line) {
    const trimmed = line.trim();
    if (!trimmed) return;
    let event;
    try {
      event = JSON.parse(trimmed);
    } catch {
      this.addEvent(job, "diagnostic", "收到无法解析的 Grok 流事件");
      return;
    }

    const type = typeof event?.type === "string" ? event.type : "unknown";
    if (type === "thought") {
      job._thoughtChunks += 1;
      this.setPhase(job, "analyzing", "正在分析任务与约束");
      this.appendChunkEvent(job, "thinking", "思考", event.data, job._thoughtChunks, "agent_thought_chunk");
      return;
    }
    if (type === "text") {
      if (typeof event.data === "string") {
        job.publicText = `${job.publicText}${event.data}`.slice(-this.maxOutputChars);
      }
      this.setPhase(job, "responding", "正在整理公开答复");
      this.appendChunkEvent(job, "message_chunk", "Grok 回复", event.data, null, "agent_message_chunk");
      return;
    }
    if (type === "plan") {
      this.setPhase(job, "analyzing", "正在制定与更新执行计划");
      const entries = Array.isArray(event.entries) ? event.entries : [];
      this.addEvent(job, "plan", "Grok 更新了执行计划", { entries, acp: event.details ?? null });
      return;
    }
    if (type === "tool") {
      const toolCallId = event.toolCallId || null;
      const previous = toolCallId
        ? [...job.events].reverse().find((item) => item.type === "tool_call" && item.details?.toolCallId === toolCallId)
        : null;
      const details = {
        toolCallId,
        kind: event.kind || previous?.details?.kind || null,
        status: event.status || previous?.details?.status || null,
        acp: {
          ...(previous?.details?.acp && typeof previous.details.acp === "object" ? previous.details.acp : {}),
          ...(event.details && typeof event.details === "object" ? event.details : {}),
        },
      };
      if (previous) {
        previous.summary = `${event.title || previous.summary.split(" · ")[0] || "Grok 工具"} · ${details.status || "pending"}`;
        previous.details = details;
        this.touch(job);
      } else {
        this.addEvent(job, "tool_call", `${event.title || "Grok 工具"} · ${event.status || "pending"}`, details);
      }
      return;
    }
    if (type === "context_usage") {
      this.addEvent(job, "context_usage", "上下文用量已更新", { context: event.context ?? null, acp: event.details ?? null });
      return;
    }
    if (["lifecycle", "permission", "acp_update", "acp_notification", "diagnostic"].includes(type)) {
      if (typeof event.sessionId === "string") job.sessionId = event.sessionId;
      const label = event.summary || (type === "acp_update" ? `ACP 事件：${event.sessionUpdate || "unknown"}` : `ACP ${type}`);
      this.addEvent(job, type, label, {
        ...(event.method ? { method: event.method } : {}),
        ...(event.sessionUpdate ? { sessionUpdate: event.sessionUpdate } : {}),
        acp: event.details ?? null,
      });
      return;
    }
    if (type === "end") {
      job._sawEnd = true;
      job.sessionId = typeof event.sessionId === "string" ? event.sessionId : null;
      job.stopReason = typeof event.stopReason === "string" ? event.stopReason : null;
      job.usage = safeUsage(event.usage) || job.usage;
      this.addEvent(job, "turn_end", `ACP 回合结束：${job.stopReason || "end_turn"}`, { acp: event.details ?? null });
      this.setPhase(job, "verifying", "正在核对运行结果");
      return;
    }

    this.addEvent(job, "grok_event", `收到 Grok 事件：${type}`, safeScalarMetadata(event));
  }

  pollWorkspace(job) {
    if (TERMINAL_STATUSES.has(job.status)) return;
    let paths;
    try {
      paths = this.statusProvider(job.cwd);
    } catch {
      return;
    }
    if (!Array.isArray(paths)) return;
    const added = [];
    for (const path of paths) {
      if (typeof path !== "string" || !path || job.changedFiles.has(path)) continue;
      if (job.changedFiles.size >= 200) break;
      job.changedFiles.add(path);
      added.push(path);
    }
    if (added.length) this.addEvent(job, "workspace_changed", `观察到 ${added.length} 个工作区变更路径`, { paths: added });
  }

  handleClose(job, code, signal) {
    if (TERMINAL_STATUSES.has(job.status)) return;
    if (job._stdoutBuffer.trim()) this.consumeLine(job, job._stdoutBuffer);
    job._stdoutBuffer = "";
    this.pollWorkspace(job);
    if (job.cancelRequested) {
      this.finish(job, "cancelled", "任务已取消");
      return;
    }
    if (code === 0) {
      this.finish(job, "completed", job.publicText ? "Grok 任务已完成" : "Grok 任务已完成，但没有公开输出");
      return;
    }
    const detail = redactDiagnostics(job._stderr) || `Grok exited with code ${code ?? "unknown"}${signal ? ` (${signal})` : ""}.`;
    this.fail(job, detail);
  }

  fail(job, message) {
    if (TERMINAL_STATUSES.has(job.status)) return;
    job.error = redactDiagnostics(message) || "Grok activity failed.";
    this.finish(job, "failed", "Grok 任务失败");
  }

  finish(job, status, summary) {
    if (TERMINAL_STATUSES.has(job.status)) return;
    const now = this.now();
    job.status = status;
    job.phase = "done";
    job.summary = summary;
    job.updatedAt = iso(now);
    job.finishedAt = iso(now);
    job._finishedMs = now;
    job.diagnostics = job._stderr ? redactDiagnostics(job._stderr) : null;
    if (job._poller) clearInterval(job._poller);
    if (job._killTimer) clearTimeout(job._killTimer);
    job._poller = null;
    job._killTimer = null;
    this.addEvent(job, status, summary);
    for (const resolveWaiter of job._waiters) resolveWaiter(this.snapshot(job));
    job._waiters.clear();
  }

  cancel(jobId) {
    const job = this.requireJob(jobId);
    if (TERMINAL_STATUSES.has(job.status)) return this.snapshot(job);
    job.cancelRequested = true;
    this.setPhase(job, "verifying", "正在停止 Grok 任务");
    if (!job._child) {
      this.finish(job, "cancelled", "任务已取消");
      return this.snapshot(job);
    }
    try {
      job._child.kill("SIGTERM");
      job._killTimer = setTimeout(() => {
        if (!TERMINAL_STATUSES.has(job.status)) job._child?.kill("SIGKILL");
      }, this.killGraceMs);
      job._killTimer.unref?.();
    } catch (error) {
      this.fail(job, `Could not cancel Grok Build: ${error.message}`);
    }
    return this.snapshot(job);
  }

  status(jobId) {
    this.cleanup();
    return this.snapshot(this.requireJob(jobId));
  }

  async wait(jobId, timeoutMs = 45_000) {
    const job = this.requireJob(jobId);
    if (!Number.isInteger(timeoutMs) || timeoutMs < 0 || timeoutMs > 55_000) {
      throw new ActivityError("INVALID_TIMEOUT", "timeoutMs must be an integer from 0 to 55000.");
    }
    if (TERMINAL_STATUSES.has(job.status) || timeoutMs === 0) return this.snapshot(job);
    return new Promise((resolveWait) => {
      let timer;
      const finishWait = (snapshot) => {
        if (timer) clearTimeout(timer);
        job._waiters.delete(finishWait);
        resolveWait(snapshot);
      };
      job._waiters.add(finishWait);
      timer = setTimeout(() => finishWait(this.snapshot(job)), timeoutMs);
      timer.unref?.();
    });
  }

  requireJob(jobId) {
    if (typeof jobId !== "string" || !jobId) throw new ActivityError("INVALID_JOB_ID", "jobId is required.");
    const job = this.jobs.get(jobId);
    if (!job) throw new ActivityError("JOB_NOT_FOUND", "Grok activity job was not found or has expired.");
    return job;
  }

  setPhase(job, phase, summary) {
    const changed = job.phase !== phase;
    job.phase = phase;
    job.summary = summary;
    this.touch(job);
    if (changed) this.addEvent(job, phase, summary);
  }

  touch(job) {
    job.updatedAt = iso(this.now());
  }

  addEvent(job, type, summary, details = undefined) {
    const event = {
      id: ++job._sequence,
      type,
      summary,
      at: iso(this.now()),
    };
    if (details && Object.keys(details).length) event.details = clone(details);
    job.events.push(event);
    if (job.events.length > this.maxEvents) job.events.splice(0, job.events.length - this.maxEvents);
    this.touch(job);
  }

  appendChunkEvent(job, type, label, value, sequence, sessionUpdate) {
    const text = typeof value === "string" ? value : "";
    const previous = job.events[job.events.length - 1];
    if (previous?.type === type && previous.details) {
      previous.details.text = `${previous.details.text || ""}${text}`.slice(-this.maxOutputChars);
      previous.details.chunkCount = (previous.details.chunkCount || 1) + 1;
      if (sequence !== null) previous.details.sequenceEnd = sequence;
      previous.summary = `${label} · ${previous.details.chunkCount} 个分片`;
      previous.details.acp = {
        sessionUpdate,
        content: { type: "text", text: previous.details.text },
        chunkCount: previous.details.chunkCount,
      };
      this.touch(job);
      return;
    }
    this.addEvent(job, type, `${label} · 1 个分片`, {
      text,
      chunkCount: 1,
      ...(sequence === null ? {} : { sequenceStart: sequence, sequenceEnd: sequence }),
      acp: { sessionUpdate, content: { type: "text", text }, chunkCount: 1 },
    });
  }

  cleanup() {
    const cutoff = this.now() - this.retentionMs;
    for (const [id, job] of this.jobs) {
      if (job._finishedMs !== null && job._finishedMs < cutoff) this.jobs.delete(id);
    }
  }

  shutdown() {
    for (const job of this.jobs.values()) {
      if (!TERMINAL_STATUSES.has(job.status)) this.cancel(job.jobId);
    }
  }

  snapshot(job) {
    const end = job._finishedMs ?? this.now();
    return {
      schemaVersion: job.schemaVersion,
      jobId: job.jobId,
      status: job.status,
      phase: job.phase,
      summary: job.summary,
      mode: job.mode,
      cwd: job.cwd,
      startedAt: job.startedAt,
      updatedAt: job.updatedAt,
      finishedAt: job.finishedAt,
      elapsedMs: Math.max(0, end - job._startedMs),
      events: clone(job.events),
      changedFiles: [...job.changedFiles],
      publicText: job.publicText,
      sessionId: job.sessionId,
      stopReason: job.stopReason,
      usage: job.usage ? clone(job.usage) : null,
      error: job.error,
      diagnostics: job.diagnostics,
      cancelRequested: job.cancelRequested,
    };
  }
}

export function isTerminalStatus(status) {
  return TERMINAL_STATUSES.has(status);
}
