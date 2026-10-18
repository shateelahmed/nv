# Plan: 020 - Flag unification and canonical CLI surface

- **Spec:** [spec.md](./spec.md)
- **Status:** Approved

## Overview

Converge every subcommand onto the single global filter vocabulary already defined in `src/cli/mod.rs` (`Cli`): `-s/--service` (repeatable), `-f/--file` (repeatable), `--all` (with a new `-a` short), `--dry-run`, `-y/--yes`. Subcommands keep only orthogonal, command-specific flags (environment scoping, exact file paths, mode flags like `--clean`/`--flat`). The work is a set of CLI-surface edits plus handler signature changes, followed by a shared helper so the "exactly one service" rule lives in one place.

## Architecture & modules

| Module | Change | Responsibility |
| --- | --- | --- |
| `src/cli/mod.rs` | edit | Remove subcommand `-s` from `Unused`/`Duplicates`; remove `-e/-c/-x/-a` from `Remove`; change `Encrypt`/`Decrypt` from `-S`/`-F` to global `-s` + `--path`; add short `-a` to global `--all`; update dispatch arms. |
| `src/cli/remove.rs` | edit | Drop `env/configmap/secrets/all` params; build `kind_filter` from global `ctx.kind_filter(cli)`; use `ctx.service_filter(cli)` only. |
| `src/cli/unused.rs` | edit | Drop `service_names` param; use `ctx.service_filter(cli)` for both `collect_all_keys` and `search_dirs`. |
| `src/cli/duplicates.rs` | edit | Drop `service_names` param; use `ctx.service_filter(cli)`. |
| `src/cli/encrypt.rs` | edit | Accept `path: &str` (from `--path`) and use `ctx.service_filter(cli)` (exactly one service) instead of `-S`. |
| `src/cli/ls.rs` | edit | Use `require_single_service` so `--all` and multiple services are rejected with the same error as `changes`. |
| `src/cli/changes.rs` | edit | Use the shared single-service helper. |
| `src/cli/context.rs` | edit | Add `require_single_service(cli, services) -> Result<String>` helper used by `ls`, `changes`, `encrypt`. |
| `README.md` | edit | Document the canonical flag pattern and update command examples; add the missing commands (remove, leaks, fake-secrets, unused, duplicates, encrypt/decrypt). |
| `CLAUDE.md`, `.github/copilot-instructions.md`, `.claude/commands/*.md`, `.github/prompts/*.md` | edit | Sync the durable CLI convention (per spec 001) — global filters are the only service/file targeting mechanism. |

## Data flow

1. `Cli::parse()` reads global filters + subcommand args.
2. `context::resolve(cli)` builds `Context` (services, config, base).
3. `ctx.service_filter(cli)` / `ctx.kind_filter(cli)` produce the effective filters (empty = all).
4. Subcommand handler targets files via `edit::collect_targets(&ctx.services, &service_filter, &kind_filter)` or its own logic (for `unused`/`missing`/`ls`/`changes` which use other discovery paths).
5. Write-capable commands call `context::preview_and_apply` which honors global `--dry-run` / `-y`.

## Key decisions & trade-offs

- **Decision:** `Remove` uses global `-f/--file` + `--all` instead of its own `-e/-c/-x/-a`. **Because:** spec FR2 requires one file-kind vocabulary; parallel flags are the source of the current inconsistency. **Alternatives:** keep `-e/-c/-x` (rejected — perpetuates the split; `-e` already collides with `Ls`/`Missing` environment).
- **Decision:** `Encrypt`/`Decrypt` use global `-s` + command-specific `--path`. **Because:** their `-F` is an exact file path, not a kind, so it cannot map onto global `-f`; `--path` is the orthogonal command-specific concept. **Alternatives:** positional `SERVICE PATH` (rejected by user), keep `-S/-F` (rejected — different case from global).
- **Decision:** Add short `-a` as an alias for global `--all`. **Because:** after removing `Remove`'s `-a`, a short alias keeps muscle memory and matches how `-s`/`-f`/`-y` have shorts. **Trade-off:** `-a` was previously ambiguous with Remove's "all file types"; now it unambiguously means "everything".
- **Decision:** Centralize "exactly one service" in `context::require_single_service`, which requires exactly one `--service` and rejects `--all`. **Because:** `ls.rs` and `changes.rs` re-implement it with divergent `--all` handling; `ls` currently ignores `--all` while `changes` rejects it. Both commands are single-service, so the uniform rule is to require one service and reject `--all` with an identical error. **Alternatives:** make `ls --all` list every service (rejected — adds multi-service output, out of scope for a flag-uniformity change); leave duplicated (rejected — causes the FR8 drift).
- **Decision:** `remove` with no `-f` and no `--all` defaults to all file kinds. **Because:** matches prior `-s`-without-type behavior (FR5 / AC1).

## Dependencies

None. Reuses `clap`, `anyhow`, and existing `context`/`edit` helpers.

## Risks & mitigations

- **Risk:** breaking existing user scripts that call `nv remove KEY -e -x`, `nv unused -s auth`, `nv encrypt -S auth -F file`. **Mitigation:** intentional per spec; document in README; tests updated to the new shape.
- **Risk:** `remove`'s mutual-exclusivity validation (`-a` vs `-e/-c/-x`) disappears, changing error paths. **Mitigation:** the new shape has no `-a`/`-e` to conflict; `--all` vs `-f` interplay handled by `kind_filter`/`service_filter` returning empty.
- **Risk:** `unused`/`duplicates` double-filtering removal changes which services are scanned when both global `-s` and subcommand `-s` were previously combined. **Mitigation:** only one `-s` exists now, sourced from `ctx.service_filter`.

## Testing strategy

- **Unit:** update `remove.rs` tests for the new flag shape (no `-e/-c/-x/-a`); add tests that `ls --all` does not require exactly one service; test `context::require_single_service` (0, 1, 2 services; with/without `--all`).
- **Integration:** `cargo test` for `Cli::try_parse_from` cases on the new surfaces (`nv remove KEY -f configmap`, `nv encrypt --key k -s auth --path file`, `nv unused --all`, `nv duplicates -s auth`).
- **Manual:** `nv <cmd> --help` for each command to confirm uniform help; run `nv remove`, `nv unused`, `nv duplicates`, `nv encrypt`, `nv ls --all -e dev` against the verification repo.

## Rollout / migration

No `nv.yml` or file-format changes. This is a pure CLI-surface change; any breaking flag changes are intentional and documented in `README.md`. Assistant configs (`.github/*`, `CLAUDE.md`, `.claude/*`) are updated together per spec 001.
