---
description: Create a feature specification (the WHAT and WHY) under specs/
argument-hint: <feature description>
---

Create a new feature **specification** for `nv` from this description:

$ARGUMENTS

Follow the spec-driven workflow in @specs/README.md.

## Steps

1. Determine the next sequential feature id (`001`, `002`, …) by listing existing
   folders in `specs/`, and choose a short kebab-case slug.
2. Create `specs/<NNN-slug>/spec.md` based on @specs/templates/spec-template.md.
3. Fill in every section. Focus on **what** and **why** — user value, behavior,
   and acceptance criteria. Do NOT describe implementation or code (that is the
   plan's job).
4. Write concrete, testable acceptance criteria using Given/When/Then.
5. List open questions. If key details are ambiguous, ask before finalizing.
6. Per spec 001, check whether the spec introduces a durable rule, convention,
   workflow step, or guarantee. If so, note in the spec that both assistant
   configs (`.github/*` and `CLAUDE.md`/`.claude/*`) must be updated together
   during implementation; if not, note that no assistant-config change is
   required.

## Constraints

- Respect the project guarantees: formatting-preserving edits, example file
  secrets stay empty, secrets are raw strings, and every command reports its
  config source.
- Keep the spec focused on a single feature.

Finish by printing the path to the new `spec.md` and a one-line summary. Do not
write production code in this step.
