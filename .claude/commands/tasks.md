---
description: Break an approved plan into small, verifiable, ordered tasks
argument-hint: <feature id or slug, e.g. 001-fuzzy-scope>
---

Break the plan into an ordered **task list** for the feature:

$ARGUMENTS

Follow the spec-driven workflow in @specs/README.md.

## Steps

1. Locate the feature folder under `specs/`. Read `spec.md` and `plan.md`. If
   `plan.md` is missing or still `Draft`, stop and ask.
2. Create `tasks.md` based on @specs/templates/tasks-template.md.
3. Decompose the plan into small tasks that can each be completed and verified
   independently. Order them so the project stays buildable at every step.
4. For each task, state its scope, the files it touches, and how it will be
   verified (a specific test name, command, or observable behavior).
5. Include tasks for unit tests that prove formatting preservation whenever
   parser or edit logic is involved.

## Constraints

- Keep the standard verification checklist (`cargo build`, `test`, `clippy`,
  `fmt`) at the end.
- Tasks must collectively satisfy every acceptance criterion in `spec.md`.

Finish by printing the path to `tasks.md` and the task count. Do not write
production code in this step.
