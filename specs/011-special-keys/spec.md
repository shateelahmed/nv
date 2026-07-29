# Spec: `special_secret_keys` for `nv leaks` and `nv fake-secrets`

- **ID:** 011-special-keys
- **Status:** Implemented
- **Author:** shateel
- **Date:** 2026-07-29

## Summary

Add a `special_secret_keys` configuration option that lets users specify
additional key names (beyond the built-in regex patterns) that should be treated
as secret keys by both `nv leaks` and `nv fake-secrets`.

## Problem / Motivation

The built-in `nv leaks` regex detects keys ending with `_KEY`, `_PASSWORD`,
`_SECRET`, `_TOKEN`, `_ID`, or `_USERNAME`. The built-in `nv fake-secrets` regex
recognizes the same naming pattern to decide which keys are "real" secrets. Some
projects use naming conventions that don't match these suffixes — for example
`MYAPP_CREDENTIALS`, `SOME_SALT`, `ENCRYPTION_CERT`, or keys that use lowercase
in the body like `myapp_shared_secret`.

Without `special_secret_keys`, `nv leaks` misses these keys entirely, and
`nv fake-secrets` wrongly reports them as non-secret keys (or placeholder
values in configmaps).

## Goals

- Allow users to list additional key names as secret keys via `nv.yml`, both
  globally and per-service.
- For `nv leaks`: `special_secret_keys` are detected as leaks (same as
  regex-matched keys) and support `--clean`, `--false-alarm`, etc.
- For `nv fake-secrets`: `special_secret_keys` are treated as legitimate secret
  keys and excluded from fake-secret detection.
- The built-in regex patterns remain unchanged.

## Non-goals

- Adding a command-line flag (config file only).
- Replacing or modifying the built-in regex patterns.
- Detecting by value pattern (only by exact key name match).

## User stories

- As a developer, I want `MYAPP_CREDENTIALS` to be flagged as a leak in example
  files by `nv leaks`, even though it doesn't end with a standard suffix.
- As a developer, I want `MYAPP_CREDENTIALS` to be skipped by `nv fake-secrets`
  so it's treated as a legitimate secret key.
- As a platform engineer, I want to define a global set of special secret keys
  for all services and per-service overrides for services with custom naming
  conventions.

## Behavior & requirements

### Configuration

- `special_secret_keys` MUST be configurable globally under
  `commands.leaks.special_secret_keys` (applies to all services).
- `special_secret_keys` MUST be configurable per-service under
  `services.<name>.commands.leaks.special_secret_keys` (applies only to that
  service).
- Global and per-service lists MUST be merged (deduplicated) when checking a
  service.

  **Global configuration:**
  ```yaml
  commands:
    leaks:
      special_secret_keys:
        - MYAPP_CREDENTIALS
        - SOME_SALT
  ```

  **Per-service configuration:**
  ```yaml
  services:
    auth:
      commands:
        leaks:
          special_secret_keys:
            - CUSTOM_AUTH_SECRET
  ```

### `nv leaks` behavior

- For each file scanned, `nv leaks` MUST check whether any key in the merged
  `special_secret_keys` list appears in the file content.
- Matched keys MUST be reported in the same format as regex-matched leaks,
  including value display, `--clean`, `--false-alarm`, and tree output.
- If a key matches both the built-in regex AND `special_secret_keys`, it MUST
  appear only once (no duplicates).
- False alarms MUST still take precedence over `special_secret_keys`.

### `nv fake-secrets` behavior

- For each file scanned, `nv fake-secrets` MUST treat keys in the merged
  `special_secret_keys` list as legitimate secret keys.
- A key in `special_secret_keys` MUST NOT be reported as a fake secret, even if
  it does not match the built-in `secret_key_pattern`.
- This applies to both configmap detection (placeholder values in non-secret
  keys) and secrets-file detection (non-secret keys in secrets files).

### CLI surface

No new CLI flags. Configuration is via `nv.yml` only.

## Acceptance criteria

- [ ] Given `commands.leaks.special_secret_keys: ["MYAPP_CREDENTIALS"]` and a
      service's `.env.example` containing `MYAPP_CREDENTIALS=supersecret`, when
      `nv leaks` is run, then `MYAPP_CREDENTIALS` is listed with value
      `supersecret`.
- [ ] Given a key that matches both the built-in regex and
      `special_secret_keys`, when `nv leaks` is run, then the key appears only
      once.
- [ ] Given `special_secret_keys: ["MYAPP_CREDENTIALS"]` in a configmap with
      `MYAPP_CREDENTIALS=changeme`, when `nv fake-secrets` is run, then
      `MYAPP_CREDENTIALS` is NOT listed (it's treated as a real secret key).
- [ ] Given `special_secret_keys: ["NON_SECRET_KEY"]` in a secrets file
      containing `NON_SECRET_KEY=value`, when `nv fake-secrets` is run, then
      `NON_SECRET_KEY` is NOT listed.
- [ ] Given `false_alarms` also includes a key in `special_secret_keys`, when
      `nv leaks` is run, the key is skipped.

## Edge cases

- Special secret key names with regex-special characters are matched as literal
  strings.
- The same key in global and per-service lists is not duplicated.
- If a special secret key does not appear in any scanned file, it is silently
  ignored.
- Empty values for special secret keys are skipped by `nv leaks` (same as
  regex) and handled normally by `nv fake-secrets`.
