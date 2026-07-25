---
description: Implement the tasks for a feature, keeping tests green
argument-hint: <feature id or slug, optionally a task id like 001-slug T3>
---

Implement the feature (or a specific task):

$ARGUMENTS

Follow the spec-driven workflow in @specs/README.md.

## Steps

1. Locate the feature folder under `specs/`. Read `spec.md`, `plan.md`, and
   `tasks.md`. If `tasks.md` is missing, stop and ask to run `/tasks` first.
2. Work through the unchecked tasks **in order** (or the single task named).
   Implement one task at a time.
3. After each task, run the relevant tests and mark it `[x]` in `tasks.md`.
4. When all tasks are done, run the full verification checklist and confirm every
   acceptance criterion in `spec.md` is met. Update the spec `Status` to
   `Implemented`.

## Constraints

- **Preserve formatting.** All file writes go through `src/parser/`; never
  re-serialize whole files. Comments, ordering, and unrelated lines stay
  byte-identical.
- Example files (`.env.example`, `.env.testing.example`, etc.) receive empty
  values for generated secrets; secrets are raw strings (no base64).
- Edition 2024: the generate module is `src/cli/generate.rs`. `rand` 0.10 methods
  come from `rand::RngExt`.
- Add/extend unit tests alongside the code, especially formatting-preservation
  tests for parser/edit changes.
- Follow the conventions in @CLAUDE.md.

## Verification

Run and report results for:

```sh
cargo build
cargo test
cargo clippy
cargo fmt
```

End with a summary of what changed and the state of `tasks.md`.
