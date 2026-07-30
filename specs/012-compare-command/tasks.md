# Tasks: `nv compare` command

- **Spec:** [spec.md](./spec.md)
- **Status:** Implemented

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

## Verification

```sh
cargo build      # no warnings
cargo test       # all green
cargo clippy     # clean
cargo fmt        # formatted
```
