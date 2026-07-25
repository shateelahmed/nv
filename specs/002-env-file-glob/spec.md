# Spec: Support `.env*` file glob

- **ID:** 002-env-file-glob
- **Status:** Draft
- **Author:** shateel
- **Date:** 2026-07-24

## Summary

Broaden the set of recognized environment files from the explicit `.env` and
`.env.example` to the glob `.env*`, covering `.env.local`, `.env.testing`,
`.env.testing.example`, and any future variants.

## Problem / Motivation

The project currently hard-codes only `.env` and `.env.example` as env file
types. Real-world projects use additional variants such as `.env.local`,
`.env.staging`, and `.env.testing.example`. These files use the same key=value
format and should be discovered, parsed, and edited by `nv` without requiring
configuration changes.

## Goals

- All files matching `.env*` in a service directory are recognized as
  environment files.
- Existing behavior for `.env` and `.env.example` is preserved exactly.
- The `.env.example`-style rule (no real secret values) applies to any file
  whose name contains `.example` (e.g., `.env.testing.example`).

## Non-goals

- Changing how secrets or YAML files are handled.
- Adding new CLI flags to filter env file variants.

## User stories

- As a developer, I want `nv` to discover `.env.local` alongside `.env` so that
  my local overrides are managed in one place.
- As a platform engineer, I want `nv` to edit `.env.testing.example` so that CI
  templates stay in sync without manual file maintenance.

## Behavior & requirements

- `nv` MUST treat every file whose name matches `.env*` as a dotenv file.
- The existing no-real-secrets rule MUST apply to any file containing
  `.example` in its name, not just `.env.example`.
- Files that match `.env*` but are not valid dotenv format SHOULD produce a
  clear error rather than being silently skipped.

## Acceptance criteria

- [ ] Given a service directory containing `.env`, `.env.local`, and
      `.env.testing.example`, when `nv` discovers files, all three are included
      in the result set.
- [ ] Given `.env.testing.example`, when secrets are generated, the file receives
      empty values.
- [ ] Given `.env.local`, when a key is edited, the value is written directly.
- [ ] Existing tests for `.env` and `.env.example` continue to pass unchanged.

## Edge cases

- Empty `.env*` files are treated as valid (zero keys).
- `.env.` (trailing dot, no suffix) is treated as a match.
- Hidden files like `.env.swp` or `.env~` are NOT matched (the glob requires
  `.env` followed by meaningful characters, not just dot/suffix artifacts).

## Open questions

- Should `nv discover --list` label env files with their variant (e.g.,
  `env:local`, `env:testing.example`) for display purposes?
