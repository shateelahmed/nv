# Tasks: `nv encode` and `nv decode` commands

- **Spec:** [spec.md](./spec.md)
- **Plan:** [plan.md](./plan.md)

Work through tasks top to bottom. Keep each task small enough to complete and
verify independently. Mark `[x]` when done.

## Tasks

- [x] **T1 — Extract the shared sensitive key-name pattern in `leaks.rs`**
  - Scope: Hoist the key-name portion of `leak_pattern()` into a
    `SENSITIVE_KEY_NAME` const and build the line regex from it, so `leaks` and
    the new command share one definition. Add `pub fn is_sensitive_key_name()`.
    No behavior change to `nv leaks`.
  - Files: `src/cli/leaks.rs`
  - Verify: `cargo test cli::leaks` — existing tests still pass; new tests cover
    `JWT_SECRET` accepted, `APP_NAME` rejected.

- [x] **T2 — Add `dotenv::rename_key`**
  - Scope: New `pub fn rename_key(content, key, new_key) -> String` that rewrites
    only the key substring on **every** matching assignment line, preserving
    indentation, `export ` prefix, value, inline comment, CRLF, and the trailing
    newline. Absent key returns content unchanged.
  - Files: `src/parser/dotenv.rs`
  - Verify: `cargo test parser::dotenv` — formatting-preservation tests.

- [x] **T3 — Add `Encode` / `Decode` CLI variants**
  - Scope: Add both variants to `Command` (no extra args; they use the global
    `-s`/`--all`/`--file`). Dispatch to `encode::run_encode` and
    `encode::run_decode`. Add `mod encode;`.
  - Files: `src/cli/mod.rs`
  - Verify: `cargo build`; `nv encode --help` lists the command.

- [x] **T4 — Implement `run_encode`**
  - Scope: Collect `DotenvExample` targets, reject other `--file` kinds, select
    sensitive keys (shared predicate + `special_secret_keys`, minus
    `false_alarms`, minus already-`ENC.`-prefixed), encode each to
    `ENC.` + `URL_SAFE_NO_PAD`, fold `rename_key` per key into a `ChangeSet`, and
    hand it to `preview_and_apply`. Empty case prints
    `No sensitive keys to encode.`
  - Files: `src/cli/encode.rs`
  - Verify: `cargo test cli::encode`.

- [x] **T5 — Implement `run_decode`**
  - Scope: Find `ENC.`-prefixed keys, decode with `URL_SAFE_NO_PAD`, and validate
    before writing: invalid base64, a decoded name that is not a valid dotenv
    key, or a collision with an existing key are all collected and reported
    together, after which nothing is written and the command exits non-zero.
    Otherwise rename back and `preview_and_apply`. Empty case prints
    `No encoded keys to decode.`
  - Files: `src/cli/encode.rs`
  - Verify: `cargo test cli::encode`; malformed-input cases abort with no writes.

- [x] **T6 — Document both commands in README**
  - Scope: Two rows in the commands table and examples under `# Examples`.
  - Files: `README.md`

- [x] **T7 — Sync assistant configs (spec 001)**
  - Scope: Add the `ENC.` golden rule to `CLAUDE.md` and
    `.github/copilot-instructions.md`. Qualify the "no base64" constraint in
    `.claude/commands/implement.md` and `.github/prompts/implement.prompt.md` so
    it reads as applying to Kubernetes `data:` values only.
  - Files: `CLAUDE.md`, `.github/copilot-instructions.md`,
    `.claude/commands/implement.md`, `.github/prompts/implement.prompt.md`

- [x] **T8 — End-to-end verification**
  - Scope: On a scratch service, run `nv encode`, confirm `nv leaks` no longer
    reports the key, then `nv decode` and confirm the original name returns.
    Confirm `--dry-run` writes nothing and configmap files stay byte-identical.
  - Verify: manual runs plus the checklist below.

## Verification checklist

- [x] `cargo build` succeeds with no warnings.
- [x] `cargo test` passes (318 unit + 13 integration).
- [x] `cargo clippy` is clean.
- [x] `cargo fmt` applied.
- [x] All acceptance criteria in the spec are met.