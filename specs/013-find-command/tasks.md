# Tasks: `nv find` command (`-s` scoping + legend removal)

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

- [x] **T3: Verify the command end-to-end**
  - Scope: Run the full test suite and smoke tests on a scratch multi-service
    tree (legend absent, `-s` scoping, `--all` override).
  - Verify: `cargo test` (all green), `cargo clippy`, `cargo fmt --check`.

## Verification checklist

- [x] `cargo build` succeeds with no warnings.
- [x] `cargo test` passes.
- [x] `cargo clippy` is clean.
- [x] `cargo fmt` applied.
- [x] All acceptance criteria in the spec are met.
