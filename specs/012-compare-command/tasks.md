# Tasks: `nv compare` command

- **Spec:** [spec.md](./spec.md)
- **Status:** Implemented
- **Updated:** 2026-07-31

## Task list

- [x] **T1: Register `Compare` variant in `Command` enum and dispatch**
  - File: `src/cli/mod.rs`
  - Add `Compare { file_path: String, values: bool }` variant.
  - Add dispatch: `Some(Command::Compare { file_path, values }) => compare::run(&cli, file_path, *values)`.
  - Add `pub mod compare;` to module declarations.
  - Verify: `cargo build` (new file can be empty placeholder).

- [x] **T2: Create `compare.rs` — core diff logic and render**
  - File: `src/cli/compare.rs`
  - Implement `peer_kinds(base_kind)` returning `Vec<FileKind>` per spec table.
  - Implement `diff_keys(base_pairs, peer_pairs)` returning diff items
    (missing, extra, and optionally value-different).
  - Implement `run(cli, file_path, values)`:
    1. Resolve base file from path (search services).
    2. Parse base file → `Vec<ParsedPair>`.
    3. Collect comparator files (same peer kinds, excluding base).
    4. For each peer: parse, diff, collect into service‑grouped structure.
    5. Render via `render_tree`.
  - Verify: `cargo build`.

- [x] **T3: Write unit tests for helper functions**
  - File: `src/cli/compare.rs` `#[cfg(test)] mod tests`
  - Test `peer_kinds` for each variant.
  - Test `diff_keys` with key-only mode.
  - Test `diff_keys` with `--values` mode.
  - Test empty base file, empty peer file.
  - Verify: `cargo test`.

- [x] **T4: Add `CompareConfig` with `skip_files` to `nv.yml` schema**
  - File: `src/config.rs`
  - Add `CompareConfig { skip_files: Vec<String> }`.
  - Register `compare: Option<CompareConfig>` on `CommandsConfig`.
  - Add a merge helper (global + per-service) analogous to
    `special_secret_keys_for`, e.g. `Config::compare_skip_files_for(&service)`.
  - Verify: `cargo build`.

- [x] **T5: Apply `skip_files` filtering to peer files**
  - File: `src/cli/compare.rs`
  - Import `glob::Pattern`; add a `matches_skip_pattern(relative, name, skip_files)`
    helper matching `unused.rs`.
  - In `run`, build the merged skip set per service and skip peer files that match.
  - Never filter the base file.
  - Verify: `cargo build`, `cargo test`.

- [x] **T6: Rework the not-found error to list available files in tree format**
  - File: `src/cli/compare.rs`
  - Change message to `file '<path>' not found.` (drop "in any service").
  - Build `TreeService`/`TreeFile` (empty items) from discovered files and
    render via `display::render_tree` to `Output::Stderr` before returning the
    error.
  - Verify: `cargo build`, `cargo test`.

- [x] **T7: Unit tests for skip_files and error output**
  - File: `src/cli/compare.rs` `#[cfg(test)] mod tests`
  - Test `matches_skip_pattern` for relative path, file name, and glob patterns.
  - Test that `diff_pairs` is unaffected (skip is applied before diffing).
  - Verify: `cargo test`.

- [x] **T8: Update `nv.yml.example` with `commands.compare.skip_files` docs**
  - File: `nv.yml.example`
  - Document global + per-service `compare.skip_files`.
  - Verify: `cargo build`.

- [x] **T9: Print the not-found error before the file tree; scope the tree to `--service`**
  - File: `src/cli/compare.rs`
  - Render the available-files tree into a `String` and embed it in the
    `file '<path>' not found.` error so the message prints first.
  - Filter the tree by `service_filter` when `--service` is provided.
  - Add an `Available files:` header between the error and the tree (omitted
    when the tree is empty).
  - Update spec/plan to document the behavior.
  - Verify: `cargo build`, `cargo test`, manual smoke test.

- [x] **T10: Hide file counts in the available-files tree**
  - File: `src/display.rs`, `src/cli/compare.rs`
  - Add a `show_file_counts` parameter to `render_tree`; pass `false` for the
    available-files listing so bare file names appear without `(count)`.
  - Other commands pass `true` (unchanged output).
  - Verify: `cargo build`, `cargo test`, `cargo clippy`, `cargo fmt`.

- [x] **T11: Error when the requested base file is under `skip_files`**
  - File: `src/cli/compare.rs`
  - Add a `file_is_skipped` helper (reused by the peer loop).
  - After base-file resolution, if the base file matches its service's merged
    `skip_files`, error with `file '<path>' is excluded by compare.skip_files.`
  - The error's `Available files:` tree lists only files of the same kind and
    omits the requested file; `--service` filtering still applies.
  - The header names the kind via `kind_label`: `Available env files:`,
    `Available configmap files:`, `Available secrets files:`.
  - Update spec/plan to document the behavior.
  - Verify: `cargo build`, `cargo test`, `cargo clippy`, `cargo fmt`,
    manual smoke test.

- [x] **T12: Omit `skip_files` matches from the error's available-files tree**
  - File: `src/cli/compare.rs`
  - `available_files_tree` now takes `&Option<Config>` and drops files that
    match their service's merged `compare.skip_files` set (they cannot be
    compared, so they are not "available"). Applies to both the
    not-found and the skipped-base error listings.
  - Add unit tests: skipped file omitted from the tree; kind-filtered tree
    still respects per-service skip patterns.
  - Verify: `cargo build`, `cargo test` (140), `cargo clippy`, `cargo fmt`,
    manual smoke test of both error paths.

- [x] **T13: Add `--order` key-order comparison**
  - File: `src/cli/mod.rs`, `src/cli/compare.rs`
  - Add `order: bool` to `Command::Compare` with `#[arg(long,
    conflicts_with = "values")]` and dispatch it to
    `compare::run(&cli, file_path, values, order)`.
  - Add a `DiffItem::OutOfOrder { key, base_pos, peer_pos }` variant.
  - Implement `order_diffs(base_pairs, peer_pairs)`: walk the peer, report a
    key present in both files whose 1-based base position is smaller than the
    largest base position already seen (first occurrence of duplicates).
  - Add `order_label(prefix, key, position, color, use_color)` producing
    `- KEY (#N)` / `+ KEY (#N)` labels.
  - Extend the diff list with `order_diffs` results when `--order` is set;
    render `OutOfOrder` as a `-`/`+` pair mirroring the `Different` pair.
  - Add unit tests: same order empty, swapped pair, reversed, missing/extra
    ignored, duplicate first-occurrence, label with/without color.
  - Update spec/plan to document the behavior (including `--values`/`--order`
    mutual exclusion).
  - Verify: `cargo build`, `cargo test` (147), `cargo clippy`, `cargo fmt`,
    manual smoke test (`--order` alone, and combined `--values --order` error).

## Verification

```sh
cargo build      # no warnings
cargo test       # all green
cargo clippy     # clean
cargo fmt        # formatted
```
