import assert from "node:assert/strict";
import { chmodSync, mkdtempSync, mkdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { execFileSync, spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import test from "node:test";

import {
  buildReviewArguments,
  buildTaskArguments,
  collectReviewInput,
  main,
  normalizeGrokResult,
  parseReviewArgs,
  parseTaskArgs,
} from "../plugins/grok-codex/scripts/grok-companion.mjs";

const hookPath = fileURLToPath(new URL("../plugins/grok-codex/scripts/stop-review-gate.mjs", import.meta.url));

function createFakeGrok(directory, text = "fake result") {
  const fake = join(directory, "grok");
  writeFileSync(fake, [
    "#!/bin/sh",
    "if [ \"$1\" = \"version\" ]; then",
    "  echo 'grok test-version'",
    "  exit 0",
    "fi",
    `echo '${JSON.stringify({ text, sessionId: "fake-session" })}'`,
    "",
  ].join("\n"));
  chmodSync(fake, 0o755);
  return fake;
}

test("task routing flags stay out of the prompt", () => {
  const options = parseTaskArgs([
    "--read",
    "--background",
    "--effort",
    "high",
    "--best-of-n",
    "3",
    "--worktree=review-fix",
    "--",
    "fix",
    "the auth test",
  ]);

  assert.equal(options.prompt, "fix the auth test");
  assert.equal(options.readOnly, true);
  assert.equal(options.background, true);
  assert.equal(options.bestOfN, 3);
  assert.equal(options.worktree, "review-fix");

  const args = buildTaskArguments(options);
  assert.deepEqual(args.slice(0, 4), ["--single", "fix the auth test", "--output-format", "json"]);
  assert.ok(args.includes("plan"));
  assert.ok(!args.includes("--always-approve"));
});

test("write-capable task mode is the default", () => {
  const args = buildTaskArguments(parseTaskArgs(["--", "implement feature"]));
  assert.ok(args.includes("--always-approve"));
  assert.ok(!args.includes("plan"));
});

test("Grok JSON variants normalize to text and session id", () => {
  const result = normalizeGrokResult(JSON.stringify({
    result: { text: "done", session_id: "session-1", stop_reason: "complete" },
  }));
  assert.equal(result.text, "done");
  assert.equal(result.sessionId, "session-1");
  assert.equal(result.stopReason, "complete");
});

test("working-tree capture includes untracked paths with spaces", () => {
  const directory = mkdtempSync(join(tmpdir(), "grok-codex-git-"));
  execFileSync("git", ["init", "-b", "main"], { cwd: directory });
  execFileSync("git", ["config", "user.email", "test@example.com"], { cwd: directory });
  execFileSync("git", ["config", "user.name", "Test"], { cwd: directory });
  writeFileSync(join(directory, "tracked.txt"), "before\n");
  execFileSync("git", ["add", "tracked.txt"], { cwd: directory });
  execFileSync("git", ["commit", "-m", "base"], { cwd: directory });

  writeFileSync(join(directory, "tracked.txt"), "after\n");
  mkdirSync(join(directory, "new folder"));
  writeFileSync(join(directory, "new folder", "new file.txt"), "new\n");

  const options = parseReviewArgs(["--cwd", directory]);
  const input = collectReviewInput(options);
  assert.equal(input.empty, false);
  assert.match(input.patch, /tracked\.txt/);
  assert.match(input.patch, /new folder\/new file\.txt/);

  const grokArgs = buildReviewArguments(options, input);
  assert.ok(grokArgs.includes("plan"));
  assert.ok(grokArgs.includes("--disable-web-search"));
});

test("working-tree capture supports a repository without an initial commit", () => {
  const directory = mkdtempSync(join(tmpdir(), "grok-codex-empty-git-"));
  execFileSync("git", ["init", "-b", "main"], { cwd: directory });
  writeFileSync(join(directory, "first file.txt"), "first\n");

  const input = collectReviewInput(parseReviewArgs(["--cwd", directory]));
  assert.equal(input.empty, false);
  assert.match(input.patch, /first file\.txt/);
});

test("companion can run against a fake Grok executable without network access", {
  skip: process.platform === "win32" ? "Unix executable fixture" : false,
}, () => {
  const directory = mkdtempSync(join(tmpdir(), "grok-codex-fake-"));
  const fake = createFakeGrok(directory);

  const env = { ...process.env, GROK_BIN: fake, XAI_API_KEY: "test-key" };
  const result = main(["task", "--read", "--", "inspect safely"], env);
  assert.equal(result.code, 0);
  assert.match(result.output, /fake result/);
  assert.match(result.output, /read-only \(plan\)/);
});

test("stop review gate emits native Codex hook output once per turn", {
  skip: process.platform === "win32" ? "Unix executable fixture" : false,
}, () => {
  const directory = mkdtempSync(join(tmpdir(), "grok-codex-hook-"));
  const fake = createFakeGrok(directory, "review finding");
  const repository = join(directory, "repo");
  const pluginData = join(directory, "plugin-data");
  mkdirSync(repository);
  execFileSync("git", ["init", "-b", "main"], { cwd: repository });
  execFileSync("git", ["config", "user.email", "test@example.com"], { cwd: repository });
  execFileSync("git", ["config", "user.name", "Test"], { cwd: repository });
  writeFileSync(join(repository, "file.txt"), "before\n");
  execFileSync("git", ["add", "file.txt"], { cwd: repository });
  execFileSync("git", ["commit", "-m", "base"], { cwd: repository });
  writeFileSync(join(repository, "file.txt"), "after\n");

  const input = JSON.stringify({ cwd: repository, session_id: "session", turn_id: "turn" });
  const env = {
    ...process.env,
    GROK_BIN: fake,
    XAI_API_KEY: "test-key",
    GROK_STOP_REVIEW_GATE: "1",
    PLUGIN_DATA: pluginData,
  };
  const first = spawnSync(process.execPath, [hookPath], { cwd: repository, env, input, encoding: "utf8" });
  assert.equal(first.status, 0);
  const output = JSON.parse(first.stdout);
  assert.equal(output.continue, false);
  assert.match(output.stopReason, /review finding/);
  assert.match(output.stopReason, /Address findings only if the user asks/);

  const second = spawnSync(process.execPath, [hookPath], { cwd: repository, env, input, encoding: "utf8" });
  assert.equal(second.status, 0);
  assert.equal(second.stdout, "");
});
