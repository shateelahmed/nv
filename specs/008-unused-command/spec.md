# Spec: `nv unused` command

- **ID:** 008-unused-command
- **Status:** Proposed
- **Author:** shateel
- **Date:** 2026-07-26

## Summary

A `nv unused` command that scans env, env example, configmap, and secret files
for keys that are not used anywhere in the codebase. It performs an exact,
case-sensitive search for each key across all source files (excluding configurable
skip directories).

## Problem / Motivation

When managing environment variables across microservices, it's easy to accumulate
unused keys in `.env`, `.env.example`, `configmap*.yml`, and `secrets*.yml`
files. These unused keys add clutter, cause confusion during audits, and may
mask real configuration issues. Currently there is no built-in way to identify
which keys are actually referenced in the codebase versus which are stale.

## Goals

- Detect keys defined in env, env example, configmap, and secret files that are
  not referenced anywhere in the project source code.
- Perform exact, case-sensitive matching for each key.
- Skip configurable directories (default: `node_modules`, `logs`, `vendor`).
- Support filtering by service using `-s`.
- Provide a `--clean` mode to remove unused keys with preview/confirmation.
- Display results in the same hierarchical colorized format as other commands.

## Non-goals

- Detecting keys by partial or fuzzy matching.
- Providing suggestions for what to do with unused keys.

## User stories

- As a developer, I want to identify unused environment keys so that I can clean
  up stale configuration and reduce clutter.
- As a platform engineer, I want to audit which keys in configmap and secret
  files are actually referenced in the codebase, so I can remove unused
  definitions.
- As a developer, I want to scan a specific service for unused keys so that I
  can focus my cleanup efforts.
- As a developer, I want to automatically remove unused keys with a single
  command so that I don't have to manually edit each file.

## Behavior & requirements

### Scanning (default mode)

- `nv unused` MUST scan files of kind `Dotenv`, `DotenvExample`, `ConfigMap`,
  and `Secret`.
- For each key found in these files, `nv unused` MUST search the entire project
  source tree for exact, case-sensitive occurrences of the key name.
- When multiple keys share a prefix (e.g., `PHP_INI_LOG_ERRORS` and
  `PHP_INI_LOG_ERRORS_MAX_LEN`), the search MUST use longest-match semantics
  so that every key is independently matched and none is masked by a shorter
  prefix key.
- For YAML files, `nv unused` MUST scan all nested keys at any depth (e.g.,
  `data.DB_HOST`, `stringData.API_KEY`, or arbitrarily nested mappings).
- The search MUST skip env-like files (dotenv, dotenv_example, configmap, secret)
  to avoid counting a key as "used" just because it appears in another env file.
- The search MUST skip the following directories by default:
  - `.git`
  - `target`
  - `vendor`
  - `node_modules`
  - `logs`
- Additional skip directories and files can be configured in `nv.yml` (see Configuration).
- A key is considered "unused" if it does NOT appear in any source file outside
  of the definition file itself.
- Results MUST be grouped by service, then by file, in the same hierarchical
  colorized format as `nv find` and `nv leaks`.
- After scanning, the command MUST print a summary line showing the number of
  files scanned, folders scanned, and keys found (e.g., "Scanned 15 files in 3
  folders for 142 keys.").

### Service filtering

- The `-s` flag MUST filter the scan to only the named service(s).

### Cleanup (`--clean`)

- `nv unused --clean` MUST remove unused keys from all scanned files.
- `nv unused --clean` MUST show a diff preview and prompt for confirmation
  before writing (using the standard `preview_and_apply` flow).
- `--dry-run` MUST show the diff without writing.
- `--yes` MUST skip the confirmation prompt.

### Configuration

Skip directories and files can be configured globally or per-service in `nv.yml`.

**Global configuration** (applies to all services):
```yaml
commands:
  unused:
    skip_dirs:
      - dist
      - build
    skip_files:
      - custom.js
      - test.ts
```

- The configured `skip_dirs` MUST be merged with the built-in defaults
  (`.git`, `target`, `vendor`, `node_modules`, `logs`).
- The configured `skip_files` has no built-in defaults.

**Per-service configuration** (applies only to that service):
```yaml
services:
  auth:
    path: services/auth
    commands:
      unused:
        skip_dirs:
          - generated
          - test_fixtures
        skip_files:
          - custom.js
          - docker/**/*.env*
  api:
    path: services/api
    commands:
      unused:
        skip_dirs:
          - vendor
          - legacy
        skip_files:
          - test.ts
          - config/secrets.yml
```

- Global and per-service `skip_dirs` MUST be merged when searching within a
  service.
- Global and per-service `skip_files` MUST be merged when searching within a
  service.
- `skip_files` entries support glob patterns:
  - `*` matches any characters except `/` (within a single path segment)
  - `**` matches any characters including `/` (recursive across directories)
  - `?` matches a single character
- Examples:
  - `docker/.env` — matches exactly `docker/.env`
  - `docker/*.env` — matches `docker/.env`, `docker/app.env`, etc.
  - `docker/**/*.env*` — matches all env files in `docker/` and any subfolder
  - `*.test.js` — matches any `.test.js` file at any depth
- `skip_files` entries are matched against the relative path from the service
  root directory (e.g., `docker/.env`, `docker/app/.env.example`).
- `skip_files` entries can also match just the file name (e.g., `custom.js`).
- If a service has no `unused.skip_dirs` or `unused.skip_files`, only the global
  lists and defaults apply.

### CLI surface

```
nv unused [OPTIONS]

Options:
  -s <SERVICE>    The service name to scan (repeatable)
  --clean         Remove unused keys (with preview)
```

Uses global flags: `--no-config`, `--root`, `--all`, `--dry-run`, `--yes`.

## Acceptance criteria

- [ ] Given a service with `.env` containing `DB_HOST=localhost` and `DB_HOST`
      is referenced in source code, when `nv unused` is run, then `DB_HOST` is
      NOT listed.
- [ ] Given a service with `.env` containing `OLD_KEY=deprecated` and `OLD_KEY`
      is NOT referenced anywhere in source code, when `nv unused` is run, then
      `OLD_KEY` is listed as unused.
- [ ] Given a service with `.env.example` containing `API_KEY=xxx` and `API_KEY`
      is not referenced in source code, when `nv unused` is run, then `API_KEY`
      is listed as unused.
- [ ] Given a service with `configmap.yml` containing nested key
      `data.DB_HOST: localhost` and `DB_HOST` is not referenced in source code,
      when `nv unused` is run, then `DB_HOST` is listed as unused.
- [ ] Given a service with `secrets.yml` containing
      `stringData.DB_PASSWORD: secret` and `DB_PASSWORD` is referenced in
      source code, when `nv unused` is run, then `DB_PASSWORD` is NOT listed.
- [ ] Given no unused keys, when `nv unused` is run, then "No unused keys
      found." is printed.
- [ ] Given `-s auth`, when `nv unused` is run, only the `auth` service is
      scanned.
- [ ] Given a key `db_host` (lowercase) defined in a file and `DB_HOST`
      (uppercase) referenced in code, when `nv unused` is run, then `db_host`
      IS listed (case-sensitive mismatch).
- [ ] Given a key `DB_HOST` defined in `.env` and also appearing in another
      `.env` file but NOT in source code, when `nv unused` is run, then
      `DB_HOST` IS listed (env-like files are excluded from search).
- [ ] Given keys `PHP_INI_LOG_ERRORS` and `PHP_INI_LOG_ERRORS_MAX_LEN` both
      defined in configmap files, and both referenced in source code, when
      `nv unused` is run, then NEITHER is listed (longest-match semantics
      ensure prefix keys don't mask longer keys).
- [ ] Given `--clean`, when confirmed, unused keys are removed from all scanned
      files.
- [ ] Given `--clean` with no unused keys, "Nothing to change." is printed.
- [ ] Given a service `auth` with `unused.skip_dirs: [generated]` and a key
      defined in source code only within `auth/generated/`, when `nv unused` is
      run, then that key IS listed as unused (the `generated` directory is
      skipped for that service).
- [ ] Given a service `auth` with `unused.skip_files: [custom.js]` and a key
      defined in source code only within `auth/custom.js`, when `nv unused` is
      run, then that key IS listed as unused (the `custom.js` file is
      skipped for that service).
- [ ] Given a service `auth` with `unused.skip_files: [docker/.env]` and a key
      defined in source code only within `auth/docker/.env`, when `nv unused` is
      run, then that key IS listed as unused (the `docker/.env` file is
      skipped for that service).
- [ ] Given global `unused.skip_dirs: [dist]` and a service `api` with
      `unused.skip_dirs: [legacy]`, when searching within `api`, both `dist`
      and `legacy` directories are skipped.
- [ ] Given global `unused.skip_files: [test.js]` and a service `api` with
      `unused.skip_files: [old.js]`, when searching within `api`, both `test.js`
      and `old.js` files are skipped.

## Edge cases

- Files that cannot be read (permissions, missing) are silently skipped.
- Keys with special characters (dots, hyphens) are searched literally.
- Keys that share a common prefix with other keys (e.g., `FOO` and `FOO_BAR`)
  are matched independently using longest-match semantics.
- Empty values in env files are skipped (no key to search for).
- Binary files are skipped during search.
- Very large files are handled without excessive memory usage.
- `--clean` with `--dry-run` shows the diff without writing.
- Service-specific `skip_dirs` and `skip_files` are additive to global
  `skip_dirs`/`skip_files` and defaults.
- `skip_files` entries support glob patterns (`*`, `**`, `?`) for flexible
  file matching. For example, `docker/**/*.env*` matches all env files in
  `docker/` and any subfolder.
