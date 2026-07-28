# Spec: Codebase Optimization

- **ID:** 010-codebase-optimization
- **Status:** Draft
- **Author:** opencode
- **Date:** 2026-07-28

## Summary

Refactor and optimize the nv codebase to eliminate duplicated code, reduce unnecessary allocations, and improve maintainability. This is a code-quality and performance improvement with no user-facing behavior changes.

## Problem / Motivation

The codebase has grown organically across 9 feature specs. Patterns emerged that were copy-pasted rather than shared:

- Tree-style output rendering is implemented 6 separate times.
- "Get colors from context" is repeated 10+ times across every CLI command.
- `mark_false_alarm()` and `find_target()` are duplicated verbatim.
- Regex patterns are recompiled on every command invocation.
- `HashSet` clones happen per-service in the unused command.
- `ColorConfig` is cloned rather than referenced.
- Unused crate dependencies remain in `Cargo.toml`.

These increase maintenance burden, binary size, and runtime cost.

## Goals

- Eliminate all duplicated functions and patterns across CLI commands.
- Reduce unnecessary heap allocations in hot paths.
- Remove unused dependencies.
- Fix `.unwrap()` calls outside test code.
- Zero behavior changes — all existing tests must pass unchanged.

## Non-goals

- TUI performance optimization (clones there are structurally required by ownership model).
- Rewriting the parser layer or changing file formats.
- Adding new features or CLI flags.

## Requirements

### DRY — Shared Utilities

1. **Tree renderer** — A single `src/display.rs` module MUST provide a function that renders a hierarchical tree with `├──`/`└──`/`│` characters and configurable colors. All 6 display functions (edit, find, leaks, fake_secrets, duplicates, unused) MUST be rewritten to call this shared renderer.

2. **Context::colors()** — `Context` MUST expose a `fn colors(&self) -> ColorConfig` method. All CLI commands MUST use this method instead of the inline 5-line extraction pattern.

3. **mark_false_alarm()** — MUST exist in exactly one place (e.g. `context.rs`). Both `leaks.rs` and `fake_secrets.rs` MUST call the shared function.

4. **find_target()** — MUST exist in exactly one place (e.g. `model.rs` or `context.rs`). Both `unused.rs` and `encrypt.rs` MUST call the shared function.

5. **encrypt/decrypt merge** — `run_encrypt` and `run_decrypt` MUST be consolidated into a single function parameterized by the transform operation.

6. **Newline detection** — `detect_newline(content: &str) -> &str` MUST be extracted and used by both `dotenv.rs` and `yaml.rs`.

### Memory & Speed

7. **Diff computed once** — `render_diff()` in `edit.rs` MUST compute `TextDiff::from_lines()` exactly once per file change and reuse the result for both counting and rendering.

8. **Regex OnceLock** — `leak_pattern()`, `key_value_pattern()`, and `secret_key_pattern()` MUST use `std::sync::OnceLock` to compile each regex exactly once.

9. **ColorConfig by reference** — Display functions that currently accept `ColorConfig` by value MUST accept `&ColorConfig` instead. The `Context::colors()` helper SHOULD return a reference where possible.

10. **Avoid HashSet clones in unused** — The `search_dir` loop in `unused.rs` MUST NOT clone `skip_dirs` or `skip_files` per service. Use references and merge per-service additions into a combined set before the loop, or pass references down.

11. **Cow for unquote()** — Both `dotenv.rs::unquote()` and `yaml.rs::unquote()` MUST return `Cow<str>`, avoiding allocation when the value is not quoted.

### Cleanup

12. **Remove unused deps** — `rand_chacha` and `thiserror` MUST be removed from `Cargo.toml`.

13. **Fix unwrap outside tests** — `std::env::current_dir().unwrap()` in `leaks.rs` and `fake_secrets.rs` MUST be replaced with `?` propagation.

14. **Cache charset Vec** — `secret.rs` MUST build `Vec<char>` from the charset once at `SecretSpec` construction, not on every `generate()` call.

15. **Avoid allocation in is_probably_text()** — `unused.rs` MUST NOT allocate a `String` (`s.to_owned() + "."`) to check special filenames. Use direct string comparison.

16. **save() allocation** — `config.rs::save()` SHOULD avoid the intermediate `Vec<String>` from `.collect().join("\n")` by writing lines directly.

## Acceptance criteria

- [ ] `cargo build` produces zero warnings.
- [ ] `cargo clippy` produces zero warnings.
- [ ] All existing tests pass unchanged (`cargo test`).
- [ ] No tree-rendering code is duplicated across more than one file.
- [ ] No function (other than trait impls) appears identically in two files.
- [ ] The `colors()` extraction pattern appears exactly once (in `Context`).
- [ ] Regex patterns compile at most once per process lifetime.
- [ ] `grep -r "\.clone()" src/cli/` shows fewer clone calls than before the change.
- [ ] Binary size is unchanged or smaller (measured via `cargo build --release`).

## Edge cases

- The shared tree renderer must handle empty service lists, empty file lists, and single-item lists (using `└──` for the last item).
- `detect_newline()` must default to `"\n"` when content has no line endings (empty string).
- `Cow<str>` from `unquote()` must preserve the original borrow lifetime — callers must not assume ownership.

## Open questions

_(None — all resolved.)_
