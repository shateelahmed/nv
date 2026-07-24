---
description: "Break an approved plan into small, verifiable, ordered tasks"
agent: "agent"
argument-hint: "<feature id or slug, e.g. 001-fuzzy-scope>"
---

Break the plan into an ordered **task list** for the feature:

> ${input:feature}

Follow the spec-driven workflow in [specs/README.md](../../specs/README.md).

## Steps

1. Locate the feature folder under `specs/`. Read both `spec.md` and `plan.md`.
   If `plan.md` is missing or still `Draft`, stop and ask me.
2. Create `tasks.md` from
   [specs/templates/tasks-template.md](../../specs/templates/tasks-template.md).
3. Decompose the plan into small tasks that can each be completed and verified
   independently. Order them so the project stays buildable at each step.
4. For every task, state its scope, the files it touches, and how it will be
   verified (a specific test name, command, or observable behavior).
5. Include tasks for unit tests that prove formatting preservation whenever
   parser or edit logic is involved.

## Constraints

- Keep the standard verification checklist (`cargo build`, `test`, `clippy`,
  `fmt`) at the end.
- Tasks must collectively satisfy every acceptance criterion in `spec.md`.

End by printing the path to `tasks.md` and the task count. Do not write production
code in this step.
