# Plan: `nv missing` command

- **Spec:** [spec.md](./spec.md)
- **Status:** Draft

## Overview

Add a new `nv missing` subcommand that finds env keys referenced in source
code (PHP `env()`/`getenv()`, JS `process.env`/`env()`) but absent from the
service's configmap and secrets for a given environment. The command reuses
the directory-scanning infrastructure from `nv unused` but inverts the logic:
instead of collecting keys from env files and searching code for matches, it
collects keys from code and checks them against configmap/secrets.

## Architecture & modules

| Module | Change | Responsibility |
| --- | --- | --- |
| `src/cli/mod.rs` | edit | Add `Missing` variant to `Command`; dispatch to `missing::run`. |
| `src/cli/missing.rs` | new | All `nv missing` logic: language-aware regex scanning, configmap/secrets key collection, diff, display. |
| `README.md` | edit | Document the new command in the commands table and add an example. |

No new crates — regex is already a dependency.

## Data flow

```
nv missing -s <service> -e <env>
       │
       ▼
  context::resolve(cli)  →  Context
       │
       ▼
  for each selected service:
       │
       ├─ collect defined keys
       │    filter service.files where kind ∈ {ConfigMap, Secret}
       │    AND path contains deploy/<env>/
       │    parse each → collect key names into BTreeSet
       │
       ├─ collect referenced keys (code scan)
       │    walk service source tree (skip .git/vendor/node_modules/target/logs)
       │    for each .php file: match env('KEY') and getenv('KEY')
       │    for each .js/.ts/.tsx/.jsx file: match process.env.KEY
       │      and env('KEY')
       │    for each match: record (key, file, line)
       │
       └─ diff: referenced keys NOT in defined keys → missing
            display in tree format with source locations
```

## Key decisions & trade-offs

- **Decision:** regex-based scanning (same as `nv unused`) — **Because:**
  full AST parsing is overkill for env key extraction and would require
  language-specific parsers; **Alternatives:** tree-sitter (rejected:
  adds heavy dependency, diminishing returns for simple patterns).
- **Decision:** `.env` files are NOT definitions — **Because:** `.env` is
  local development config, not a deployed configmap; **Alternatives:**
  treat `.env` as defined (rejected: defeats the purpose of catching
  missing deployment config).
- **Decision:** Dockerfiles are NOT scanned for code references — **Because:**
  Dockerfile ARG/ENV are build-time, not application code; **Alternatives:**
  also scan Dockerfiles (rejected: would produce false positives for
  build-only args).
- **Decision:** word-boundary check on matches — **Because:** prevents
  substring false positives (e.g. `MY_APP_KEY` matching `APP_KEY`);
  **Alternatives:** no boundary check (rejected: too many false positives).
- **Decision:** require `-e` (environment) — **Because:** configmap/secrets
  are environment-specific; **Alternatives:** default to all environments
  (rejected: would report false positives for keys only in prod).

## Dependencies

None new.

## Risks & mitigations

- **Risk:** false positives from `env()` in non-PHP contexts (e.g. JS
  `env()` call) — **Mitigation:** language-specific file extension filtering
  (`.php` for PHP patterns, `.js/.ts/.tsx/.jsx` for JS patterns).
- **Risk:** false negatives from dynamic key construction (e.g.
  `env("APP_{$suffix}")`) — **Mitigation:** out of scope; documented in
  spec as accepted limitation.
- **Risk:** performance on large codebases — **Mitigation:** reuse the
  same file-filtering and skip-dir infrastructure from `nv unused`.

## Testing strategy

- Unit tests in `missing.rs`: PHP pattern matching, JS pattern matching,
  word-boundary check, key collection from mock configmap content.
- Manual: `nv missing -s pol-payment-core-ms -e dev` on the real PHP repo;
  `nv missing -s pol-payment-admin-ui-ms -e dev` on the real JS repo.

## Rollout / migration

No `nv.yml` changes required. README gains the new command row and an
example. `commands.missing.skip_files` is available for per-service
customization (same config key pattern as `commands.unused.skip_files`).
