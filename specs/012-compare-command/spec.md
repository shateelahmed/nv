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
- Support key-order comparison via an `--order` flag.
- Support comment comparison via a `--comments` flag.
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

### Key-order comparison (`--order`)

- With `--order`, keys present in BOTH files are checked to appear in the same
  relative order in the peer file as in the base file.
- Only keys present in both files participate; keys missing from either file are
  handled by the key-only comparison instead.
- A key is reported when it appears "too early" in the peer relative to the
  base ordering: walking the peer in file order, a key is out of order when its
  position in the base file is smaller than the largest base position already
  seen among keys reported so far in the peer walk.
- Each reported key is shown as two lines: a `- KEY (#base_position)` in
  `removed` color and a `+ KEY (#peer_position)` in `added` color, where
  `#` positions are 1-based.
- Duplicate keys use their first occurrence in both files.
- Keys with identical order produce no output.
- `--order` is mutually exclusive with `--values` (clap `conflicts_with`);
  passing both errors with `the argument '--values' cannot be used with
  '--order'`.

### Comment comparison (`--comments`)

- With `--comments`, comments are compared in two passes shown together:
  per-key attached comments first, then every other comment.
- **Per-key pass (first).** Only keys present in BOTH files participate. A
  key's attached comment is the consecutive prose `#` lines directly above it
  plus the inline `# comment` on its own line, normalized and joined with
  single spaces. When the attached comment differs (a key documented on one
  side only counts as a difference), the key is shown as a
  `- KEY # base_comment` in `removed` color and a `+ KEY # peer_comment` in
  `added` color; a side without a comment renders as bare `- KEY` / `+ KEY`.
  Per-key items are ordered by key name.
- **Other-comments pass (second).** Every comment in the files is collected as
  normalized text — full-line `#` comments (section headers, prose, and
  commented-out assignments such as `# REDIS_ENTERPRISE_HOST=redis-enterprise`)
  and inline `# comment` text. The comment lines consumed by the per-key pass
  (attached to a key present in both files) are removed from each file's pool,
  and the remaining pools are diffed as multisets: comments only in the base
  pool are shown with a `-` prefix in `removed` color, comments only in the
  peer pool with a `+` prefix in `added` color, each rendered as
  `- # comment` / `+ # comment`.
- Comment text is normalized for comparison by stripping indentation, leading
  `#` markers, and surrounding whitespace; a `#` inside single or double
  quotes is not treated as a comment. Prose block lines and an inline comment
  attached to the same key compare equal when their text matches (`# DB` above
  a key plus `# prod` inline equals a single `DB prod` comment).
- A comment line that parses as a commented-out assignment (`KEY=value` in
  dotenv, `KEY: value` in YAML) never attaches to a key and breaks any comment
  block above it; such lines are always compared in the other-comments pass.
- Comments attached to a key present in only one file are not per-key
  comparable; they flow into the other-comments pass instead of being dropped.
- Repeated comments are counted: a comment appearing more often in one pool
  yields one item per excess occurrence. Position/order do not matter within
  the other-comments pass.
- `--comments` is mutually exclusive with `--values` and `--order` (clap
  `conflicts_with_all`); passing `--comments` with either errors with
  `the argument '--comments' cannot be used with '--values'` /
  `'--order'`.

### CLI surface

```
nv compare [OPTIONS] <FILE_PATH>

Arguments:
  FILE_PATH  Path to the base file, relative to the services root.

Options:
  --values   Also compare values for keys present in both files.
  --order    Also check that keys present in both files appear in the same order.
             Cannot be combined with --values.
  --comments Compare comments: each key's attached comment, then every other
             comment in the files.
             Cannot be combined with --values or --order.

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
    ├── + ANOTHER_EXTRA_KEY = other_value
    ├── - OUT_OF_ORDER_KEY (#base_pos)        (with --order)
    ├── + OUT_OF_ORDER_KEY (#peer_pos)
    ├── - KEY # base_comment                   (with --comments, per-key)
    ├── + KEY # peer_comment
    ├── - # MISSING_COMMENT                    (with --comments, other comments)
    └── + # EXTRA_COMMENT
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
- [ ] Given a base file with keys `A`, `B`, `C` and a peer file with keys
      `A`, `C`, `B` (all values identical), when `nv compare` runs with
      `--order` against the base file, the output shows `- B (#2)` and
      `+ B (#3)` under the peer file.
- [ ] Given the same files as above, when `nv compare` runs WITHOUT `--order`,
      "No comparisons found." is printed (order is not checked by default).
- [ ] Given a peer file with identical keys and values in the same order, when
      `nv compare` runs with `--order`, no order lines are shown.
- [ ] Given a key that is present in both files but has both a different value
      and a different order, passing `--values --order` together errors with
      `the argument '--values' cannot be used with '--order'`.
- [ ] Given a base `.env` with commented-out keys `# REDIS_ENTERPRISE_HOST=
      redis-enterprise`, `# REDIS_ENTERPRISE_PORT=12000`,
      `# REDIS_ENTERPRISE_PASSWORD=secret` and a peer `.env` without them, when
      `nv compare` runs with `--comments`, the output shows the three comments
      with `- # ` prefixes under the peer file — even when those lines sit
      directly above a real key (commented-out assignments never attach).
- [ ] Given a base file with `FOO=bar # production` and a peer file with
      `FOO=bar # development`, when `nv compare` runs with `--comments`, the
      output shows `- FOO # production` and `+ FOO # development` (the inline
      comments attach to `FOO`, which is present in both files).
- [ ] Given a base file with `# DB connection` directly above `DATABASE_URL=x`
      and a peer file with `DATABASE_URL=x` (no comment), when `nv compare`
      runs with `--comments`, the output shows `- DATABASE_URL # DB connection`
      and `+ DATABASE_URL`, and the comment is NOT repeated as a `- #` item.
- [ ] Given a base file with a blank-line-separated `# Header A` and a peer
      file with `# Header B`, when `nv compare` runs with `--comments`, the
      output shows `- # Header A` and `+ # Header B` (free comments are
      compared as a multiset).
- [ ] Given a key whose comment changed and a free comment removed, when
      `nv compare` runs with `--comments`, the per-key pair appears before the
      free-comment items in the output.
- [ ] Given a comment attached to a key present only in the base file, when
      `nv compare` runs with `--comments`, the comment is reported as a
      `- # comment` free-comment item rather than being dropped.
- [ ] Given two files with identical comments in a different order or
      position, when `nv compare` runs with `--comments`, no difference is
      reported (free comments are compared as a set).
- [ ] Given a comment repeated twice in the base file and once in the peer
      file, when `nv compare` runs with `--comments`, one `- # comment` item is
      reported.
- [ ] Given `--comments` combined with either `--values` or `--order`, the
      command errors with `the argument '--comments' cannot be used with
      '--values'` / `'--order'`.

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
- `--order` only reports keys present in both files; keys missing from either
  file are left to the key-only comparison.
- `--order` uses the first occurrence of duplicate keys; later duplicates are
  ignored for ordering.
- `--order` reports a key only when it appears before a key that precedes it in
  the base ordering (i.e., it regresses past an already-seen base position);
  keys that merely appear later never produce output.
- `--values` and `--order` are mutually exclusive; combining them is a clap
  parse error.
- `--comments` is mutually exclusive with `--values` and `--order`; combining
  them is a clap parse error.
- `--comments` compares comments in two passes: per-key attached comments
  first (only keys present in both files, sorted by key), then every other
  comment as a multiset. The attached comment lines of keys present in both
  files are removed from the multiset pools so nothing is reported twice.
- `--comments` normalizes comment text (leading `#`/whitespace stripped,
  quoted `#` ignored); comments differing only in indentation or `#`-spacing
  compare equal, and a block plus an inline comment attached to the same key
  compare equal to the joined text.
- A comment line that parses as a commented-out assignment (`KEY=value` in
  dotenv, `KEY: value` in YAML) never attaches to a key and breaks any comment
  block above it; such lines are compared only in the other-comments pass.
  YAML prose containing a colon (e.g. `# Note: this is a comment`) is
  structurally indistinguishable and is treated the same way.
- Comments attached to a key present in only one file cannot be compared
  per-key; they flow into the other-comments pass instead of being dropped.
- `--comments` compares free comments as a multiset; a comment repeated more
  often in one pool yields one item per excess occurrence, and
  position/order is ignored.
