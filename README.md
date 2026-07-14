# Grok for Codex

`grok-codex` is a Codex plugin that delegates substantial coding work and
read-only code reviews to [Grok Build](https://x.ai/cli). It packages focused
Codex skills, a dependency-free Node.js companion runtime, and an optional
stop-time review gate.

## Features

| Skill | Purpose |
| --- | --- |
| `$grok-setup` | Check the Grok binary and local authentication state. |
| `$grok-rescue` | Delegate implementation, debugging, or diagnosis to Grok. |
| `$grok-review` | Review the working tree or a branch without letting Grok edit files. |
| `$grok-adversarial-review` | Run a skeptical review focused on correctness, security, and edge cases. |

Codex may invoke these skills implicitly when the request clearly calls for
Grok delegation. Mention the skill explicitly when you want deterministic
routing.

## Requirements

- Codex with plugin support.
- [Grok Build](https://x.ai/cli) available as `grok`, or its path set in
  `GROK_BIN`.
- A Grok login (`grok login`) or `XAI_API_KEY` in the environment.
- Node.js 18 or newer.

Delegation sends the task and any repository content Grok reads to xAI. A
write-capable rescue run uses Grok's `--always-approve` mode and may modify the
current workspace.

## Install

Add the marketplace and install the plugin:

```bash
codex plugin marketplace add superchain/grok-plugin-codex
codex plugin add grok-codex@grok-plugin-codex
```

For local development, run these commands from the repository root:

```bash
codex plugin marketplace add .
codex plugin add grok-codex@grok-plugin-codex
```

Start a new Codex task after installation so the new skills and hook are
discovered.

## Usage

```text
Use $grok-setup to check my Grok installation.
Use $grok-rescue to fix the failing auth tests in apps/api.
Use $grok-rescue with --read to diagnose the rendering jitter without edits.
Use $grok-review to review my working tree.
Use $grok-adversarial-review with --base main to challenge this branch.
```

Rescue routing flags:

| Flag | Effect |
| --- | --- |
| `--read` | Run Grok in read-only plan mode. |
| `--background` | Detach the run and return a PID and log path. |
| `--effort <level>` | Set Grok reasoning effort. |
| `--model <id>` | Select a Grok model. |
| `--cwd <path>` | Change the working directory. |
| `--best-of-n <N>` | Run multiple candidates and keep the best. |
| `--check` | Ask Grok to perform a self-verification pass. |
| `--worktree` or `--worktree=<name>` | Run in a new Git worktree. |
| `--resume` | Continue the latest Grok session for the directory. |

Review routing flags include `--base <ref>`, `--scope
auto|working-tree|branch`, `--wait`, `--background`, `--effort`, `--model`, and
`--cwd`. Reviews always use Grok plan mode and receive a captured Git diff;
they cannot edit the repository.

## Optional stop review gate

The bundled `Stop` hook can run one Grok working-tree review before Codex ends
a turn. It is off by default because it adds latency and uses Grok quota.

```bash
export GROK_STOP_REVIEW_GATE=1
```

After enabling it, open `/hooks` in Codex and trust the plugin hook. Unset the
variable to disable the gate.

## Development

```bash
npm test
python3 /path/to/plugin-creator/scripts/validate_plugin.py plugins/grok-codex
```

The runtime tests use a fake Grok executable and never contact xAI.

## Repository layout

```text
.agents/plugins/marketplace.json
plugins/grok-codex/
  .codex-plugin/plugin.json
  hooks/hooks.json
  scripts/grok-companion.mjs
  scripts/stop-review-gate.mjs
  skills/*/SKILL.md
```

## License

[MIT](./LICENSE)
