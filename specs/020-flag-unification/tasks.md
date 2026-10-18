# Tasks: 020 - Flag unification and canonical CLI surface

- **Spec:** [spec.md](./spec.md)
- **Plan:** [plan.md](./plan.md)

Work through tasks top to bottom. Keep each task small enough to complete and
verify independently. Mark `[x]` when done.

## Tasks

- [x] **T1 — Add `-a` short alias for global `--all`**
  - Scope: Add short `-a` to the global `--all` argument in `Cli`.
  - Files: `src/cli/mod.rs`
  - Verify: `nv remove KEY --all` and `nv remove KEY -a` both parse; no conflict with command-specific `-a` (which will be removed in a later task).

- [x] **T2 — Centralize "exactly one service" check**
  - Scope: Add `context::require_single_service(cli, services) -> Result<String>` and use it in `ls.rs`, `changes.rs`, and `encrypt.rs`.
  - Files: `src/cli/context.rs`, `src/cli/ls.rs`, `src/cli/changes.rs`, `src/cli/encrypt.rs`
  - Verify: `ls` honors `--all` uniformly; `changes` behavior unchanged; error messages consistent.

- [x] **T3 — Remove: use global `-f/--file` and `--all`**
  - Scope: Drop subcommand `-e/-c/-x/-a` from `Remove`; update `remove::run` signature and logic to use `ctx.kind_filter(cli)` and `ctx.service_filter(cli)`.
  - Files: `src/cli/mod.rs`, `src/cli/remove.rs`
  - Verify: `nv remove KEY` (no `-f`/`--all`) defaults to all kinds; `nv remove KEY -f configmap` targets only configmaps; `nv remove KEY --all` targets everything; mutual-exclusivity validation updated.

- [x] **T4 — Unused: drop subcommand `-s`**
  - Scope: Remove `services` field from `Unused`; update `unused::run` to use `ctx.service_filter(cli)` instead of `service_names`.
  - Files: `src/cli/mod.rs`, `src/cli/unused.rs`
  - Verify: `nv unused -s auth` works via global filter; `nv unused --all` scans all services.

- [x] **T5 — Duplicates: drop subcommand `-s`**
  - Scope: Remove `services` field from `Duplicates`; update `duplicates::run` to use `ctx.service_filter(cli)`.
  - Files: `src/cli/mod.rs`, `src/cli/duplicates.rs`
  - Verify: `nv duplicates -s auth` and `nv duplicates --all` behave consistently.

- [x] **T6 — Encrypt/Decrypt: switch to global `-s` + `--path`**
  - Scope: Replace `-S`/`-F` with global `-s`/`--service` and new `--path`. Update `encrypt::run_encrypt`/`run_decrypt` signatures and logic.
  - Files: `src/cli/mod.rs`, `src/cli/encrypt.rs`
  - Verify: `nv encrypt --key k -s auth --path file` and `nv decrypt --key k -s auth --path file` work; no `-S`/`-F` remain.

- [x] **T7 — Ls and Changes share the single-service rule**
  - Scope: Ensure `ls.rs` uses `require_single_service` so it rejects `--all` and multiple services with the same error as `changes` (currently `ls` ignores `--all` while `changes` rejects it).
  - Files: `src/cli/ls.rs`, `src/cli/changes.rs`
  - Verify: `nv ls --all -e dev` and `nv changes --from x --all` both fail with the identical error; `nv ls -s auth -e dev` works.

- [x] **T8 — Update README.md**
  - Scope: Document the canonical flag pattern; update command examples; add missing commands (remove, leaks, fake-secrets, unused, duplicates, encrypt/decrypt).
  - Files: `README.md`
  - Verify: All commands listed with unified flags.

- [x] **T9 — Sync assistant configs (per spec 001)**
  - Scope: Update `.github/copilot-instructions.md`, `.claude/commands/*.md`, `.github/prompts/*.md`, `CLAUDE.md` to reflect the durable CLI convention (global filters are the only service/file targeting mechanism; `-a` is short for `--all`).
  - Files: `.github/*`, `.claude/*`, `CLAUDE.md`
  - Verify: Both assistant configs document the same convention.

## Verification checklist

- [x] `cargo build` succeeds with no warnings.
- [x] `cargo test` passes.
- [x] `cargo clippy` is clean.
- [x] `cargo fmt` applied.
- [x] All acceptance criteria in the spec are met.
