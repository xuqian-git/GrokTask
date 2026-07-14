#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, mkdirSync, openSync, readFileSync, closeSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

function truthy(value) {
  return /^(1|true|on|yes)$/i.test(String(value || "").trim());
}

function readInput() {
  try {
    const text = readFileSync(0, "utf8").trim();
    return text ? JSON.parse(text) : {};
  } catch {
    return {};
  }
}

function hasChanges(cwd) {
  const result = spawnSync("git", ["status", "--porcelain=v1", "--untracked-files=all"], {
    cwd,
    encoding: "utf8",
  });
  return result.status === 0 && Boolean(result.stdout.trim());
}

function claimTurn(input) {
  const identity = [input.session_id, input.turn_id].filter(Boolean).join(":");
  if (!identity) return true;
  const dataRoot = process.env.PLUGIN_DATA || join(tmpdir(), "grok-codex-plugin-data");
  const directory = join(dataRoot, "stop-review-gate");
  mkdirSync(directory, { recursive: true });
  const key = createHash("sha256").update(identity).digest("hex");
  const marker = join(directory, key);
  if (existsSync(marker)) return false;
  try {
    const fd = openSync(marker, "wx");
    closeSync(fd);
    return true;
  } catch {
    return false;
  }
}

function finish() {
  process.exitCode = 0;
}

try {
  if (!truthy(process.env.GROK_STOP_REVIEW_GATE)) {
    finish();
  } else {
    const input = readInput();
    const cwd = input.cwd || process.cwd();
    if (input.stop_hook_active || !hasChanges(cwd) || !claimTurn(input)) {
      finish();
    } else {
      const companion = join(dirname(fileURLToPath(import.meta.url)), "grok-companion.mjs");
      const review = spawnSync(process.execPath, [companion, "review", "--wait", "--cwd", cwd], {
        cwd,
        encoding: "utf8",
        maxBuffer: 64 * 1024 * 1024,
        timeout: 600_000,
      });
      const text = (review.stdout || "").trim();
      if (review.status === 0 && text) {
        process.stdout.write(`${JSON.stringify({
          continue: false,
          stopReason: `${text}\n\nGrok stop review gate completed. Address findings only if the user asks for fixes.`,
        })}\n`);
      }
      finish();
    }
  }
} catch {
  // The optional gate is fail-open so it can never trap a Codex task.
  finish();
}
