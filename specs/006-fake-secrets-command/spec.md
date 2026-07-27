# Spec: `nv fake-secrets` command

- **ID:** 006-fake-secrets-command
- **Status:** Proposed
- **Author:** shateel
- **Date:** 2026-07-26

## Summary

A `nv fake-secrets` command that scans configmap and secrets files for keys that
are likely placeholder or fake secrets (not real secrets). This complements
`nv leaks` which finds real secrets in example files — `fake-secrets` finds
misplaced or dummy secrets in production-like files.

## Problem / Motivation

Configmap and secrets files sometimes contain placeholder values (like `xxx...`)
or keys that don't follow the naming convention for real secrets. These "fake
secrets" can cause confusion during audits and make it harder to identify actual
secret values that need protection.

There are two categories of fake secrets:
1. **Placeholder values in configmaps**: Keys with values like `xxx`,
   `changeme`, `placeholder`, etc. These are typically dummy values that should
   be replaced with real secrets or removed.
2. **Non-secret keys in secrets files**: Keys present in `secrets*.yml` or
   `secrets*.yml.example` that don't match the secret key naming pattern
   (`*_KEY`, `*_PASSWORD`, `*_SECRET`, `*_TOKEN`, `*_ID`, `*_USERNAME`). These
   may be misfiled and should be in configmaps instead.

## Goals

- Detect keys with placeholder values (`xxx`, `changeme`, `placeholder`, etc.)
  in configmap files.
- Detect keys in secrets files that don't match the secret key naming pattern
  (`*_KEY`, `*_PASSWORD`, `*_SECRET`, `*_TOKEN`, `*_ID`, `*_USERNAME`).
- Show both the key name and its value so the user can assess the issue.
- Support both dotenv (`KEY=value`) and YAML (`KEY: value`) formats.
- Allow filtering by service using `--service`.
- Allow marking specific keys as false alarms via `--false-alarm`, which saves
  them to `nv.yml` so they are skipped on future runs.

## Non-goals

- Modifying or fixing detected fake secrets (use `nv remove` or `nv leaks`
  for cleanup).
- Scanning `.env` or `.env.example` files (those are handled by `nv leaks`).
- Detecting fake secrets by key name pattern only (we check both name and
  value).
- Providing a `--clean` mode (users can use `nv remove` to clean up detected
  keys).

## User stories

- As a developer, I want to quickly see if any configmaps contain placeholder
  values like `xxx` so that I don't deploy with dummy secrets.
- As a platform engineer, I want to audit secrets files for non-secret keys
  that should be in configmaps instead.
- As a developer, I want to mark a detected key as a false alarm so that it
  doesn't show up in future scans.

## Behavior & requirements

### Scanning (default mode)

- `nv fake-secrets` MUST scan only files of kind `ConfigMap` and `Secret`.
  This includes files like `secrets.yml`, `secrets-db.yaml`,
  `secrets.yml.example`, and `secrets-db.yaml.example`.
- `nv fake-secrets` MUST detect two categories of fake secrets:

  **Category 1: Placeholder values in configmaps**
  - Keys in configmap files with values matching placeholder patterns:
    - `xxx`, `xxxx`, `xxxxx`, etc. (three or more `x` characters)
    - `changeme`, `change_me`, `change-me`
    - `placeholder`
    - `dummy`
    - `test`
    - `example`
    - `fake`
    - `secret` (literal value)
    - Empty or whitespace-only values
  - The detection is case-insensitive.

  **Category 2: Non-secret keys in secrets files**
  - Keys in secrets files (`secrets*.yml` or `secrets*.yml.example`) that do
    NOT match the secret key naming pattern:
    ```
    [A-Za-z0-9_][A-Z0-9_]+_(KEY|PASSWORD|SECRET|TOKEN|ID|USERNAME)
    ```
  - This identifies keys that are likely misfiled and should be in configmaps.
  - For Kubernetes-style secrets files, only keys under the `data` or
    `stringData` sections are considered. Keys in `metadata`, `spec`, or other
    sections are ignored.

- The regex MUST handle both dotenv syntax (`KEY=value`, `KEY= value`,
  `export KEY=value`) and YAML syntax (`KEY: value`).
- Results MUST be grouped by service, then by file, in the same hierarchical
  colorized format as `nv find` and `nv leaks`.
- Keys marked as false alarms in `nv.yml` MUST be skipped.

### Service filtering

- The `--service` / `-s` global flag MUST filter the scan to only the named
  service(s).

### False alarms (`--false-alarm`)

- `nv fake-secrets --false-alarm <KEY>` MUST mark the named key as a false
  alarm for every matching fake secret across all scanned files.
- False alarms are stored in `nv.yml` under the `false_alarms` top-level key,
  shared with `nv leaks`:
  ```yaml
  false_alarms:
    <service-name>:
      - KEY_NAME
  ```
- On subsequent runs, keys listed in `false_alarms` MUST be silently skipped.
- If the key is not found in any fake secret, the command MUST exit with an
  error.

### CLI surface

```
nv fake-secrets [OPTIONS]

Options:
  --false-alarm <KEY>       Mark a key as a false alarm (saved to nv.yml)
```

Uses global flags: `--service`, `--file`, `--no-config`, `--root`, `--all`.

## Acceptance criteria

- [ ] Given a service with `configmap.yml` containing `API_KEY: xxx`, when
      `nv fake-secrets` is run, then `API_KEY` is listed with value `xxx`.
- [ ] Given a service with `secrets.yml` containing `LOG_LEVEL: debug`, when
      `nv fake-secrets` is run, then `LOG_LEVEL` is listed (non-secret key in
      secrets file).
- [ ] Given a service with `secrets.yml` containing `DB_PASSWORD: secret123`,
      when `nv fake-secrets` is run, then `DB_PASSWORD` is NOT listed (matches
      secret key pattern).
- [ ] Given a service with `configmap.yml` containing `API_KEY: sk-live`, when
      `nv fake-secrets` is run, then `API_KEY` is NOT listed (real value, not
      placeholder).
- [ ] Given no matches, when `nv fake-secrets` is run, then "No fake secrets
      found." is printed.
- [ ] Given `--service auth`, when `nv fake-secrets` is run, only the `auth`
      service is scanned.
- [ ] Given `--false-alarm API_KEY`, the key is saved to `nv.yml` and skipped
      on the next run.
- [ ] Given `secrets-db.yaml.example` with a non-secret key, when
      `nv fake-secrets` is run, the key is listed.
- [ ] Given a Kubernetes secrets file with `metadata.name: my-secret` and
      `data.LOG_LEVEL: debug`, when `nv fake-secrets` is run, only `LOG_LEVEL`
      is listed (metadata keys are ignored).
- [ ] Given a Kubernetes secrets file with `stringData.CONFIG: value` and
      `data.DB_PASSWORD: secret`, when `nv fake-secrets` is run, only `CONFIG`
      is listed (non-secret key in data/stringData).

## Edge cases

- Files that cannot be read (permissions, missing) are silently skipped.
- YAML values that are multi-line or complex objects are not fully captured —
  only the first line is matched.
- Keys with both `export` prefix and dotenv/YAML syntax are handled.
- `--false-alarm` with a key not found in any fake secret produces an error.
- Keys that match the secret key pattern but have placeholder values in
  configmaps ARE detected (Category 1).
- Keys that don't match the secret key pattern but have real values in secrets
  files ARE detected (Category 2).
- `secrets-*.yaml.example` files are treated as Secret files (same as
  `secrets-*.yaml`).
- Kubernetes secrets files have keys under `data` or `stringData` sections;
  keys in other sections (like `metadata`) are not scanned.

## Open questions

- Should `nv fake-secrets` also scan regular `.env` files for placeholder
  values?
- Should there be a configurable list of placeholder patterns in `nv.yml`?
- Should the placeholder pattern matching be configurable per-service?
