---
description: Turn an approved spec into a technical plan (the HOW)
argument-hint: <feature id or slug, e.g. 001-fuzzy-scope>
---

Create a technical **plan** for the feature:

$ARGUMENTS

Follow the spec-driven workflow in @specs/README.md.

## Steps

1. Locate the feature folder under `specs/` matching the id/slug. If ambiguous or
   missing, ask.
2. Read its `spec.md` fully. If it is still `Draft` or has unresolved open
   questions, stop and ask to resolve them first.
3. Create `plan.md` based on @specs/templates/plan-template.md.
4. Define architecture (modules changed/added), data flow, key decisions with
   trade-offs, dependencies, risks, and a testing strategy.
5. Every design decision MUST trace back to a requirement in `spec.md`.

## Constraints

- Reuse the existing module layout (`src/parser`, `src/edit`, `src/cli`,
  `src/tui`, …). Writes go through `src/parser/` and preserve formatting.
- Prefer the standard library and existing crates; justify any new dependency.
- Do not introduce a comment-preserving YAML serializer for writes.

Finish by printing the path to `plan.md` and a short summary. Do not write
production code in this step.
