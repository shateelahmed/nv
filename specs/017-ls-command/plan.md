# Plan: `nv ls` command

- **Spec:** [spec.md](./spec.md)
- **Status:** Draft

## Overview

Add a new `nv ls` subcommand that scans a single service's four key sources
(configmap, secrets, `.env`, Dockerfile), collects all unique key names, and
prints a sorted flat list. The command takes two required flags
(`--service`, `--environment`) and uses the same environment matching logic
already proven in `nv changes`.

No existing modules change; a new `src/cli/ls.rs` module contains all
command-specific logic.

## Architecture & modules

| Module | Change | Responsibility |
| --- | --- | --- |
| `src/cli/mod.rs` | edit | Add `Ls` variant to `Command` with `service: String` and `environment: String` args; dispatch to `ls::run`. |
| `src/cli/ls.rs` | new | All `nv ls` logic: resolve service, collect keys from 4 sources, dedup, sort, print. |
| `README.md` | edit | Document the new command in the commands table and add an example. |

No new crates.

## Data flow

```
nv ls -s <service> -e <env>
       │
       ▼
  context::resolve(cli)  →  Context
       │
       ▼
  find service by name   →  &Service (bail if not found)
       │
       ├─ configmaps  →  filter files where path contains deploy/<env>/
       │                  parse each → collect key names
       │
       ├─ secrets     →  filter files where path contains deploy/<env>/
       │                  parse each → collect key names
       │
       ├─ .env        →  all FileKind::Dotenv files (skip DotenvExample)
       │                  parse each → collect key names
       │
       └─ Dockerfile  →  glob docker/**/Dockerfile*
                          extract ARG/ENV names → collect key names
       │
       ▼
  deduplicate → sort → print one key per line
```

## Key decisions & trade-offs

- **Decision:** require both `-s` and `-e` — **Because:** the command is
  meaningless without a service, and configmap/secrets keys depend on
  environment; **Alternatives:** default environment to "all" (rejected:
  produces too many keys and defeats the purpose of env-specific listing).
- **Decision:** `.env` files included, `.env.example` excluded — **Because:**
  `.env` contains actual deployed values while `.env.example` contains
  placeholders; **Alternatives:** include `.env.example` (rejected: pollutes
  the list with example-only keys).
- **Decision:** Dockerfiles parsed for ARG/ENV directives only — **Because:**
  only `ARG` and `ENV` define env-like keys; **Alternatives:** also parse
  `COPY`/`ENTRYPOINT` (rejected: not key-value env definitions).
- **Decision:** flat sorted output, no tree lines — **Because:** the user
  explicitly asked for a list of unique keys; a tree with one service would
  add noise; **Alternatives:** tree format (rejected: overengineered for a
  flat list).
- **Decision:** environment matching via path segment (`deploy/<env>/`) —
  **Because:** same proven logic as `nv changes`; **Alternatives:**
  config-based env list (rejected: not all projects configure environments).

## Dependencies

None new.

## Risks & mitigations

- **Risk:** Dockerfiles may have multiline `ENV` where only the first line
  carries a key name — **Mitigation:** parse the first token after `ARG` or
  `ENV` on each line; continuation lines (starting with `\`) are ignored.
- **Risk:** Environment string doesn't match any deploy folder — **Mitigation:**
  command succeeds with only common keys; no error (matches `changes`
  behavior).
- **Risk:** Service not found — **Mitigation:** bail with a clear error
  before scanning.

## Testing strategy

- Unit tests in `ls.rs`: key collection from a mock index, deduplication,
  sorting, empty result.
- Unit test: Dockerfile ARG/ENV extraction from a content string.
- Unit test: error when service not found.
- Manual: `nv ls -s pol-payment-core-ms -e dev` and `-e prod` on the real
  repo; verify key sets differ for configmap/secrets and overlap for
  `.env`/Dockerfile.

## Rollout / migration

No `nv.yml` changes. README gains the new command row and an example.
