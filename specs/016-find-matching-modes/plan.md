# Plan: `nv find` strict keyword and pattern matching

- **Spec:** [spec.md](./spec.md)
- **Status:** Approved

## Overview

`nv find` currently routes every query through `search::search` (nucleo fuzzy
matching). We add two opt-in matching modes behind subcommand flags
(`--exact`, `--pattern <GLOB>`) and keep fuzzy matching as the default path.
The index-building, service scoping, `skip_files` filtering, grouping, and
output rendering stay untouched; only the query→result step is made mode-aware.

The three modes map to three small, pure functions in `src/search.rs` that
share one signature, so the caller in `find.rs` picks one based on the flags
and the empty-query behavior stays uniform.

## Architecture & modules

| Module | Change | Responsibility |
| --- | --- | --- |
| `src/cli/mod.rs` | edit | Add `--exact` and `--pattern <GLOB>` args to the `Find` variant with `conflicts_with`; pass them into `find::run`. |
| `src/cli/find.rs` | edit | Read the flags, select the matching mode, and call the right search function; validate `--pattern`'s glob. |
| `src/search.rs` | edit | Add `search_exact` and `search_glob` (plus a shared `match_mode` dispatcher if the caller needs one). |
| `README.md` | edit | Document the two flags and add examples. |

No new modules, no new crates.

## Data flow

```
query + flags (find.rs)
   │
   ├─ (no flags)   → search::search(index, query)        # fuzzy, unchanged
   ├─ --exact      → search::search_exact(index, query)  # whole-key equality
   └─ --pattern    → search::search_glob(index, query)   # glob on key name
        │
        ▼
   grouped results → display::render_tree (unchanged)
```

Service scoping (`-s`/`--all`) and `commands.find.skip_files` filtering happen
in `find::run` before the index is built, exactly as today, so every mode sees
the same in-scope index.

## Key decisions & trade-offs

- **Decision:** `--exact` is whole-key equality (case-insensitive), not
  substring or fuzzy — **Because:** the spec's strict-keyword requirement
  (016) and the user's choice that `URL` must not match `DATABASE_URL` —
  **Alternatives:** literal substring keyword matching (rejected: still
  returns unrelated substring keys; indistinguishable from fuzzy for many
  queries).
- **Decision:** `--pattern` uses glob, not regex — **Because:** the project
  already exposes glob syntax in `commands.find.skip_files`, so users know it
  and no second pattern language is introduced — **Alternatives:** regex via
  the `regex` crate (rejected: new dependency, new syntax, no precedent in nv).
- **Decision:** both modes are ASCII case-insensitive — **Because:** fuzzy
  matching already ignores case (`CaseMatching::Ignore`), so all three modes
  agree — **Alternatives:** case-sensitive modes (rejected: surprising when
  fuzzy is insensitive).
- **Decision:** both modes match the key name only — **Because:** the spec
  scopes them to keys; service/file matching would duplicate `-s` and
  `skip_files` and dilute "strict" — **Alternatives:** match the full
  `"<service> <KEY> <file>"` haystack (rejected: fuzzy already covers that).
- **Decision:** `--exact`/`--pattern` conflict via clap `conflicts_with` —
  **Because:** two different matchers can't both define the result set;
  a clap error is immediate and self-documenting — **Alternatives:** last-flag
  wins precedence (rejected: order-dependent, confusing).
- **Decision:** glob case-insensitivity by lowercasing both the pattern and
  the key before `glob::Pattern::new` — **Because:** the `glob` crate has no
  case-insensitive matching mode — **Alternatives:** hand-rolled glob
  matcher (rejected: reimplementing a well-tested crate for one feature).

## Dependencies

None. The `glob` crate is already a dependency and is used by
`context::matches_skip_pattern` for `skip_files`.

## Risks & mitigations

- **Risk:** `glob::Pattern` matching is whole-string, so `--pattern 'DB_'`
  would not match `DB_HOST` (no implicit trailing wildcard) — **Mitigation:**
  document in `--help` and README that patterns match the whole key name; the
  acceptance criteria use explicit `*` (e.g. `'DB_*'`).
- **Risk:** invalid glob strings (`glob::Pattern::new` returns `Err`) — **Mitigation:** validate up front and return an `anyhow` error with a clear
  message; covered by an acceptance criterion.
- **Risk:** mode dispatch drifting from fuzzy behavior — **Mitigation:** a
  single shared empty-query early return in each search function and existing
  fuzzy tests kept intact to prove the default path is unchanged.

## Testing strategy

- Unit tests in `src/search.rs`:
  - `search_exact`: whole-key match, case-insensitive, no-substring (`URL`
    does not match `DATABASE_URL`), empty query returns all in index order.
  - `search_glob`: `DB_*`, `*_URL`, case-insensitive, `*` matches all,
    invalid glob errors.
- Unit tests in `src/cli/find.rs`: mode selection from flags; conflict is
  caught by clap (covered by a `try_parse` test in `src/cli/mod.rs`).
- Manual verification on the real repo
  (`pol-payment-core-ms`): `--exact`, `--pattern`, conflict error, invalid
  glob error, `-s` + `--pattern` combination, and `find <query>` fuzzy output
  identical to before.

## Rollout / migration

No `nv.yml` changes and no file-format changes. README gains the two flags in
the find documentation and two example commands. This is additive; existing
invocations are unaffected.
