# Plan: `nv encode` and `nv decode` commands

- **Spec:** [spec.md](./spec.md)
- **Status:** Approved

## Overview

Add `nv encode` and `nv decode`, an inverse pair that rewrites sensitive
environment-variable **key names** in `.env.example` files between their plain
form and a URL-safe unpadded base64 form prefixed with `ENC.`.

The work needs exactly one genuinely new primitive — renaming a key in place
while preserving formatting — plus a command module that reuses the sensitive-key
criteria already implemented for `nv leaks` so the two can never disagree.

No new crates. `base64` 0.23 is already a dependency (used by `src/secret.rs`
and `src/cli/encrypt.rs`).

## Architecture & modules

| Module | Change | Responsibility |
| --- | --- | --- |
| `src/parser/dotenv.rs` | edit | New `rename_key(content, key, new_key) -> String`: rewrite only the key text of every matching assignment line, preserving indent, `export ` prefix, value, and inline comment. |
| `src/cli/leaks.rs` | edit | Extract the sensitive key-name pattern into a single shared `SENSITIVE_KEY_NAME` const, and add `is_sensitive_key_name()` so encode/decode and leaks share one definition. |
| `src/cli/encode.rs` | new | `ENC_PREFIX`, base64 helpers, sensitive-key selection, `run_encode`, `run_decode`, and decode validation. |
| `src/cli/mod.rs` | edit | `Encode` / `Decode` variants on `Command`; dispatch both to `encode::run_encode` / `encode::run_decode`. |
| `README.md` | edit | Two command-table rows and examples. |
| `CLAUDE.md`, `.github/copilot-instructions.md` | edit | Golden rule for the `ENC.` convention (spec 001 sync). |
| `.claude/commands/implement.md`, `.github/prompts/implement.prompt.md` | edit | Qualify the "no base64" constraint so it does not block this feature. |

No new crates.

## Data flow

### `nv encode`

```
nv encode [-s SERVICE ...] [--all]
       │
       ▼
  context::resolve(cli)  →  Context  (+ print_banner)
       │
       ▼
  reject --file kinds other than dotenv_example
       │
       ▼
  service_filter → collect_targets(services, filter, [DotenvExample])
       │
       ▼
  per service: is_sensitive_key_name(key, special_keys) minus false_alarms
               minus keys already starting with "ENC."
       │
       ▼
  per file: parse keys → new_key = "ENC." + URL_SAFE_NO_PAD(key)
            new_content = fold dotenv::rename_key(old, key, new_key)
       │
       ▼
  ChangeSet (only files whose content actually changed)
       │
       ▼
  context::preview_and_apply  →  diff → --dry-run? stop → -y? apply
```

### `nv decode`

```
nv decode [-s SERVICE ...] [--all]
       │
       ▼
  same target collection
       │
       ▼
  per file: keys starting with "ENC."
            strip prefix → URL_SAFE_NO_PAD.decode
            on failure, or decoded name not a valid dotenv key,
            or decoded name already present  → record problem
       │
       ▼
  if any problem: print every problem, write NOTHING, exit non-zero
       │
       ▼
  otherwise rename each back and preview_and_apply
```

## Key decisions & trade-offs

- **Decision:** URL-safe **unpadded** base64 — **Because:** dotenv keys are split
  at the first `=`, so padded output corrupts the line; verified against the real
  binary that `ENC.SldUX1NFQ1JFVA==x` parses as key `ENC.SldUX1NFQ1JFVA` with
  value `=x`. URL-safe unpadded never emits `=`, `+`, or `/`.
  **Alternatives:** `STANDARD` (rejected: corrupts), `STANDARD_NO_PAD` (rejected:
  can still emit `+` and `/`, which are invalid in a dotenv key).
- **Decision:** put the key-name regex in one shared const used by both `leaks`
  and `encode` — **Because:** the spec requires `encode` to produce exactly the
  key set `leaks` reports; a duplicated regex would silently drift.
  **Alternatives:** copy the pattern into `encode.rs` (rejected: two sources of
  truth), import `fake_secrets.rs`'s copy (rejected: it is a separate whole-line
  criterion and refactoring it is out of scope).
- **Decision:** add `rename_key` to `src/parser/dotenv.rs` and call it as
  `parser::dotenv::rename_key`, with no `parser::rename_key` dispatcher —
  **Because:** the command only ever targets `DotenvExample`, so a YAML branch
  would be unreachable, and this is a binary crate where unused `pub` items emit
  `dead_code` warnings, which the zero-warning policy forbids.
  **Alternatives:** implement YAML rename for symmetry (rejected: dead code),
  express the rename as `remove_key` + `set_value` (rejected: moves the key to
  the end of the file and loses the inline comment).
- **Decision:** one module `src/cli/encode.rs` holding both directions —
  **Because:** they share `ENC_PREFIX` and the base64 helpers, and this mirrors
  the existing `src/cli/encrypt.rs`, which holds both `run_encrypt` and
  `run_decrypt`. **Alternatives:** `encode.rs` + `decode.rs` (rejected: forces
  the shared prefix and helpers to be duplicated or hoisted for no benefit).
- **Decision:** `rename_key` rewrites **every** matching line, not just the
  first — **Because:** the spec's edge cases require both occurrences of a
  duplicated key to be encoded, whereas `set_value`/`remove_key` stop at the
  first match.
- **Decision:** sensitivity is a function of the key **name** only; the value is
  never inspected — **Because:** the spec's goals hide the name, and leaks
  ignores empty-valued keys, so a value-based rule would make the encoded set
  drift as values change.
- **Decision:** keys listed in `false_alarms` are never encoded —
  **Because:** a false alarm is a key the user has declared *not* a secret;
  obfuscating it would be wrong. Keys in `special_secret_keys` *are* encoded,
  because leaks treats them as sensitive.
- **Decision:** decode validates every file completely before writing anything —
  **Because:** the spec forbids partial rewrites, and `ChangeSet` already
  computes all new content in memory before `apply()`, so this needs no new
  machinery.
- **Decision:** `--file` with a kind other than `dotenv_example` is a hard error
  — **Because:** the target kind is fixed; silently ignoring the flag (as
  `nv leaks` currently does) would mislead the user.

## Dependencies

None new. `base64` 0.23 is already in `Cargo.toml`; this uses
`base64::engine::general_purpose::URL_SAFE_NO_PAD` with the `base64::Engine`
trait, the same engine `src/secret.rs` already uses for base64 secrets.

## Risks & mitigations

- **Risk:** an encoded key name could itself match the leaks pattern, re-flagging
  the file after encoding — **Mitigation:** measured across ~7k realistic
  `UPPER_SNAKE` keys, 0% of encoded forms contain `_`, so the pattern cannot
  match; `encode` additionally skips any key already starting with `ENC.`.
- **Risk:** a real (non-base64) key that legitimately starts with `ENC.` is
  unrecoverable by `decode` — **Mitigation:** decode reports it as malformed,
  writes nothing, and exits non-zero rather than corrupting the file. Documented
  as a known limitation.
- **Risk:** renaming must not disturb comments, ordering, or line endings —
  **Mitigation:** `rename_key` edits only the key substring of a matching line;
  dedicated formatting-preservation tests cover comments, `export `, indent,
  CRLF, and a missing trailing newline.
- **Risk:** comments naming the key still leak it — **Mitigation:** documented
  in the spec's *Known limitations*; deliberately out of scope because golden
  rule 1 requires comments to stay byte-identical.

## Testing strategy

- Unit tests in `src/parser/dotenv.rs` for `rename_key`: plain rename, value and
  inline comment preserved, `export ` prefix preserved, indentation preserved,
  CRLF preserved, missing trailing newline preserved, absent key is a no-op, and
  duplicate keys are all renamed.
- Unit tests in `src/cli/leaks.rs` asserting the shared name predicate accepts
  `JWT_SECRET` and rejects `APP_NAME`, and that `leak_pattern` still behaves as
  before (guards the refactor).
- Unit tests in `src/cli/encode.rs`: encode/decode round-trip for several keys,
  `ENC.` prefix applied and stripped, URL-safe unpadded output, `false_alarms`
  excluded, already-encoded keys skipped, decode rejects malformed base64,
  decode rejects a key that decodes to an invalid dotenv key, decode detects a
  collision.
- Manual: run `nv encode -s <svc> -y` then `nv leaks -s <svc>` on a scratch
  service and confirm the leak disappears; then `nv decode -s <svc> -y` and
  confirm the original name returns.
- Verify `--dry-run` writes nothing.

## Rollout / migration

No `nv.yml` changes are required; `encode` reuses the existing
`commands.leaks.special_secret_keys` and `false_alarms` settings. `README.md` and
both assistant configs are updated per spec 001. Existing files are untouched
until a user explicitly runs `nv encode`, and `nv decode` reverses it.