#!/usr/bin/env node

import { spawn } from "node:child_process";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

import { ActivityError } from "./grok-activity-core.mjs";
import { startHttpServer } from "./grok-activity-server.mjs";

function takeValue(argv, index, flag) {
  const value = argv[index + 1];
  if (!value || value.startsWith("--")) throw new ActivityError("INVALID_ARGUMENT", `${flag} requires a value.`);
  return value;
}

export function parseDevArgs(argv) {
  const options = {
    root: process.cwd(),
    port: 0,
    cloudflared: process.env.CLOUDFLARED_BIN || "cloudflared",
    tunnel: true,
  };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--no-tunnel") options.tunnel = false;
    else if (["--root", "--port", "--cloudflared"].includes(arg)) {
      const value = takeValue(argv, index, arg);
      index += 1;
      if (arg === "--root") options.root = resolve(value);
      if (arg === "--port") options.port = Number(value);
      if (arg === "--cloudflared") options.cloudflared = value;
    } else {
      throw new ActivityError("INVALID_ARGUMENT", `Unknown development flag: ${arg}`);
    }
  }
  if (!Number.isInteger(options.port) || options.port < 0 || options.port > 65_535) {
    throw new ActivityError("INVALID_ARGUMENT", "--port must be an integer from 0 to 65535.");
  }
  return options;
}

function tunnelUrlFrom(line) {
  return line.match(/https:\/\/[a-z0-9-]+\.trycloudflare\.com/i)?.[0] || null;
}

export async function main(argv = process.argv.slice(2)) {
  const options = parseDevArgs(argv);
  const activity = await startHttpServer({ root: options.root, port: options.port });
  process.stdout.write(`Local MCP: ${activity.localUrl}\n`);
  process.stdout.write(`Local dashboard: ${activity.dashboardUrl}\n`);
  process.stdout.write(`Root: ${resolve(options.root)}\n`);

  let tunnel = null;
  if (options.tunnel) {
    tunnel = spawn(options.cloudflared, ["tunnel", "--no-autoupdate", "--url", `http://127.0.0.1:${activity.port}`], {
      stdio: ["ignore", "pipe", "pipe"],
    });
    let announced = false;
    let tunnelOutput = "";
    const consume = (chunk) => {
      tunnelOutput = `${tunnelOutput}${chunk}`.slice(-16_000);
      const base = tunnelUrlFrom(tunnelOutput);
      if (base && !announced) {
        announced = true;
        process.stdout.write(`\nDeveloper app MCP URL:\n${base}${activity.endpointPath}\n\n`);
        process.stdout.write("Keep this process running while testing the developer-mode app.\n");
      }
    };
    tunnel.stdout.setEncoding("utf8");
    tunnel.stderr.setEncoding("utf8");
    tunnel.stdout.on("data", consume);
    tunnel.stderr.on("data", consume);
    tunnel.once("error", (error) => {
      process.stderr.write(`cloudflared: ${error.message}\n`);
      activity.server.close();
      process.exitCode = 1;
    });
    tunnel.once("close", (code) => {
      if (code && !process.exitCode) process.exitCode = code;
      activity.server.close();
    });
  }

  const shutdown = () => {
    tunnel?.kill("SIGTERM");
    activity.server.close(() => process.exit());
  };
  process.once("SIGINT", shutdown);
  process.once("SIGTERM", shutdown);
}

const entry = process.argv[1] ? pathToFileURL(process.argv[1]).href : null;
if (import.meta.url === entry) {
  main().catch((error) => {
    process.stderr.write(`start-grok-activity-dev: ${error.message}\n`);
    process.exitCode = 1;
  });
}
