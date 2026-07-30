# Spec: `nv compare` command

- **ID:** 012-compare-command
- **Status:** Implemented
- **Author:** shateel
- **Date:** 2026-07-30

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

## Acceptance criteria

- [ ] Given a `.env.example` with keys `A`, `B` and another `.env.example`
      with keys `A`, `C`, when `nv compare` runs with `--values` against
      the first file, the output shows `- B` and `+ C` under the second file.
- [ ] Given a `configmap.yml` with keys `X`, `Y` and no other configmap
      files, when `nv compare` runs, "No comparisons" is printed.
- [ ] Given `--values` and two files with key `K` set to `v1` and `v2`,
      the output shows `- K = v1` and `+ K = v2`.
- [ ] Given an invalid or non-existent file path, the command errors with a
      clear message.

## Edge cases

- Base file not found in any service → error.
- Base file path matches multiple services without `--service` → error with
  disambiguation hint.
- No other files of the same kind → "No comparisons found."
- Binary or unreadable compared files → silently skipped.
- Dotenv vs DotenvExample: both directions are compared, but only files
  resolvable through service discovery are included.
