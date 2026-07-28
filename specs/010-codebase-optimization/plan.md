# Plan: Codebase Optimization

- **Spec:** [spec.md](./spec.md)
- **Status:** Draft

## Overview

Refactor the codebase in three phases: (1) extract shared utilities to eliminate duplication, (2) optimize memory and speed in hot paths, (3) cleanup unused code and minor issues. Every change is behavior-preserving — no user-facing differences.

## Architecture & modules

| Module | Change | Responsibility |
| --- | --- | --- |
| `src/display.rs` | **new** | Shared tree renderer (`├──`/`└──`/`│` with colors) |
| `src/context.rs` | edit | Add `colors()` method, `mark_false_alarm()`, `find_target()` |
| `src/config.rs` | edit | Optimize `save()` allocation |
| `src/model.rs` | edit | Possibly host `find_target()` (depends on types) |
| `src/edit.rs` | edit | Compute diff once, use shared renderer, `&ColorConfig` |
| `src/parser/dotenv.rs` | edit | `Cow<str>` for `unquote()`, shared `detect_newline()` |
| `src/parser/yaml.rs` | edit | `Cow<str>` for `unquote()`, shared `detect_newline()` |
| `src/parser/mod.rs` | edit | Export `detect_newline()` from parser module |
| `src/secret.rs` | edit | Cache `Vec<char>` in `SecretSpec` |
| `src/cli/leaks.rs` | edit | Use shared renderer, `colors()`, `mark_false_alarm()`, `OnceLock` regex |
| `src/cli/fake_secrets.rs` | edit | Use shared renderer, `colors()`, `mark_false_alarm()`, `OnceLock` regex |
| `src/cli/unused.rs` | edit | Use shared renderer, `colors()`, `find_target()`, avoid HashSet clones |
| `src/cli/duplicates.rs` | edit | Use shared renderer, `colors()` |
| `src/cli/find.rs` | edit | Use shared renderer, `colors()` |
| `src/cli/encrypt.rs` | edit | Merge encrypt/decrypt, use shared `find_target()`, `colors()` |
| `src/cli/set.rs` | edit | Use `colors()` |
| `src/cli/generate.rs` | edit | Use `colors()` |
| `src/cli/remove.rs` | edit | Use `colors()` |
| `Cargo.toml` | edit | Remove `rand_chacha`, `thiserror` |

## Data flow

No data flow changes. This is purely a code restructuring:

```
Before:  Command A ──→ duplicated tree renderer
         Command B ──→ duplicated tree renderer
         Command C ──→ duplicated tree renderer

After:   Command A ──→ shared display::render_tree()
         Command B ──→ shared display::render_tree()
         Command C ──→ shared display::render_tree()
```

## Key decisions & trade-offs

- **`display.rs` builds `String`** — Because: matches existing pattern used by all 6 callers. `Write` trait rejected as over-engineered for CLI output. (Spec: requirement 1, open question resolved)

- **`colors()` returns `ColorConfig` by value** — Because: `ColorConfig` is 7 small enums (7 bytes with serde default). Copying is cheaper than threading lifetime parameters through all display functions. Will revisit if `ColorConfig` grows. (Spec: requirement 2, 9)

- **`OnceLock` over `lazy_static!`** — Because: standard library, no extra dependency. (Spec: requirement 8)

- **`Cow<str>` for `unquote()`** — Because: most env values are unquoted; avoids allocation on the common path. (Spec: requirement 11)

- **`mark_false_alarm()` in `context.rs`** — Because: it needs `Config` and `PathBuf` resolution, both already in `context`. (Spec: requirement 3)

- **`find_target()` in `model.rs`** — Because: it takes `&[Service]` which is a model type, and returns `&EnvFile` which is also a model type. (Spec: requirement 4)

## Dependencies

No new crates. Removing two unused:
- `rand_chacha` — listed in Cargo.toml but never imported.
- `thiserror` — listed in Cargo.toml but never imported.

## Risks & mitigations

- **Risk:** Shared tree renderer breaks existing output format. — **Mitigation:** Existing tests validate output. Run full test suite after each display refactor.

- **Risk:** `Cow<str>` lifetime complications in parsers. — **Mitigation:** Both `unquote()` callers immediately use the result as `&str` or pass to `String`-building. Low risk.

- **Risk:** Removing `rand_chacha` breaks something unseen. — **Mitigation:** `cargo build` after removal; if it fails, the crate is actually used.

## Testing strategy

- **Unit tests:** All 102 existing tests must pass unchanged. No new tests needed (this is a refactor).
- **Integration:** `cargo build`, `cargo clippy`, `cargo test`, `cargo fmt` after each phase.
- **Manual:** Run `nv leaks`, `nv unused`, `nv find` against the POL project to verify output format is preserved.

## Rollout / migration

No config or file-format changes. No user-facing behavior changes. This is a pure internal refactor.
