# Tasks: `special_secret_keys` for `nv leaks` and `nv fake-secrets`

- **ID:** 011-special-keys
- **Status:** Implemented
- **Author:** shateel
- **Date:** 2026-07-29

## Task list

- [x] **T1: Add `special_secret_keys` field to `LeakConfig`**
  - File: `src/config.rs`
  - Add `special_secret_keys: Vec<String>` to `LeakConfig`.
  - Verify: `cargo build` (no warnings).

- [x] **T2: Add `special_secret_keys_for()` method to `Config`**
  - File: `src/config.rs`
  - Returns union of global + per-service `special_secret_keys`.
  - Unit tests: global, per-service, merge, empty.
  - Verify: `cargo test`.

- [x] **T3: Add special-secret-keys scan pass in `leaks.rs`**
  - File: `src/cli/leaks.rs`
  - Dynamic regex from escaped keys, `captures_iter`, merge with regex pass.
  - Verified: `cargo build` (no warnings).

- [x] **T4: Implement `special_secret_keys` in `fake_secrets.rs`**
  - File: `src/cli/fake_secrets.rs`
  - Retrieve merged list per service; treat listed keys as secret keys.
  - Verified: `cargo build`.

- [x] **T5: Update `nv.yml.example`**
  - Document `special_secret_keys` in global and per-service sections.

- [x] **T6: Write unit tests**
  - `config.rs`: 4 tests for `special_secret_keys_for`.
  - `leaks.rs`: 5 tests for `special_secret_key_pattern` regex matching.
  - Verify: `cargo test` (all pass).

## Verification

```sh
cargo build      # no warnings
cargo test       # all green
cargo clippy     # clean
cargo fmt        # formatted
```
