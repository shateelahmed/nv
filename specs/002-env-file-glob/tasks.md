# Tasks: Support `.env*` file glob

- **Spec:** [spec.md](./spec.md)
- **Plan:** [plan.md](./plan.md)

## Tasks

- [ ] **T1 — Simplify `detect_kind()` in `src/parser/mod.rs`**
  - Scope: Replace enumerated `.env` / `.env.example` / `.env.local` checks
    with a single `.env*` match that excludes swap/backup suffixes.
  - Files: `src/parser/mod.rs`
  - Verify: `cargo test` — existing tests pass; new tests in T2 pass.

- [ ] **T2 — Add `detect_kind()` tests for `.env*` variants**
  - Scope: Test `.env.local`, `.env.testing.example`, `.env.swp`, `.env~`,
    `.env.` (trailing dot), `.env.foo.bar`.
  - Files: `src/parser/mod.rs` (in `#[cfg(test)] mod tests`)
  - Verify: `cargo test parser`

- [ ] **T3 — Update documentation**
  - Scope: Change `.env`, `.env.example` references to `.env*` in CLAUDE.md,
    `.github/copilot-instructions.md`, `README.md`, and prompt files.
  - Files: `CLAUDE.md`, `.github/copilot-instructions.md`, `README.md`,
    `.github/prompts/*.md`, `.claude/commands/*.md`
  - Verify: grep for stale `.env.example` references; all say `.env*`.

- [ ] **T4 — Verify**
  - Scope: Run full build and lint suite.
  - Verify: `cargo build`, `cargo test`, `cargo clippy`, `cargo fmt`

## Verification checklist

- [ ] `cargo build` succeeds with no warnings.
- [ ] `cargo test` passes.
- [ ] `cargo clippy` is clean.
- [ ] `cargo fmt` applied.
- [ ] All acceptance criteria in the spec are met.
