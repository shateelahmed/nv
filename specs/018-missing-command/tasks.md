# Tasks: `nv missing` command

- **Spec:** [spec.md](./spec.md)
- **Plan:** [plan.md](./plan.md)

Work through tasks top to bottom. Keep each task small enough to complete and
verify independently. Mark `[x]` when done.

## Tasks

- [x] **T1 — Add `Missing` variant to the CLI**
  - Scope: Added `Missing { environment: String }` variant to `Command`
    (uses global `-s`/`--service` and `--all`). Dispatch to `missing::run`.
  - Files: `src/cli/mod.rs`

- [x] **T2 — Create `missing.rs` with configmap/secrets key collection**
  - Scope: New file `src/cli/missing.rs`. Collects defined keys from
    configmap/secrets scoped to the environment via `in_environment()`
    segment matching into a `BTreeSet`.
  - Files: `src/cli/missing.rs`

- [x] **T3 — PHP codebase scanning**
  - Scope: `php_references()` regex for `env('KEY')`/`getenv("KEY")`.
    Directory walk skips `.git`/`vendor`/`node_modules`/`target`/`logs`.
    Collects `(key, file, line)` tuples.
  - Files: `src/cli/missing.rs`

- [x] **T4 — JavaScript/TypeScript codebase scanning**
  - Scope: `js_references()` regexes for `process.env.KEY`,
    `process.env['KEY']`, and `env('KEY')`. Collects `(key, file, line)`.
  - Files: `src/cli/missing.rs`

- [x] **T5 — Diff and tree display**
  - Scope: missing = referenced minus defined. Displayed in standard tree
    format by source file with green `+` markers and source locations.
    Summary shows total count. Verified on real PHP and JS repos.
  - Files: `src/cli/missing.rs`

- [x] **T5b — Optional `-e` with per-environment reporting**
  - Scope: `-e` is now optional. Without it, `discovered_environments()`
    identifies every environment from configmap/secrets file paths
    (reusing `ENV_WRAPPER_DIRS`) and reports missing keys per environment,
    each labeled `<service>/<env>`. Added unit tests for `env_for()` and
    `in_environment()`.
  - Files: `src/cli/mod.rs`, `src/cli/missing.rs`

- [x] **T6 — Document `nv missing` in README**
  - Scope: Added a row to the commands table (with `--secrets`/`--configmap`
    note already present) and examples under `# Examples`.
  - Files: `README.md`

## Verification checklist

- [x] `cargo build` succeeds with no warnings.
- [x] `cargo test` passes (296 unit + 13 integration).
- [x] `cargo clippy` is clean.
- [x] `cargo fmt` applied.
- [x] All acceptance criteria in the spec are met.