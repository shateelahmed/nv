# Tasks: `nv find` command (`-s` scoping, legend removal, `skip_files`)

- **ID:** 013-find-command
- **Spec:** [spec.md](./spec.md)
- **Plan:** [plan.md](./plan.md)
- **Status:** Implemented
- **Author:** shateel
- **Date:** 2026-07-31

## Task list

- [x] **T1: Remove the color legend**
  - Scope: Delete `print_legend()`, `colorize_color_name()`, their call in
    `run()`, the now-unused `AnsiColor` import, and the
    `print_legend_does_not_panic` test.
  - Files: `src/cli/find.rs`
  - Verify: `cargo build` (no warnings); output starts with the tree, no
    `Color legend:` lines.

- [x] **T2: Scope search to `-s`/`--service` services**
  - Scope: In `run()`, compute `ctx.service_filter(cli)`, filter
    `ctx.services` by name (clone into `Vec<Service>`; no filter when empty or
    `--all`), and pass the filtered list to `search::build_index()`.
  - Files: `src/cli/find.rs`
  - Verify: `cargo build`; manual — `find -s auth db` returns only auth
    matches, `find -s auth <billing-only-key>` prints `No matches.`.

- [x] **T3: Add `FindConfig` and `find_skip_files_for()` to config**
  - Scope: Add `FindConfig { skip_files }` to `src/config.rs`,
    `CommandsConfig::find`, and `Config::find_skip_files_for(service)`
    (global + per-service merge, deduplicated).
  - Files: `src/config.rs`
  - Verify: `cargo test` — 4 new tests (empty, global, per-service, merge).

- [x] **T4: Share skip helpers in `context.rs`**
  - Scope: Move `matches_skip_pattern`, `file_is_skipped`, `build_skip_files`
    from `compare.rs` to `context.rs` (generic `build_skip_files` takes an
    accessor closure; `file_is_skipped` takes the service root path). Update
    `compare.rs` call sites and tests.
  - Files: `src/cli/context.rs`, `src/cli/compare.rs`
  - Verify: `cargo test` (compare + context tests pass).

- [x] **T5: Apply `skip_files` in find**
  - Scope: In `run()`, for each scoped service drop files matching
    `commands.find.skip_files` (global + per-service) before building the
    index.
  - Files: `src/cli/find.rs`
  - Verify: `cargo build`; manual — global `skip_files: [docker/.env]` hides
    nested keys, per-service `skip_files: [.env]` hides only that service.

- [x] **T6: Document `commands.find.skip_files` in `nv.yml.example`**
  - Scope: Add global and per-service examples.
  - Files: `nv.yml.example`
  - Verify: comment block matches the compare example.

- [x] **T7: Verify the command end-to-end**
  - Scope: Run the full test suite and smoke tests on a scratch multi-service
    tree (legend absent, `-s` scoping, `--all` override, skip_files
    filtering).
  - Verify: `cargo test` (all green), `cargo clippy`, `cargo fmt --check`.

## Verification checklist

- [x] `cargo build` succeeds with no warnings.
- [x] `cargo test` passes.
- [x] `cargo clippy` is clean.
- [x] `cargo fmt` applied.
- [x] All acceptance criteria in the spec are met.
