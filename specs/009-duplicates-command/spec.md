# Spec: `nv duplicates` command

- **ID:** 009-duplicates-command
- **Status:** Proposed
- **Author:** shateel
- **Date:** 2026-07-27

## Summary

A `nv duplicates` command that scans env and env example files for keys that
appear multiple times within a single file. Duplicate keys can cause confusion,
as only one value will be used (depending on the parser or runtime), and may
indicate copy-paste errors or incomplete refactoring.

## Problem / Motivation

When managing environment variables across microservices, it's easy to
accidentally define the same key multiple times in `.env` or `.env.example`
files. This can happen due to:

- Copy-paste errors when duplicating configuration
- Merging branches that added the same key
- Refactoring that left old definitions in place

Duplicate keys are problematic because:
- Only one value will typically be used (the last one in dotenv files, or
  the first one in some YAML parsers)
- It's unclear which value is "correct"
- It adds noise and confusion during audits

Currently there is no built-in way to identify duplicate keys within env files.

## Goals

- Detect keys that appear more than once within a single env file (`.env`,
  `.env.example`), configmap file, or secrets file.
- Show the key name, the file where it appears, and the values (if different).
- Support filtering by service using `-s`.
- Display results in the same hierarchical colorized format as other commands.

## Non-goals

- Detecting duplicate keys in configmap or secret files (those are handled
  by other commands).
- Automatically merging or deduplicating keys (use `nv remove` for cleanup).
- Detecting keys with the same value across different services or files
  (cross-file deduplication is out of scope).
- Providing cleanup or removal of duplicate keys (this command only reports).

## User stories

- As a developer, I want to quickly see if any env files contain duplicate keys
  so that I can clean up my configuration.
- As a platform engineer, I want to audit env files across all services for
  duplicate definitions so that I can ensure consistency.
- As a developer, I want to see which files contain duplicate keys and what
  values they have so that I can determine which one to keep.

## Behavior & requirements

### Scanning (default mode)

- `nv duplicates` MUST scan files of kind `Dotenv`, `DotenvExample`, `ConfigMap`,
  and `Secret`.
- `nv duplicates` MUST display a spinner (with `indicatif`) during the scan
  showing the current progress.
- After scanning, `nv duplicates` MUST print a summary line: "Scanned N files,
  M folders."
- For each service, `nv duplicates` MUST detect:
  - Keys that appear more than once within a single file (intra-file duplicates).
- The detection MUST be case-sensitive (e.g., `DB_HOST` and `db_host` are
  different keys).
- Empty values (e.g., `KEY=` or `KEY:`) MUST still be considered for duplicate
  detection.
- Results MUST be grouped by service, then by file, in the same hierarchical
  colorized format as `nv find` and `nv leaks`.
- Each duplicate key entry MUST show:
  - The key name
  - The file where it appears (with full path relative to service root)
  - The values in each occurrence (to help identify which to keep)

### Service filtering

- The `-s` flag MUST filter the scan to only the named service(s).

### CLI surface

```
nv duplicates [OPTIONS]

Options:
  -s <SERVICE>    The service name to scan (repeatable)
```

Uses global flags: `--no-config`, `--root`, `--all`.

## Acceptance criteria

- [ ] Given a service with `.env` containing `DB_HOST=localhost` and
      `DB_HOST=127.0.0.1` (two occurrences), when `nv duplicates` is run, then
      `DB_HOST` is listed with both values and their locations.
- [ ] Given a service with `.env` containing `DB_HOST=localhost` and
      `.env.testing` containing `DB_HOST=staging`, when `nv duplicates` is
      run, then `DB_HOST` is NOT listed (each file is scanned independently).
- [ ] Given a service with `.env` containing `DB_HOST=localhost` (single
      occurrence), when `nv duplicates` is run, then `DB_HOST` is NOT listed.
- [ ] Given a service with `.env` containing `DB_HOST=localhost` and
      `db_host=127.0.0.1` (different case), when `nv duplicates` is run, then
      neither is listed (case-sensitive).
- [ ] Given no duplicates, when `nv duplicates` is run, then "No duplicate
      keys found." is printed.
- [ ] Given `-s auth`, when `nv duplicates` is run, only the `auth` service
      is scanned.

## Edge cases

- Files that cannot be read (permissions, missing) are silently skipped.
- Keys with `export` prefix are treated the same as keys without it (the
  prefix is stripped for comparison).
- Empty values (e.g., `KEY=`) are still considered for duplicate detection.
- Very large files with many duplicates are handled without excessive memory
  usage.
