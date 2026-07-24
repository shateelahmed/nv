---
description: "Turn an approved spec into a technical plan (the HOW)"
agent: "agent"
argument-hint: "<feature id or slug, e.g. 001-fuzzy-scope>"
---

Create a technical **plan** for the feature:

> ${input:feature}

Follow the spec-driven workflow in [specs/README.md](../../specs/README.md).

## Steps

1. Locate the feature folder under `specs/` matching the id/slug. If it is
   ambiguous or missing, ask me.
2. Read its `spec.md` fully. If the spec is still `Draft` or has unresolved open
   questions, stop and ask me to resolve them first.
3. Create `plan.md` from
   [specs/templates/plan-template.md](../../specs/templates/plan-template.md).
4. Define the architecture: which modules change or are added, data flow, key
   decisions with trade-offs, dependencies, risks, and a testing strategy.
5. Every design decision MUST trace back to a requirement in `spec.md`.

## Constraints

- Reuse the existing module layout (`src/parser`, `src/edit`, `src/cli`,
  `src/tui`, etc.). Writes go through `src/parser/` and must preserve formatting.
- Prefer the standard library and existing crates; justify any new dependency.
- Do not introduce a comment-preserving YAML serializer for writes.

End by printing the path to `plan.md` and a short summary of the approach. Do not
write production code in this step.
