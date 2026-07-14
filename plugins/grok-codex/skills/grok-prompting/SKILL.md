---
name: grok-prompting
description: Shape a vague coding request into a precise one-shot Grok Build task before delegation. Use with grok-rescue when the request needs a clearer goal, scope, constraints, acceptance criteria, or expected output, while preserving the user's intent.
---

# Grok prompting

Prepare a compact prompt that Grok can complete without follow-up questions.
Include:

1. One clear outcome.
2. The relevant working directory, files, functions, or error text.
3. Constraints and files that must not change.
4. Observable acceptance criteria and useful verification commands.
5. The expected final response, such as edited files plus a summary or a
   read-only diagnosis.

Prefer concrete paths and exact errors. Keep one coherent task per run. Do not
invent requirements or turn routing flags into prompt text. Use `--read` for
analysis-only work and reserve `--effort high` or `--check` for tasks where the
extra latency is justified.
