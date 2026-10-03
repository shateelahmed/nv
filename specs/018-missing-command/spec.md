# Spec: `nv missing` command

- **ID:** 018-missing-command
- **Status:** Implemented
- **Author:** shateel
- **Date:** 2026-08-14

## Summary

`nv missing` scans the project source tree for environment-variable references
in code and reports any that are absent from the service's configmap and
secrets files for a given environment. It is the inverse of `nv unused`:
unused finds keys defined but not referenced; missing finds keys referenced
but not defined.

## Problem / Motivation

- Deployments fail silently or at runtime when code references an env key
  that was never added to the configmap or secrets.
- Currently there is no automated way to detect this gap; developers must
  manually compare their code's `env()` / `process.env` calls against
  deployment manifests.
- The problem spans two ecosystems: PHP (Laravel `env()`, native `getenv()`)
  and JavaScript/TypeScript (`process.env.KEY`, `env('KEY')`).

## Goals

- Detect env keys referenced in source code but absent from configmap/secrets
  for a specified environment.
- Support both PHP and JavaScript/TypeScript codebases via language-aware
  regex patterns.
- Follow the same CLI conventions as `nv unused` (`--service`, environment
  filtering, skip_files, etc.).
- Report each missing key with the source file and line where it is
  referenced.

## Non-goals

- Editing or fixing missing keys.
- Scanning Dockerfiles for ARG/ENV references (Dockerfile keys are
  build-time, not application code).
- Scanning `.env` files for missing keys (those are local development
  files, not deployed configmaps).
- TUI changes.

## User stories

- As a PHP developer, I want `nv missing -s auth -e dev` to show me every
  `env('KEY')` and `getenv('KEY')` call whose key is not in the dev
  configmap or secrets, so I know what to add before deploying.
- As a JS/TS developer, I want the same command to catch `process.env.KEY`
  and `env('KEY')` references missing from my deployment manifests.
- As a platform engineer, I want to run this in CI to prevent deployments
  with missing env keys.

## Language patterns

### PHP

| Pattern | Example | Captured key |
| --- | --- | --- |
| `env('KEY')` | `env('APP_DEBUG')` | `APP_DEBUG` |
| `env("KEY")` | `env("DB_HOST")` | `DB_HOST` |
| `env('KEY', default)` | `env('CACHE_TTL', 60)` | `CACHE_TTL` |
| `getenv('KEY')` | `getenv('HTTP_PROXY')` | `HTTP_PROXY` |
| `getenv("KEY")` | `getenv("NO_PROXY")` | `NO_PROXY` |

Regex for PHP: `env\(\s*['"]([A-Za-z_][A-Za-z0-9_]*)['"]` and
`getenv\(\s*['"]([A-Za-z_][A-Za-z0-9_]*)['"]`.

Only `.php` files are scanned. Vendor directories are skipped by default.

### JavaScript / TypeScript

| Pattern | Example | Captured key |
| --- | --- | --- |
| `process.env.KEY` | `process.env.API_KEY` | `API_KEY` |
| `process.env['KEY']` | `process.env['DB_HOST']` | `DB_HOST` |
| `process.env["KEY"]` | `process.env["NO_PROXY"]` | `NO_PROXY` |
| `env('KEY')` | `env('MIX_APP_URL')` | `MIX_APP_URL` |
| `env("KEY")` | `env("API_SECRET")` | `API_SECRET` |

Regex for JS: `process\.env\.([A-Za-z_][A-Za-z0-9_]*)` and
`env\(\s*['"]([A-Za-z_][A-Za-z0-9_]*)['"]`.

Only `.js`, `.ts`, `.tsx`, `.jsx` files are scanned. `node_modules` and
`vendor` are skipped by default.

## Behavior & requirements

### CLI surface

```
nv missing [OPTIONS]

Options:
  -s, --service <NAME>       Service to scan (repeatable, or --all).
  -e, --environment <ENV>    Environment to check against (optional,
                             e.g. dev, prod, qa1, local). When omitted,
                             missing keys are reported per environment.
  --all                      Scan every service.
```

`-e` is optional. At least one of `-s` or `--all` must be provided.

### Behavior when `-e` is omitted

When `--environment` is omitted, the command discovers every environment
present in the service's configmap/secrets file paths and reports missing
keys for each one. Each environment is shown as its own service node labeled
`<service>/<env>`:

```
pol-payment-core-ms/dev/
└── src/bootstrap/app.php (3)
    ├── + DOB_OTP_MOCK_VERIFICATION_TOKEN src/bootstrap/app.php:125
    ├── + DOB_SUBCONNECT_MOCK_AUTH_TOKEN src/bootstrap/app.php:124
    └── + MAIL_DRIVER                    src/bootstrap/app.php:129

pol-payment-core-ms/prod/ (7)
└── src/bootstrap/app.php (7)
    ├── + APP_ENV   src/bootstrap/app.php:10
    ...
```

Environment names are matched like `nv ls` and `nv changes`: a
path segment that is not a structural container folder
(`deploy`, `kubernetes`, `secrets`, etc.).

### Sources of defined keys

Keys are considered **defined** when they appear in:

1. Configmap files for the specified environment (`deploy/<ENV>/`).
2. Secrets files for the specified environment (`deploy/<ENV>/`).

`.env` files and Dockerfiles are NOT treated as definitions — they are
local/build-time and do not constitute a deployed configmap entry.

### Sources of referenced keys

Keys are considered **referenced** when they appear in source code files
matching the language patterns above, subject to:

- **Skip dirs:** `.git`, `target`, `vendor`, `node_modules`, `logs`
  (same defaults as `nv unused`).
- **Skip files:** configurable via `commands.missing.skip_files` in
  `nv.yml` (same mechanism as `nv unused`).
- **Word-boundary check:** a match must be a whole-word occurrence
  (surrounded by non-alphanumeric, non-underscore characters or line
  boundaries) to avoid false positives from substrings.

### Output

The command uses the standard hierarchical tree format (same as `nv unused`,
`nv duplicates`, etc.), grouped by the source file where each key is
referenced:

```
pol-payment-core-ms/ (3)
└── src/bootstrap/app.php (3)
    ├── + DOB_OTP_MOCK_VERIFICATION_TOKEN src/bootstrap/app.php:125
    ├── + DOB_SUBCONNECT_MOCK_AUTH_TOKEN  src/bootstrap/app.php:124
    └── + MAIL_DRIVER                     src/bootstrap/app.php:129

3 missing key(s) found.
```

- `+` indicates a missing key (green/added color).
- Files are grouped under the service; each key shows its source file and
  line.
- A key referenced in multiple files appears under each source file.
- The summary line shows the total count of missing keys.

### Config source banner

The standard `Config source:` banner is printed to stderr.

### Environment matching

Same as `nv ls` and `nv changes`: the `--environment` value is matched as
a path segment in file paths (e.g. `dev` matches
`deploy/dev/kubernetes/configmap-*.yaml`).

## Acceptance criteria

- [ ] Given `-s auth -e dev`, keys referenced in `env()` / `getenv()` calls
      in PHP files but absent from dev configmap/secrets are reported.
- [ ] Given `-s admin-ui -e dev`, keys referenced via `process.env.KEY` in
      JS/TS files but absent from dev configmap/secrets are reported.
- [ ] Given a key referenced in code AND present in the configmap, it is NOT
      reported.
- [ ] Given a key referenced in code but present in `.env` (not configmap),
      it IS reported (`.env` is not a definition).
- [ ] Given `--all -e dev`, all services are scanned.
- [ ] Given `-s auth` without `-e`, missing keys are reported per environment,
      each labeled `<service>/<env>`.
- [ ] Given no `-e`, the command exits with a clap usage error.
- [ ] Given no `-s` and no `--all`, the command exits with a clap usage
      error.
- [ ] Vendor and node_modules directories are skipped by default.
- [ ] `skip_files` from `nv.yml` are respected.
- [ ] Keys are matched with word-boundary checks (no substring false
      positives).
- [ ] Multiple references to the same key show all source locations.

## Edge cases

- Service has no source code files → reports nothing (all keys are
  "missing" from code perspective, but the command only reports code→config
  gaps, not the reverse).
- Code references a key via string concatenation (e.g.
  `env('APP_' . 'DEBUG')`) → not detected (out of scope for regex-based
  scanning).
- Code references `env()` with a variable (e.g. `env($key)`) → not detected.
- Key appears in both configmap and secrets for the same environment →
  considered defined (no false positive).

## Open questions

- (none)

## Assistant-config sync

No assistant-config change is required. This spec adds a new command with no
cross-cutting rule, convention, workflow step, or project guarantee.
