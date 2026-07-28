# Tasks: Codebase Optimization

- **Spec:** [spec.md](./spec.md)
- **Plan:** [plan.md](./plan.md)

Work through tasks top to bottom. Keep each task small enough to complete and verify independently. Mark `[x]` when done.

## Phase 1 — DRY (shared utilities)

- [ ] **T1 — Extract `display::render_tree()`**
  - Scope: Create `src/display.rs` with a shared tree renderer that outputs `├──`/`└──`/`│` lines with configurable colors. Accepts a nested data structure: `Vec<(service_name, Vec<(file_display, Vec<(key, value)>)>)>` plus `&ColorConfig`.
  - Files: `src/display.rs` (new), `src/main.rs` (add `mod display`)
  - Verify: `cargo build` passes. No callers yet — just the module and unit tests for empty/single/multi-item trees.

- [ ] **T2 — Refactor `leaks.rs` to use `display::render_tree()`**
  - Scope: Replace `print_leaks()` body with a call to `display::render_tree()`. Remove the local tree-rendering loop.
  - Files: `src/cli/leaks.rs`
  - Verify: `cargo test cli::leaks` passes. Output format unchanged.

- [ ] **T3 — Refactor `fake_secrets.rs` to use `display::render_tree()`**
  - Scope: Replace `print_fake_secrets()` body with a call to `display::render_tree()`.
  - Files: `src/cli/fake_secrets.rs`
  - Verify: `cargo test cli::fake_secrets` passes.

- [ ] **T4 — Refactor `duplicates.rs` to use `display::render_tree()`**
  - Scope: Replace `print_duplicates()` body with a call to `display::render_tree()`.
  - Files: `src/cli/duplicates.rs`
  - Verify: `cargo test cli::duplicates` passes.

- [ ] **T5 — Refactor `find.rs` to use `display::render_tree()`**
  - Scope: Replace `print_hierarchical_output()` body with a call to `display::render_tree()`.
  - Files: `src/cli/find.rs`
  - Verify: `cargo test cli::find` passes.

- [ ] **T6 — Refactor `unused.rs` to use `display::render_tree()`**
  - Scope: Replace `print_unused_keys()` body with a call to `display::render_tree()`.
  - Files: `src/cli/unused.rs`
  - Verify: `cargo test cli::unused` passes.

- [ ] **T7 — Refactor `edit.rs::render_diff()` to use `display::render_tree()`**
  - Scope: Replace the manual tree construction in `render_diff()` with calls to `display::render_tree()`.
  - Files: `src/edit.rs`
  - Verify: `cargo test edit` passes.

- [ ] **T8 — Add `Context::colors()` method**
  - Scope: Add `pub fn colors(&self) -> ColorConfig` to `Context` in `context.rs`. Replace all 10+ inline extraction blocks across CLI commands with `ctx.colors()`.
  - Files: `src/cli/context.rs`, `src/cli/leaks.rs`, `src/cli/fake_secrets.rs`, `src/cli/unused.rs`, `src/cli/duplicates.rs`, `src/cli/find.rs`, `src/cli/encrypt.rs`, `src/cli/set.rs`, `src/cli/generate.rs`, `src/cli/remove.rs`
  - Verify: `cargo build` passes. `grep -r "should_use_color" src/cli/` shows only `context.rs`.

- [ ] **T9 — Extract `mark_false_alarm()` to `context.rs`**
  - Scope: Move the `mark_false_alarm()` function from `leaks.rs` to `context.rs` as a public method or free function. Update both `leaks.rs` and `fake_secrets.rs` to call it.
  - Files: `src/cli/context.rs`, `src/cli/leaks.rs`, `src/cli/fake_secrets.rs`
  - Verify: `cargo test` passes. No duplicate `mark_false_alarm` function exists.

- [ ] **T10 — Extract `find_target()` to `model.rs`**
  - Scope: Move `find_target()` from `unused.rs` to `model.rs` as a public function. Update both `unused.rs` and `encrypt.rs` to call it.
  - Files: `src/model.rs`, `src/cli/unused.rs`, `src/cli/encrypt.rs`
  - Verify: `cargo test` passes. No duplicate `find_target` function exists.

- [ ] **T11 — Merge encrypt/decrypt in `encrypt.rs`**
  - Scope: Consolidate `run_encrypt` and `run_decrypt` into a single `run_transform(cli, keys, transform_fn)` function where `transform_fn: fn(&str, &str) -> Result<String>`.
  - Files: `src/cli/encrypt.rs`
  - Verify: `cargo test cli::encrypt` passes.

- [ ] **T12 — Extract `detect_newline()` to parser module**
  - Scope: Add `pub fn detect_newline(content: &str) -> &str` to `src/parser/mod.rs`. Replace the 4 duplicated `contains("\r\n")` blocks in `dotenv.rs` and `yaml.rs`.
  - Files: `src/parser/mod.rs`, `src/parser/dotenv.rs`, `src/parser/yaml.rs`
  - Verify: `cargo test parser` passes.

## Phase 2 — Memory & Speed

- [ ] **T13 — Compute diff once in `edit.rs`**
  - Scope: In `render_diff()`, compute `TextDiff::from_lines()` once, store in a local, and reuse for both the counting pass and the rendering pass.
  - Files: `src/edit.rs`
  - Verify: `cargo test edit` passes.

- [ ] **T14 — OnceLock for regexes**
  - Scope: Change `leak_pattern()`, `key_value_pattern()`, and `secret_key_pattern()` to use `std::sync::OnceLock<Regex>` so each compiles exactly once.
  - Files: `src/cli/leaks.rs`, `src/cli/fake_secrets.rs`
  - Verify: `cargo test` passes.

- [ ] **T15 — Avoid HashSet clones in `unused.rs`**
  - Scope: In the `run()` function and `collect_all_keys()`, avoid cloning `skip_dirs` and `skip_files` per service. Instead, build the merged set once before the loop or pass references.
  - Files: `src/cli/unused.rs`
  - Verify: `cargo test cli::unused` passes.

- [ ] **T16 — `Cow<str>` for `unquote()`**
  - Scope: Change `unquote()` in both `dotenv.rs` and `yaml.rs` to return `Cow<'_, str>`. Update callers to handle `Cow`.
  - Files: `src/parser/dotenv.rs`, `src/parser/yaml.rs`
  - Verify: `cargo test parser` passes.

- [ ] **T17 — Cache charset `Vec<char>` in `SecretSpec`**
  - Scope: Add a `chars: Vec<char>` field to `SecretSpec` (built at construction from `charset`). Use it in `generate()` instead of re-collecting.
  - Files: `src/secret.rs`
  - Verify: `cargo test secret` passes.

## Phase 3 — Cleanup

- [ ] **T18 — Remove unused dependencies**
  - Scope: Remove `rand_chacha` and `thiserror` from `Cargo.toml` `[dependencies]`.
  - Files: `Cargo.toml`
  - Verify: `cargo build` passes. `cargo test` passes.

- [ ] **T19 — Fix `.unwrap()` outside tests**
  - Scope: Replace `std::env::current_dir().unwrap()` in `leaks.rs` and `fake_secrets.rs` with `?` propagation (the enclosing functions already return `Result`).
  - Files: `src/cli/leaks.rs`, `src/cli/fake_secrets.rs`
  - Verify: `cargo clippy` passes. `cargo test` passes.

- [ ] **T20 — Avoid allocation in `is_probably_text()`**
  - Scope: Replace `s.to_owned() + "."` with direct string comparison against the `TEXT_FILENAMES` slice (compare `s` directly, not `s.`).
  - Files: `src/cli/unused.rs`
  - Verify: `cargo test cli::unused` passes.

- [ ] **T21 — Optimize `config::save()` allocation**
  - Scope: Replace `.collect::<Vec<_>>().join("\n")` with `intersperse()` on the iterator or a `write!`-based approach to avoid the intermediate `Vec`.
  - Files: `src/config.rs`
  - Verify: `cargo test config` passes.

## Verification checklist

- [ ] `cargo build` succeeds with no warnings.
- [ ] `cargo test` passes (all 102+ tests).
- [ ] `cargo clippy` is clean.
- [ ] `cargo fmt` applied.
- [ ] No tree-rendering code is duplicated across more than one file.
- [ ] No function appears identically in two files.
- [ ] The `colors()` extraction pattern appears exactly once (in `Context`).
- [ ] All acceptance criteria in the spec are met.
