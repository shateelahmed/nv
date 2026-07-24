# Spec: Keep Copilot and Claude configs in sync with every spec

- **ID:** 001-sync-assistant-configs
- **Status:** Approved
- **Author:** Shateel Ahmed
- **Date:** 2026-07-24

## Summary

Whenever a new feature spec is created (or an existing one materially changes),
the GitHub Copilot and Claude Code customization files MUST be reviewed and, if
affected, updated in the same change so both assistants stay behaviorally
identical.

## Problem / Motivation

`nv` supports two AI assistants that must behave the same way: GitHub Copilot
(`.github/copilot-instructions.md`, `.github/prompts/`) and Claude Code
(`CLAUDE.md`, `.claude/commands/`). These configurations are maintained by hand
and can drift. If a new spec introduces a rule, workflow step, convention, or
guarantee, and only one assistant's config is updated, the two tools give
divergent results. Keeping them in lockstep prevents that drift.

## Goals

- Every new or changed spec triggers a review of both assistants' config files.
- When a spec introduces or changes a durable rule/convention/workflow, both
  Copilot and Claude configs are updated in the same commit.
- Divergence between the two assistant configurations is caught before merge.

## Non-goals

- Auto-generating one assistant's config from the other.
- Consolidating the two toolchains into a single file (they use different
  substitution syntaxes and locations).
- Applying this to one-off, feature-local details that do not represent a durable
  rule.

## User stories

- As a maintainer, I want a spec that codifies keeping both assistant configs in
  sync so contributors update them together.
- As a contributor using either assistant, I want identical behavior regardless
  of which tool I use.

## Behavior & requirements

- When a new spec is added, the author MUST review both:
  - **Copilot**: `.github/copilot-instructions.md` and `.github/prompts/*`.
  - **Claude**: `CLAUDE.md` and `.claude/commands/*`.
- If the spec introduces or changes a durable rule, convention, workflow step, or
  project guarantee, the author MUST apply the equivalent change to **both**
  assistant configs in the same change set.
- Equivalent changes MAY differ in syntax (Copilot uses `${input:...}` and
  `.prompt.md`; Claude uses `$ARGUMENTS` and `.claude/commands/*.md`) but MUST be
  semantically identical.
- If a spec has no cross-cutting rule, the author SHOULD note "no assistant-config
  change required" in the spec or PR so the review is explicit.
- The four workflow commands (`/specify`, `/plan`, `/tasks`, `/implement`) MUST
  exist and stay equivalent across both assistants.

### CLI / TUI surface (if applicable)

None. This is a process/governance spec; it changes contributor workflow and
repository documentation only, not the `nv` binary.

## Acceptance criteria

- [ ] Given a new spec that introduces a durable rule, when the change is
      prepared, then both `.github/copilot-instructions.md` and `CLAUDE.md` (and
      any relevant prompts/commands) reflect the rule.
- [ ] Given the two assistant config sets, when compared, then their rules,
      conventions, and workflow steps are semantically identical.
- [ ] Given a spec with no cross-cutting rule, when reviewed, then it explicitly
      records that no assistant-config change is required.
- [ ] Given the SDD workflow docs, when read, then they state the sync
      requirement (in `specs/README.md` and both instruction files).

## Edge cases

- **Syntax-only differences**: acceptable as long as behavior matches.
- **Assistant-specific capability**: if a rule cannot be expressed in one tool,
  document the limitation in both configs rather than silently omitting it.
- **Spec that only touches assistant configs** (like this one): still follows the
  workflow; the "implementation" is the doc/config edits themselves.

## Open questions

- None.
