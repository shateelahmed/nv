---
description: "Create a feature specification (the WHAT and WHY) under specs/"
agent: "agent"
argument-hint: "<feature description>"
---

Create a new feature **specification** for `nv` from this description:

> ${input:description}

Follow the spec-driven workflow in [specs/README.md](../../specs/README.md).

## Steps

1. Pick the next sequential feature id (`001`, `002`, …) by inspecting existing
   folders in `specs/`, and choose a short kebab-case slug.
2. Create `specs/<NNN-slug>/spec.md` from
   [specs/templates/spec-template.md](../../specs/templates/spec-template.md).
3. Fill in every section. Focus on **what** and **why** — user value, behavior,
   and acceptance criteria. Do **not** describe implementation, code, or file
   layout (that belongs in the plan).
4. Write concrete, testable acceptance criteria using Given/When/Then.
5. List any open questions. If key details are ambiguous, ask me before finalizing.
6. Per spec 001, check whether the spec introduces a durable rule, convention,
   workflow step, or guarantee. If so, note in the spec that both assistant
   configs (`.github/*` and `CLAUDE.md`/`.claude/*`) must be updated together
   during implementation; if not, note that no assistant-config change is
   required.

## Constraints

- Respect the project's core guarantees: formatting-preserving edits,
  `.env.example` secrets stay empty, secrets are raw strings, and every command
  reports its config source.
- Keep the spec focused on a single feature.

End by printing the path to the new `spec.md` and a one-line summary. Do not write
any production code in this step.
