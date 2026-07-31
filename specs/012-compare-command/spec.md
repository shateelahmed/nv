# Spec: `nv compare` command

- **ID:** 012-compare-command
- **Status:** Implemented
- **Author:** shateel
- **Date:** 2026-07-30
- **Updated:** 2026-07-31

## Summary

A `nv compare <file-path>` command that compares an env file specified by the
user against all other files of the same kind, showing key-level and value-level
differences in the standard tree output format.

## Problem / Motivation

Users often need to know whether a ``.env`` file is in sync with other ``.env``
and ``.env.example`` files across services — what keys are missing, what keys
are extra, and (optionally) whether values differ. Without a dedicated compare
command, this requires manual inspection or external diff tools that don't
understand the project's service structure.

## Goals

- Compare a user-specified base file against all other files of the same kind.
- Show missing keys (present in base, absent in other) with `-` prefix in red.
- Show extra keys (absent in base, present in other) with `+` prefix in green.
- Support value comparison via a `--values` flag.
- Output follows the same hierarchical tree format as `nv find` and `nv leaks`.

## Non-goals

- Writing or modifying files (read-only comparison).
- Comparing files across different kinds (e.g., `.env` vs `configmap`).

## User stories

- As a developer, I want to see which keys in a `.env.example` are missing
  from another service's `.env.example`, so I can keep templates in sync.
- As a platform engineer, I want to verify that all configmaps have the same
  set of keys with a single command.

## Behavior & requirements

### File-kind matching

The command MUST determine the kind of the base file and compare it only
against files of matching kinds:

| Base kind | Compared against |
| --- | --- |
| `Dotenv` | `Dotenv`, `DotenvExample` |
| `DotenvExample` | `Dotenv`, `DotenvExample` |
| `ConfigMap` | `ConfigMap` |
| `Secret` | `Secret` |

### Key-only comparison (default)

- Keys present in the base file but absent in the other file MUST be shown
  with a `- ` prefix and colored `removed` (red).
- Keys absent in the base file but present in the other file MUST be shown
  with a `+ ` prefix and colored `added` (green).
- Keys present in both files MUST be omitted.

### Value comparison (`--values`)

- With `--values`, keys present in both files but with different values MUST
  be shown as two lines: a `- KEY = base_value` in `removed` color and a
  `+ KEY = other_value` in `added` color.
- Keys with identical values MUST be omitted.
- Missing/extra key display is the same as key-only mode, but with the value
  appended (`- KEY = value`).

### CLI surface

```
nv compare [OPTIONS] <FILE_PATH>

Arguments:
  FILE_PATH  Path to the base file, relative to the services root.

Options:
  --values   Also compare values for keys present in both files.

Uses global flags: --service (to disambiguate when the same path exists in
multiple services), --no-config, --root.
```

If `--service` is provided, only that service is searched for the base file.
If the path is found in multiple services without `--service`, the command
MUST error with a disambiguation message.

### Output format

Results MUST be grouped by service, then by compared file, using the standard
hierarchical tree format:

```
service_name/
├── compared_file_a.env
│   ├── - MISSING_KEY
│   └── + EXTRA_KEY
└── compared_file_b.env
    ├── - ANOTHER_MISSING_KEY = base_value    (with --values)
    └── + ANOTHER_EXTRA_KEY = other_value
```

Tree branches (`├──`, `└──`, `│`) use the same coloring rules as other
commands (service color for service lines, file color for file lines).
Individual item lines use `added`/`removed` colors (green/red) for the
`+`/`-` prefixes.

### Configuration

Files to exclude from comparison can be configured globally or per-service in
`nv.yml`.

**Global configuration** (applies to all services):
```yaml
commands:
  compare:
    skip_files:
      - docker/.env
      - "*.example.env"
```

**Per-service configuration** (applies only to that service):
```yaml
services:
  auth:
    path: services/auth
    commands:
      compare:
        skip_files:
          - .env.testing.example
          - docker/**/*.env*
```

- Global and per-service `skip_files` MUST be merged when comparing within a
  service.
- `skip_files` entries support glob patterns:
  - `*` matches any characters except `/` (within a single path segment)
  - `**` matches any characters including `/` (recursive across directories)
  - `?` matches a single character
- `skip_files` entries are matched against the relative path from the service
  root directory (e.g., `docker/.env`, `docker/app/.env.example`).
- `skip_files` entries can also match just the file name (e.g., `custom.env`).
- If the requested base file matches a `skip_files` pattern, the command
  errors explicitly instead of comparing (never silently ignored) — only peer
  (compared) files are silently filtered.
- `skip_files` has no built-in defaults.
- Files matching a service's merged `skip_files` set MUST also be omitted from
  the `Available ... files:` error listings (they cannot be compared, so they
  are not "available").

**Requested base file is excluded:** If the file the user asks to compare is
itself listed under `skip_files`, the command MUST error with
`file '<path>' is excluded by compare.skip_files.` followed by an
`Available <kind> files:` tree listing only files of the same kind as the
requested file (with the requested file itself omitted), so the user can pick
an alternative. The `<kind>` label is `env` for dotenv/dotenv_example files,
`configmap` for configmap files, and `secrets` for secret files.

### File not found

When the base file path is not found in any service, the command MUST print the
error message `file '<path>' not found.` first, then the header
`Available files:`, then a listing of the available files in the uniform
hierarchical tree format (grouped by service) so the user can pick a valid
path. File lines in this listing MUST show bare file names without item counts.
When `--service` is provided, the tree MUST only include files from the
specified service(s). When the tree is empty, the `Available files:` header is
omitted.

## Acceptance criteria

- [ ] Given a `.env.example` with keys `A`, `B` and another `.env.example`
      with keys `A`, `C`, when `nv compare` runs with `--values` against
      the first file, the output shows `- B` and `+ C` under the second file.
- [ ] Given a `configmap.yml` with keys `X`, `Y` and no other configmap
      files, when `nv compare` runs, "No comparisons" is printed.
- [ ] Given `--values` and two files with key `K` set to `v1` and `v2`,
      the output shows `- K = v1` and `+ K = v2`.
- [ ] Given an invalid or non-existent file path, the command errors with a
      clear message (`file '<path>' not found.`) printed first, followed by
      `Available files:` and the file tree in the uniform tree format with bare
      file names (no counts).
- [ ] Given an invalid path with `--service auth`, the available files tree
      lists only `auth`'s files.
- [ ] Given a service `auth` with `compare.skip_files: [.env.testing.example]`
      and a key present only in `auth/.env.testing.example`, when `nv compare`
      runs against another file, the `.env.testing.example` file is NOT shown
      as a compared file.
- [ ] Given global `compare.skip_files: [docker/.env]`, when `nv compare` runs,
      `docker/.env` files are excluded from comparison in every service.
- [ ] Given `compare.skip_files: [custom.env]` where `custom.env` is a
      sub-path (e.g., `docker/custom.env`), when `nv compare` runs, the file
      is excluded by its file name alone.
- [ ] Given the requested file is listed under `compare.skip_files`, the
      command errors with `file '<path>' is excluded by compare.skip_files.`
      and lists available files of the same kind (excluding the requested
      file), with a header naming the kind (`Available env files:`,
      `Available configmap files:`, `Available secrets files:`).
- [ ] Given a service with `compare.skip_files: [docker/.env]`, when the
      available-files tree is shown (file not found or base excluded), the
      `docker/.env` file is NOT listed.

## Edge cases

- Base file not found in any service → error `file '<path>' not found.`
  printed first, then `Available files:` and the available-files tree (uniform
  output format, bare file names without counts); with `--service`, the tree
  contains only the specified service(s); an empty tree omits the header.
- Base file path matches multiple services without `--service` → error with
  disambiguation hint.
- No other files of the same kind → "No comparisons found."
- Binary or unreadable compared files → silently skipped.
- Files matching a `compare.skip_files` pattern → excluded from comparison
  and omitted from the `Available ... files:` error listings.
- Requested base file itself listed under `compare.skip_files` → error
  `file '<path>' is excluded by compare.skip_files.` plus an
  `Available <kind> files:` tree of same-kind files (requested file omitted),
  where `<kind>` is `env`, `configmap`, or `secrets`.
- Dotenv vs DotenvExample: both directions are compared, but only files
  resolvable through service discovery are included.
