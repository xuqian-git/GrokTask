# Grok Activity local dashboard

## Goal

Show the full Grok Build process in a localhost page without depending on MCP
Apps card rendering. The page displays ACP lifecycle events, plans, thought
chunks, tool calls and updates, observed workspace changes, usage, unknown ACP
updates, and Grok's public output.

## Runtime design

```text
Codex local MCP (stdio)
  -> grok_activity_start
  -> GrokActivityManager
  -> local ACP bridge
  -> grok agent stdio

Same Node.js process
  -> 127.0.0.1 random port
  -> random 256-bit path token
  -> /activity dashboard
  -> /api/activity/latest status polling
```

`grok_activity_start` returns both an activity snapshot and `dashboardUrl`.
The Grok skills instruct Codex to open or reuse this URL in the Codex in-app
Browser when that capability is available. The local service does not invoke
Chrome or any system browser.

The dashboard follows the most recently started job. Its main transcript shows
normalized ACP events as readable messages rather than complete protocol JSON.
Thought chunks are nested under the stage where they occurred and include the
raw thought text. Plans, tools, paths, and usage show only their human-readable
contents. The
live stage expands automatically; stages and event details opened by the user
remain open across polling updates. Reply chunks appear as continuous text,
and the final answer is rendered as safe Markdown without executing raw HTML.
The page follows the newest content until the user scrolls upward, then resumes
following after they return to the bottom. It polls faster while a
job is active and continues low-frequency polling after completion so a later
job appears in the same page. Cancellation is a localhost POST guarded by the
same random token.

Internal ACP lifecycle notifications and command-loading updates remain in the
server snapshot for diagnostics but are hidden from the transcript. Tool calls
stay inside the current stage, merge updates by tool-call ID, and show only the
actual command/path/input/output content instead of adding a separate tool-call
phase.

## Security and privacy

- Bind only to `127.0.0.1`.
- Generate a fresh URL-safe 256-bit token for every server process.
- Apply `no-store`, CSP, `nosniff`, and frame-denial headers to the page.
- Do not put prompts or API keys in URLs.
- Keep activity on localhost behind a random path token; snapshots may contain
  repository content, thought chunks, tool inputs, and tool results.
- Bound each normalized ACP event and redact known credential patterns from
  diagnostics.
- Redact known credential patterns from bounded diagnostics.
- Keep HTTP-mode jobs inside the configured workspace root.
- Run headless read jobs with `dontAsk`, the Grok `read-only` sandbox, explicit
  read/Grep/Git allow rules, Edit/WebFetch deny rules, disabled web search, and
  no subagents. This avoids interactive plan approval while keeping the
  workspace non-writable.
- Treat the dashboard URL as a process-lifetime credential.

## MCP tools

- `grok_activity_start`: starts one asynchronous task and returns
  `{ activity, dashboardUrl }`.
- `grok_activity_status`: returns the latest snapshot for a job.
- `grok_activity_wait`: bounded wait for foreground orchestration.
- `grok_activity_cancel`: idempotently requests cancellation.

The MCP server advertises tools only. It does not advertise an MCP Apps UI
resource or output template, and the plugin has no Developer App manifest.

## Validation

Automated tests cover tool descriptors, dashboard URL propagation, the HTML
and JSON routes, token isolation, cancellation, malformed requests, body
limits, ACP normalization, complete event retention, retention, and a fake ACP
end-to-end runtime. Plugin and skill validators must also pass.
