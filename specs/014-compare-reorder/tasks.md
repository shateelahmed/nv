# Tasks: `nv compare --reorder`

- **Spec:** [spec.md](./spec.md)
- **Plan:** [plan.md](./plan.md)

Work through tasks top to bottom. Keep each task small enough to complete and
verify independently. Mark `[x]` when done.

## Tasks

- [x]  **T1 — Shared reorder scanner (`src/parser/reorder.rs`)**
  - Scope: New module with the generic region scanner (key units + orphan
    collection per the plan's scan rules) and the placement step (orphans pinned
    at absolute indexes, remaining positions filled with the permuted unit
    lines). Expose a `ScanRules` struct (key detection, comment-attachment,
    commented-out detection) and `reorder_region(lines, rules, order) -> Vec<String>`.
    Include the exact-output regression test for orphan + variable-height-unit
    interleaving.
  - Files: `src/parser/reorder.rs` (+ `pub mod reorder;` in `src/parser/mod.rs`)
  - Verify: `cargo test reorder` passes the scanner unit tests; `cargo build` clean.

- [x]  **T2 — Dotenv reorder entry point**
  - Scope: `dotenv::reorder(content, order)` that splits lines, scans the whole
    file as one region with dotenv rules (`parse_line` keys, any-indent prose
    comments attach), permutes, and rejoins with preserved newline/trailing
    newline. Unit tests: basic reorder, extras to bottom with relative order
    kept, attached comment moves with key, blank-separated header stays,
    commented-out assignment stays and breaks attachment, already-in-order
    returns identical content, CRLF preserved, trailing newline preserved,
    duplicate keys use first occurrence.
  - Files: `src/parser/dotenv.rs`
  - Verify: `cargo test dotenv`; formatting-preservation tests green.

- [x]  **T3 — YAML reorder entry point**
  - Scope: `yaml::reorder(content, order)` handling flat shape (whole-file
    region, top-level keys only, empty-value keys excluded) and k8s shape
    (per-`stringData:`/`data:` block regions, direct children only, child-indent
    comment attachment, no cross-block moves, block header/indentation
    untouched). Unit tests covering both shapes.
  - Files: `src/parser/yaml.rs`
  - Verify: `cargo test yaml`; formatting-preservation tests green.

- [x]  **T4 — Parser dispatch in `mod.rs`**
  - Scope: `parser::reorder(content, kind, order) -> String` dispatching to
    dotenv/YAML like `set_value`. Smoke test that it wires through
    `parser::parse`-recognized key sets.
  - Files: `src/parser/mod.rs`
  - Verify: `cargo test parser`; `cargo build`.

- [x]  **T5 — `ChangeSet::reorder` in `src/edit.rs`**
  - Scope: Builder that reads each target (skipping missing/unreadable files),
    computes `parser::reorder(content, kind, order)`, and pushes a `FileChange`
    (noop changes filtered by the existing `effective()`). Unit tests: effective
    changes only for differing files; unreadable peers skipped; `apply` writes
    only changed peers.
  - Files: `src/edit.rs`
  - Verify: `cargo test edit`; `cargo build`.

- [x]  **T6 — CLI flag and dispatch**
  - Scope: Add `--reorder` to the `Compare` subcommand with
    `conflicts_with_all = ["values", "order", "comments"]` and wire
    `compare::run(&cli, file_path, *values, *order, *comments, *reorder)`.
  - Files: `src/cli/mod.rs`
  - Verify: `cargo build`; `nv compare --reorder --values` exits 2 with a clap
    error.

- [x]  **T7 — `compare::run` reorder branch**
  - Scope: Extend `run` signature with `reorder: bool`. After the banner and
    before base resolution, bail `--reorder requires --service to identify the
    base file's service.` when `cli.services.is_empty()`. When `reorder`, build
    the base order (first occurrence dedup), collect peer targets (base service,
    `peer_kinds`, exclude base file and `compare.skip_files` matches), build the
    `ChangeSet` via `ChangeSet::reorder`, and call `context::preview_and_apply`.
    Unit tests for the order-building dedup and target selection helpers.
  - Files: `src/cli/compare.rs`
  - Verify: `cargo test compare`; smoke test against a temp service for
    reorder, `--dry-run`, declined confirm, `--yes`, and the no-`--service`
    error.

- [x]  **T8 — README documentation**
  - Scope: Document `nv compare --reorder` under the compare examples.
  - Files: `README.md`
  - Verify: `cargo build`; docs read correctly.

- [x]  **T9 — Full verification**
  - Scope: Run the standard checks and walk the spec's acceptance criteria.
  - Files: —
  - Verify: `cargo build` (no warnings), `cargo test`, `cargo clippy`,
    `cargo fmt --check`; all acceptance criteria met.

## Verification checklist

- [x]  `cargo build` succeeds with no warnings.
- [x]  `cargo test` passes.
- [x]  `cargo clippy` is clean.
- [x]  `cargo fmt` applied.
- [x]  All acceptance criteria in the spec are met.
