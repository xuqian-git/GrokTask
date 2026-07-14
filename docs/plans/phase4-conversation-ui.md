# Phase 4 Conversation UI implementation brief

Status: ready to delegate to Grok. Codex owns this brief; Grok owns all product/test code.

This phase implements the conversation experience. It must not start tray/menu-bar, agent config editing, old plugin removal, release packaging, or README migration work.

## Why this phase exists

The current UI is only a minimum timeline renderer from Phase 2–3. The next batch must turn it into a useful GrokTask conversation UI, borrowing interaction patterns from open-source ACP clients while preserving GrokTask's own stricter model:

- strict semantic timeline order;
- no raw ACP notifications in normal UI;
- thought stages inside the timeline, not hidden in a separate global panel;
- tool rows that read like “what Grok is doing”;
- bottom-follow that respects manual scroll;
- user disclosure state that persists and is never auto-overridden;
- final assistant content rendered as safe Markdown.

Primary specs:

- `docs/specs/conversation-stream.md`
- `docs/research/acp-conversation-flow.md`
- `docs/acceptance.md`, sections 7–9

Important open-source references:

- ACP UI `cd9c3cb464a4b321bff652101953a64c07473e31`: Vue/Tauri app structure, session list, header controls, compact tool status, traffic monitor separation.
- Harnss `dc1dfd8a33caa46a1eefcfe9e14697b27ac4c33d`: rich ACP workbench, semantic tool visualization, multi-pane product feel.
- Zed ACP thread: strict conversation/editor integration and ordered thread model.

Use these as product references only. Do not copy code.

## Scope for this batch

Implement a Phase 4-quality frontend using existing Phase 2–3 DTOs and mock adapter where daemon live subscription is not ready. Prefer pure TypeScript transforms and fixtures so review can validate behavior without a real Grok process.

### In scope

1. Task shell
   - Full-window layout with left history column, task header, timeline, active Plan bar placeholder/projection, composer placeholder.
   - Popover compact layout using the same timeline item components/projection logic.
   - History items show title, cwd, mode, status, model when available, and updated/finished time.

2. Timeline projection
   - Convert `TimelineEvent[]` into render rows without changing persisted item IDs.
   - Preserve strict event order for user/reasoning/tool/plan/permission/assistant/context notice.
   - Add render-only aggregation for adjacent completed lightweight read/search/explore tool rows, respecting the spec's protected-anchor rules at least for user-expanded member avoidance.
   - Do not aggregate edit, terminal, error, permission, plan, assistant, or reasoning rows.

3. Reasoning stages
   - Each `reasoning_segment` is a stage-level thought block at its timeline position.
   - Auto state: streaming shows a three-line preview; completed shows a concise summary.
   - User-expanded shows full safe Markdown; user-collapsed shows only title/summary.
   - Summary priority: `stageTitle` → first Markdown heading → first complete sentence → first non-empty 80-grapheme-ish text fallback → “思考过程”.

4. Tool rows
   - Semantic one-line title with icon/status/target/stats.
   - Use current `message`, `title`, `toolKind`, `locations`, `status`, and `text` fields.
   - Running/pending uses present tense; completed uses past tense; failed is explicit.
   - Expanded details show safe text/pre output and paths. Keep raw JSON out of normal UI.

5. Assistant replies
   - Streaming assistant text displays stable plain text without markdown flicker.
   - Completed/final assistant text uses safe Markdown.
   - No duplicate “final answer” card.

6. Active Plan bar
   - Render `TaskDetail.activePlan` between timeline and composer when present.
   - Show all steps in a bounded scroll area, with current step and completed/total count.
   - Do not put thought summaries inside Plan.

7. Scroll and disclosure state
   - Improve `createScrollController` / `TimelineView` to handle wheel, touch, scrollbar, resize/content growth, unread count, and jump-to-latest.
   - Preserve manual expansion across event changes, aggregation changes, task completion, and popover/full-window reuse.
   - Keep tests focused and deterministic; deep virtualization can be a lightweight windowing/projection test if full virtualization is too large for this batch.

8. Markdown and safety
   - Keep current no-raw-HTML policy.
   - Extend safe Markdown enough for tables, task lists, fenced code, blockquote, links, headings, lists, inline code.
   - Continue blocking dangerous URLs and raw tags.

9. Tests
   - DOM order fixture for Thought → Tool → Thought → Reply.
   - No raw `session/update`, `tool_call_update`, `_x.ai`, or JSON object visible in normal timeline.
   - Tool update remains one card.
   - Reasoning preview/expanded/collapsed behavior.
   - Lightweight aggregation does not hide a user-expanded member.
   - Manual scroll detach/follow, unread count, jump-to-latest.
   - Final Markdown safety including script/raw HTML/dangerous URL cases.
   - Popover and full window use same projection and disclosure map.

### Out of scope

- Tauri tray/menu-bar native behavior.
- Real daemon `task.subscribe` streaming IPC if it requires new backend protocol; a mock/live adapter interface is enough for this batch.
- Codex/Claude config editing.
- Removing old plugin files.
- Release packaging.
- Real Grok E2E.

## Current code to start from

- `src/components/timeline/TimelineView.vue`
- `src/components/timeline/TimelineItemCard.vue`
- `src/views/TaskView.vue`
- `src/views/PopoverView.vue`
- `src/views/HistoryView.vue`
- `src/lib/types.ts`
- `src/lib/mockData.ts`
- `src/lib/scroll.ts`
- `src/lib/expansion.ts`
- `src/lib/markdown.ts`
- `src/lib/ipc.ts`
- existing tests under `tests/`

Grok may add frontend-only files such as:

- `src/lib/timelineProjection.ts`
- `src/lib/toolDisplay.ts`
- `src/lib/thoughtSummary.ts`
- `src/components/timeline/*`
- `src/components/plan/*`
- `src/components/history/*`

Backend changes should be minimal and only when DTOs need harmless additions for existing data. Do not change MCP/CLI semantics in this phase.

## Acceptance commands for this batch

Run at minimum:

```text
pnpm lint
pnpm test
pnpm build
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --all-targets --all-features
```

If UI-only changes make Rust unaffected, still run the Rust checks before reporting completion because this branch is still a Rust/Tauri app.

## Reporting requirements

At the end, Grok must report:

- changed files;
- what open-source UI ideas were used, in product terms only;
- tests run and exact results;
- known gaps left for Phase 5–6;
- confirmation that it did not commit and did not start tray/config/release work.
