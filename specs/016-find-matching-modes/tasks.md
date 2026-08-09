# Tasks: `nv find` strict keyword and pattern matching

- **Spec:** [spec.md](./spec.md)
- **Plan:** [plan.md](./plan.md)

Work through tasks top to bottom. Keep each task small enough to complete and
verify independently. Mark `[x]` when done.

## Tasks

- [x] **T1 — Add `--exact` / `--pattern` to the `Find` command**
  - Scope: Add `exact: bool` and `pattern: Option<String>` fields to the
    `Find` variant in `Command`, wired with clap
    (`#[arg(long)]` for `--exact`, `#[arg(long, value_name = "GLOB")]` for
    `--pattern`, and `conflicts_with` on one of them). Update the `Find { query
    }` pattern in `run()` to destructure the new fields and pass them to
    `find::run`.
  - Files: `src/cli/mod.rs`
  - Verify: `cargo build` (no warnings); `nv find --exact X --pattern 'Y*'`
    exits non-zero with a conflict message; `nv find --help` lists both flags.

- [x] **T2 — Implement `search_exact` and `search_glob`**
  - Scope: In `src/search.rs`, add `search_exact(index, query) ->
    Vec<&EnvKey>` (whole-key ASCII case-insensitive equality, index order,
    empty query returns all) and `search_glob(index, query) -> Result<Vec<&EnvKey>>`
    (lowercase pattern + key, `glob::Pattern` match on the whole key name,
    invalid glob → `Err`). Add `#[cfg(test)] mod tests` covering whole-key
    match, case-insensitivity, no-substring (`URL` vs `DATABASE_URL`), `DB_*`,
    `*_URL`, `*` matches all, empty query, and invalid glob.
  - Files: `src/search.rs`
  - Verify: `cargo test search::tests` — new tests pass; existing fuzzy tests
    still pass.

- [x] **T3 — Route matching modes in `find::run`**
  - Scope: Accept the new flags in `find::run`'s signature. When `exact` is
    set call `search_exact`; when `pattern` is `Some` validate the glob up
    front (via `search_glob`, mapping `Err` to an `anyhow!` error before any
    output) and call it; otherwise keep `search::search`. `No matches.` and
    tree rendering stay unchanged.
  - Files: `src/cli/find.rs`
  - Verify: `cargo test` — all green; manual on a scratch tree:
    `find --exact DATABASE_URL` vs `find --exact URL`, `find --pattern 'DB_*'`,
    `find --pattern '['` errors, `find --exact -s auth KEY` scopes.

- [x] **T4 — Update README find documentation**
  - Scope: Document `--exact` and `--pattern` in the find section and add
    example commands (`nv find --exact DATABASE_URL`, `nv find --pattern
    'DB_*'`, `nv find -s auth --pattern '*_URL'`).
  - Files: `README.md`
  - Verify: docs read cleanly; examples match actual flag behavior.

- [x] **T5 — Verify the feature end-to-end**
  - Scope: Full test suite, clippy, fmt, and manual acceptance-criteria checks
    on `pol-payment-core-ms` (or a scratch multi-service tree): `--exact`
    whole-key only, `--pattern` case-insensitive glob, flag conflict error,
    invalid glob error, `-s` combination, `skip_files` exclusion, and fuzzy
    default unchanged.
  - Verify: `cargo build` (no warnings), `cargo test`, `cargo clippy`,
    `cargo fmt --check`, all acceptance criteria from the spec met.

## Verification checklist

- [x] `cargo build` succeeds with no warnings.
- [x] `cargo test` passes.
- [x] `cargo clippy` is clean.
- [x] `cargo fmt` applied.
- [x] All acceptance criteria in the spec are met.
