# Spec: `nv remove` command

- **ID:** 004-remove-command
- **Status:** Implemented
- **Author:** shateel
- **Date:** 2026-07-24

## Summary

A `nv remove` command that deletes specified environment variable keys from
files, scoped by file type and optionally by microservice. Changes are previewed
and require user confirmation before being applied.

## Problem / Motivation

When rotating secrets, deprecating config, or cleaning up unused variables,
developers need to remove keys across multiple services and file types. There is
no built-in way to do this safely — with a preview — across env files,
configmaps, and secrets at once.

## Goals

- Remove one or more keys from files selected by type (`-e`, `-c`, `-x`) and
  optionally by microservice (`-s`).
- Support a catch-all `-a` flag to remove from all services and all file types.
- Show a diff preview and require confirmation before writing.
- Respect the formatting-preserving guarantee — only the target key line is
  removed; comments, ordering, and other lines stay byte-identical.
- `-a` is mutually exclusive with `-e`, `-c`, `-x`, and `-s`.

## Non-goals

- Removing entire files (only key removal within files).
- Conditional or regex-based key removal (keys are specified explicitly).
- Undoing removals after they are applied.

## User stories

- As a developer, I want to remove a deprecated `OLD_TOKEN` key from all
  `.env.example` and `configmap*.yml` files across every service, so that
  stale config doesn't confuse new contributors.
- As a platform engineer, I want to remove a rotated secret key from only the
  `auth` service's secrets files, so that the old value is no longer present.
- As a developer, I want to preview exactly what will be removed before
  confirming, so that I don't accidentally delete the wrong keys.

## Behavior & requirements

### Key arguments

- `nv remove` MUST accept one or more positional key arguments. At least one
  key is required.
- Example: `nv remove OLD_TOKEN DEPRECATED_SECRET`

### File type flags

| Flag | Targets | Description |
| --- | --- | --- |
| `-e` | `.env*` files | All dotenv files (`.env`, `.env.example`, `.env.local`, etc.) |
| `-c` | `configmap*.yml` / `.yaml` | Kubernetes configmap files |
| `-x` | `secrets*.yml` / `.yaml` | Kubernetes secrets files |
| `-a` | all of the above | All services, all file types |

- `-e`, `-c`, `-x` MAY be combined (e.g., `-e -c` targets both env and
  configmap files).
- `-a` MUST NOT appear alongside `-e`, `-c`, `-x`, or `-s`. If combined, the
  command MUST exit with an error.
- At least one of `-e`, `-c`, `-x`, `-a`, or `-s` MUST be provided. If none
  are provided, the command MUST exit with an error.

### Service flag (`-s`)

- `-s <NAME>` MAY be combined with `-e`, `-c`, `-x` to restrict removal to a
  single microservice.
- When `-s` is provided without `-e`, `-c`, or `-x`, ALL file types
  (`.env*`, `configmap*`, `secrets*`) are targeted for that service.
- `-s` is optional. When omitted (without `-a`), all services are targeted.
- `-s` MUST NOT appear with `-a`.

### Confirmation flow

- `nv remove` MUST show a diff preview of all changes before writing.
- The command MUST prompt for confirmation unless `--yes` is passed.
- `--dry-run` MUST show the diff without writing or prompting.

### Key removal behavior

- For each target file, the line defining the key MUST be removed entirely.
- For dotenv files: the `KEY=value` line (including any `export ` prefix) is
  removed.
- For YAML files: the `key: value` line is removed.
- If the key does not exist in a file, that file is silently skipped.
- Comments and blank lines surrounding the removed line MUST be preserved.
- If no files contain any of the specified keys, the command MUST report
  "Nothing to remove."

### CLI surface

```
nv remove <KEY>... [OPTIONS]

Options:
  -e              Target env files (.env*)
  -c              Target configmap files
  -x              Target secrets files
  -a              Target all services and file types
  -s <NAME>       Restrict to a single microservice
  --yes           Skip confirmation prompt
  --dry-run       Show diff without writing
```

Global flags also apply: `--no-config`, `--root`.

## Acceptance criteria

- [ ] Given `nv remove OLD_TOKEN -e --yes`, when confirmed, `OLD_TOKEN` is
      removed from all `.env*` files across all services.
- [ ] Given `nv remove OLD_TOKEN -e -s auth --yes`, when confirmed, `OLD_TOKEN`
      is removed only from the `auth` service's `.env*` files.
- [ ] Given `nv remove OLD_TOKEN -e -c --yes`, when confirmed, `OLD_TOKEN` is
      removed from both `.env*` and `configmap*` files.
- [ ] Given `nv remove OLD_TOKEN -a --yes`, when confirmed, `OLD_TOKEN` is
      removed from every file in every service.
- [ ] Given `nv remove OLD_TOKEN -s auth --yes`, when confirmed, `OLD_TOKEN` is
      removed from all file types (`.env*`, `configmap*`, `secrets*`) in the
      `auth` service.
- [ ] Given `nv remove OLD_TOKEN -a -e`, the command exits with an error
      (mutually exclusive flags).
- [ ] Given `nv remove OLD_TOKEN -a -s auth`, the command exits with an error.
- [ ] Given `nv remove OLD_TOKEN` (no flags), the command exits with an error.
- [ ] Given `nv remove NONEXISTENT_KEY -e`, the command reports "Nothing to
      remove."
- [ ] Given `nv remove OLD_TOKEN -e` (no `--yes`), a diff is shown and the
      user is prompted to confirm.
- [ ] Given `--dry-run`, the diff is shown but no files are written.
- [ ] After removal, comments and unrelated lines in each file are
      byte-identical to before.

## Edge cases

- Removing a key that appears multiple times in the same file (e.g., under
  different YAML blocks): all occurrences MUST be removed.
- Empty files after removal: the file is left with just a trailing newline.
- Files that cannot be read (permissions, missing) are silently skipped.
- No keys specified: the command exits with an error.

## Open questions

- Should there be a `--force` flag to skip confirmation without `--yes`?
- Should removed keys be logged somewhere for audit purposes?
