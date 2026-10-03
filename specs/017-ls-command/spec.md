# Spec: `nv ls` command

- **ID:** 017-ls-command
- **Status:** Implemented
- **Author:** shateel
- **Date:** 2026-08-12

## Summary

`nv ls` lists every unique environment-variable key across configmap, secrets,
`.env`, and Dockerfile sources for a given service and environment. The user
must specify both a service and an environment. Dockerfile and `.env` keys are
common to every environment and always appear regardless of which environment
is specified.

## Problem / Motivation

- There is no single command to see all env keys in a service for a given
  deployment environment. Users must open each configmap, secrets, and `.env`
  file individually to understand what keys a service uses.
- When migrating, auditing, or documenting a service, a consolidated key list
  is the fastest way to understand the service's environment shape.
- Dockerfile and `.env` keys are shared across environments but are invisible
  to `nv find`, which searches key/value content but not Dockerfile ARG/ENV
  directives.

## Goals

- List all unique env-key names across four source types:
  - Configmap files for the specified environment
  - Secrets files for the specified environment
  - `.env` files (common to all environments)
  - Dockerfile `ARG` and `ENV` directives (common to all environments)
- Require both `--service` and `--environment` to scope the scan.
- Output a deduplicated, alphabetically sorted list of key names.

## Non-goals

- Editing or modifying any files.
- Showing key values (this is a key-listing command, not a display command).
- Supporting `nv ls` without an environment (always requires `-e`).
- Scanning `.env.example` files (those contain placeholder/example keys, not
  deployed keys).
- TUI changes.

## User stories

- As a developer, I want `nv ls -s pol-payment-core-ms -e dev` to show me
  every env key used in the dev deployment so I can audit the service
  configuration quickly.
- As a platform engineer, I want to compare key lists across environments
  (by running `ls` with different `-e` values) so I can spot environment
  drift.
- As a developer, I want Dockerfile ARG/ENV keys included in the list so I
  understand what build-time and runtime keys affect the container.

## Behavior & requirements

### CLI surface

```
nv ls [OPTIONS]

Options:
  -s, --service <NAME>       Service to scan (required).
  -e, --environment <ENV>    Environment to scope configmap/secrets to (required,
                             e.g. dev, prod, qa1, uat1, optimization1, local).
  --flat                     Print all keys on a single line separated by spaces.
  --secrets                  Only list keys from secrets files (mutually exclusive
                             with --configmap).
  --configmap                Only list keys from configmap files (mutually
                             exclusive with --secrets).
```

Both `-s` and `-e` are required. If either is missing, the command MUST exit
with a clap usage error before scanning.

`--secrets` and `--configmap` are mutually exclusive. When `--secrets` is
specified, only secrets files are scanned and Dockerfiles are excluded. When
`--configmap` is specified, only configmap files are scanned and Dockerfiles
are excluded. When neither is specified, all applicable sources are scanned.

### Sources

Keys are collected from up to four source types:

1. **Configmap** (`FileKind::ConfigMap`): only files whose path contains
   `deploy/<ENV>/` as a path segment. Every key parsed from the file's
   `data:` section is collected.

2. **Secrets** (`FileKind::Secret`): only files whose path contains
   `deploy/<ENV>/` as a path segment. Every key parsed from the file's
   `data:` or `stringData:` section is collected.

3. **`.env` files** (`FileKind::Dotenv`): only included when the user
   specifies `local` as the environment (`-e local`). These are local
   development files and are not part of any deployed environment.
   `FileKind::DotenvExample` files are always excluded.

4. **Dockerfiles**: all files named `Dockerfile*` under the service root.
   Keys are `ARG` and `ENV` directive names (e.g. `ARG HTTP_PROXY` → key
   `HTTP_PROXY`). Dockerfile keys are common to every environment.

### Deduplication & ordering

- Each key appears at most once in the output, regardless of how many source
  files contain it.
- Keys MUST be sorted case-insensitively (alphabetical, A–Z).

### Output

The command prints two sections:

1. **Files (N):** — all source files that contributed keys, one per line,
   indented with two spaces. The count in parentheses shows the number of
   files.
2. **Keys (N):** — all unique key names, one per line (or all on one line
   with `--flat`), indented with two spaces. The count in parentheses shows
   the number of keys.

- A summary is printed to stderr: `N key(s) listed.`
- When no keys are found, the command prints `No keys found.` to stderr and
  exits successfully.

### Config source banner

The standard `Config source:` banner is printed to stderr (same as every other
command).

### Environment matching

- The `--environment` value is matched as a path segment (e.g. `dev` matches
  `deploy/dev/kubernetes/configmap-*.yaml`). The same matching logic used by
  `nv changes` applies here.
- If the environment matches no configmap or secrets files, the command still
  succeeds and lists only the common keys (`.env` + Dockerfile).

### `-s`/`--service`

- The service must exist in the discovered service list. If not found, the
  command exits with a clear error.
- `--all` is not supported by `ls` (exactly one service is required).

## Acceptance criteria

- [ ] Given `-s pol-payment-core-ms -e dev`, configmap/secrets files from
      `deploy/dev/kubernetes/` and Dockerfiles appear in the Files section;
      their keys appear in the Keys section. `.env` files do NOT appear.
- [ ] Given `-s pol-payment-core-ms -e prod`, only prod configmap/secrets
      files appear (not dev/qa1/uat1/optimization1). Dockerfiles still appear.
- [ ] Given `-s pol-payment-core-ms -e local`, `.env` files appear in the
      Files section and their keys appear in the Keys section.
- [ ] Given `-s pol-payment-core-ms -e nonexistent`, the command succeeds and
      lists only Dockerfile files/keys (no configmap/secrets/.env keys).
- [ ] Given no `-s`, the command exits with a clap usage error.
- [ ] Given no `-e`, the command exits with a clap usage error.
- [ ] Given `-s nonexistent-service -e dev`, the command exits with a clear
      error.
- [ ] Given a key that appears in multiple files, it appears only once in the
      Keys section.
- [ ] Keys are sorted alphabetically.
- [ ] Files are sorted alphabetically.
- [ ] `.env.example` files are never scanned.
- [ ] With `--flat`, all keys appear on a single space-separated line.
- [ ] Dockerfile ARG/ENV keys are included regardless of the `-e` value.

## Edge cases

- Service exists but has no files → prints `No keys found.`.
- Service has `.env` and Dockerfiles but no configmap/secrets for the given
  environment → lists only common keys.
- Multiple Dockerfiles in a service → all are scanned; duplicate keys across
  Dockerfiles are deduplicated.
- Dockerfile has multiline `ENV` (e.g. `ENV FOO="bar" \` followed by a
  continuation line) → only the first token on each directive line is treated
  as a key name.
- Empty or missing `.env` files → skipped silently.

## Open questions

- (none)

## Assistant-config sync

No assistant-config change is required. This spec adds a new command with no
cross-cutting rule, convention, workflow step, or project guarantee.
