# Spec: `nv leaks` command

- **ID:** 003-leaks-command
- **Status:** Implemented
- **Author:** shateel
- **Date:** 2026-07-24

## Summary

A `nv leaks` command that scans example and configmap files for keys that look
like hardcoded secrets (passwords, API keys, tokens), reports them, and
optionally cleans them up or marks them as false alarms.

## Problem / Motivation

Example files (`.env.example`, `.env.testing.example`) and configmap templates
often contain placeholder or accidentally committed secret values. There is no
built-in way to audit these files for potential leaks without manually inspecting
each one.

## Goals

- Detect keys whose names suggest secrets (`*_KEY`, `*_PASSWORD`, `*_SECRET`,
  `*_TOKEN`, `*_ID`, `*_USERNAME`) in example and configmap files.
- Show both the key name and its value so the user can assess the risk.
- Support both dotenv (`KEY=value`) and YAML (`KEY: value`) formats.
- Allow filtering by service using `--service`.
- Provide a `--clean` mode that removes keys from configmaps and sets empty
  values in example files, with a preview/confirm step.
- Allow marking specific keys as false alarms via `--false-alarm`, which saves
  them to `nv.yml` so they are skipped on future runs.

## Non-goals

- Modifying or fixing detected leaks outside of `--clean` mode.
- Scanning `.env` or `secrets*.yml` files (those are expected to hold real
  values).
- Detecting secrets by value pattern (only by key name).

## User stories

- As a developer, I want to quickly see if any example files contain real secret
  values before committing, so that I don't accidentally leak credentials.
- As a platform engineer, I want to audit configmap templates for hardcoded
  secrets across all services in one command.
- As a developer, I want to clean up detected leaks in one step so that I don't
  have to manually edit each file.
- As a developer, I want to mark a detected key as a false alarm so that it
  doesn't show up in future scans.

## Behavior & requirements

### Scanning (default mode)

- `nv leaks` MUST scan only files of kind `DotenvExample` and `ConfigMap`.
- `nv leaks` MUST display a spinner (with `indicatif`) during the scan showing
  the current progress.
- After scanning, `nv leaks` MUST print a summary line: "Scanned N files, M
  folders."
- `nv leaks` MUST match keys using the regex pattern:
  ```
  (?m)^\s*(?:export\s+)?([A-Za-z0-9_][A-Z0-9_]+_(?:KEY|PASSWORD|SECRET|TOKEN|ID|USERNAME))[^\S\n]*(?::\s*(.+)|=[^\S\n]*(\S.*))$
  ```
- The regex MUST handle both dotenv syntax (`KEY=value`, `KEY= value`,
  `export KEY=value`) and YAML syntax (`KEY: value`).
- Empty values (e.g., `KEY=` or `KEY:`) MUST be skipped.
- Results MUST be grouped by service, then by file, in the same hierarchical
  colorized format as `nv find`.
- Keys marked as false alarms in `nv.yml` MUST be skipped.

### Service filtering

- The `--service` / `-s` global flag MUST filter the scan to only the named
  service(s).

### Cleanup (`--clean`)

- `nv leaks --clean` MUST remove detected keys entirely from `ConfigMap` files.
- `nv leaks --clean` MUST set detected keys to empty values in `DotenvExample`
  files.
- `nv leaks --clean` MUST show a diff preview and prompt for confirmation
  before writing (using the standard `preview_and_apply` flow).
- `--dry-run` MUST show the diff without writing.
- `--yes` MUST skip the confirmation prompt.

### False alarms (`--false-alarm`)

- `nv leaks --false-alarm <KEY>` MUST mark the named key as a false alarm for
  every matching leak across all scanned files.
- False alarms are stored in `nv.yml` under a `false_alarms` top-level key:
  ```yaml
  false_alarms:
    <service-name>:
      - KEY_NAME
  ```
- On subsequent runs, keys listed in `false_alarms` MUST be silently skipped.
- If the key is not found in any leak, the command MUST exit with an error.

### CLI surface

```
nv leaks [OPTIONS]

Options:
  --clean                   Remove/set-empty detected keys (with preview)
  --false-alarm <KEY>       Mark a key as a false alarm (saved to nv.yml)
```

Uses global flags: `--service`, `--file`, `--no-config`, `--root`, `--all`.

## Acceptance criteria

- [ ] Given a service with `.env.example` containing `DB_PASSWORD=secret123`,
      when `nv leaks` is run, then `DB_PASSWORD` is listed with value
      `secret123`.
- [ ] Given a service with `configmap.yml` containing `API_KEY: sk-live`,
      when `nv leaks` is run, then `API_KEY` is listed with value `sk-live`.
- [ ] Given a service with `.env.example` containing `DB_PASSWORD=` (empty),
      when `nv leaks` is run, then `DB_PASSWORD` is NOT listed.
- [ ] Given a service with `.env.example` containing `LOG_LEVEL=debug`, when
      `nv leaks` is run, then `LOG_LEVEL` is NOT listed.
- [ ] Given no matches, when `nv leaks` is run, then "No potential leaks
      found." is printed.
- [ ] Given `--service auth`, when `nv leaks` is run, only the `auth` service
      is scanned.
- [ ] Given `--clean`, when confirmed, the key is removed from configmaps and
      set to empty in example files.
- [ ] Given `--false-alarm DB_PASSWORD`, the key is saved to `nv.yml` and
      skipped on the next run.

## Edge cases

- Files that cannot be read (permissions, missing) are silently skipped.
- YAML values that are multi-line or complex objects are not fully captured —
  only the first line is matched.
- Keys with both `export` prefix and dotenv/YAML syntax are handled.
- `--false-alarm` with a key not found in any leak produces an error.
- `--clean` with no matching leaks prints "Nothing to change."

## Open questions

- Should `nv leaks` also scan regular `.env` files (not just examples)?
- Should there be a `--format` flag for JSON/machine-readable output?
