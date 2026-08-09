# Spec: `nv find` strict keyword and pattern matching

- **ID:** 016-find-matching-modes
- **Status:** Implemented
- **Author:** shateel
- **Date:** 2026-08-09

## Summary

`nv find` gains two opt-in matching modes behind flags: `--exact` (the query
must equal a key's whole name, case-insensitively) and `--pattern <GLOB>`
(the query is a glob pattern matched against the key name). Fuzzy matching
remains the default and is unchanged. This spec extends
[013-find-command](../013-find-command/spec.md); it does not replace it.

## Problem / Motivation

- Fuzzy matching is great for locating a key when you only remember part of
  its name, but it has no way to say "I want *this exact* key". Typing the full
  `DATABASE_URL` still returns every similar key across services, so a precise
  lookup is noisy.
- Fuzzy matching cannot express "every key that starts with `DB_`" or "any key
  ending in `_URL`" at all. Users hand-type each candidate key instead.
- The project already has a well-known glob syntax in
  `commands.find.skip_files`; reusing it for pattern matching keeps the tool
  consistent rather than introducing a second pattern language.

## Goals

- Add `--exact` so the query must match a key's whole name
  (case-insensitive); substring and fuzzy matches are excluded.
- Add `--pattern <GLOB>` so the query is a glob matched against the key name,
  using the same syntax as `commands.find.skip_files`.
- Keep fuzzy matching as the default and behavior with no flags bit-for-bit
  identical to today.
- Both modes work with the existing `-s`/`--all` scoping and `skip_files`
  exclusion.

## Non-goals

- Changing the default fuzzy-matching algorithm or its results.
- Regex patterns (the `--pattern` flag is glob-only in this spec).
- Matching against service names or file paths for `--exact`/`--pattern`
  (these match key names only; service scoping already exists via `-s`).
- New `nv.yml` configuration for these modes (flags only).
- TUI changes.

## User stories

- As a developer, I want `nv find --exact DATABASE_URL` to show me only the
  keys literally named `DATABASE_URL` (in whatever case), not every key with
  `DATABASE_URL` as a substring, so a precise lookup is quiet.
- As a developer, I want `nv find --pattern 'DB_*'` to show every key matching
  the glob, so I can see related keys without enumerating them.
- As a developer, I want to combine these modes with `-s`, so I can run
  `nv find -s auth --pattern '*_URL'` and only see auth's URL keys.
- As a developer, I want fuzzy search to keep working exactly as before when I
  don't pass either flag.

## Behavior & requirements

### CLI surface

```
nv find [OPTIONS] <QUERY>

Arguments:
  QUERY  Search query. Defaults to "" (lists every key in scope).

Options:
  -s, --service <NAME>  Restrict to these services (repeatable).
  --all                 Target every service, ignoring --service.
  --exact               Treat QUERY as a literal keyword: only keys whose
                        whole name equals QUERY (case-insensitive).
  --pattern <GLOB>      Treat QUERY as a glob pattern matched against the key
                        name (case-insensitive). Same syntax as
                        commands.find.skip_files.
```

- `--exact` and `--pattern` MUST be mutually exclusive; passing both MUST
  produce a clap usage error before any search runs.
- Both flags belong to the `find` subcommand only (not global options).

### Matching modes

- With no `--exact`/`--pattern`, matching MUST behave exactly as today:
  fuzzy matching via nucleo against `"<service> <KEY> <file_display>"`,
  results ordered by descending relevance.
- `--exact <QUERY>`: a key MUST match only when its whole name equals QUERY,
  compared case-insensitively (ASCII). A key whose name merely contains QUERY
  MUST NOT match.
- `--pattern <GLOB>`: a key MUST match when GLOB matches its whole name,
  compared case-insensitively (ASCII). The glob syntax is the one already used
  by `commands.find.skip_files`:
  - `*` matches any characters,
  - `**` matches any characters (recursive),
  - `?` matches a single character.
- `--exact` and `--pattern` MUST match the key name only; service names and
  file paths are not matched by either mode.
- Both modes MUST be case-insensitive, consistent with fuzzy matching's
  existing `CaseMatching::Ignore`.

### Result set & ordering

- An empty or blank QUERY MUST return every key in scope, regardless of mode
  (the flags are no-ops on an empty query).
- `--exact`/`--pattern` results MUST be returned in index order (the order
  keys appear in the scanned files); there is no relevance ranking in these
  modes.
- Grouping into the hierarchical tree, key-line colorization, and the
  `No matches.` message on stderr (exit 0) MUST be unchanged.

### Interaction with existing behavior

- `-s`/`--service` and `--all` scoping MUST apply before matching, exactly as
  they do today.
- `commands.find.skip_files` (global + per-service) MUST still exclude files
  from the index before matching, so no key from a skipped file can match in
  any mode.

## Acceptance criteria

- [ ] Given `DATABASE_URL` in auth and `API_DATABASE_URL` in billing, when I
      run `nv find --exact DATABASE_URL`, then only auth's `DATABASE_URL`
      appears and billing's substring key does not.
- [ ] Given a key `DATABASE_URL`, when I run `nv find --exact database_url`,
      then it appears (case-insensitive).
- [ ] Given `DATABASE_URL` and `API_KEY` and no other URL-ish key, when I run
      `nv find --exact URL`, then `No matches.` is printed (whole-key, no
      substring, no fuzzy).
- [ ] Given keys `DB_HOST`, `DB_PORT`, and `REDIS_URL`, when I run
      `nv find --pattern 'DB_*'`, then only `DB_HOST` and `DB_PORT` appear.
- [ ] Given uppercase keys `DB_HOST` and `REDIS_URL`, when I run
      `nv find --pattern 'db_*'`, then `DB_HOST` appears (case-insensitive).
- [ ] Given keys `DATABASE_URL` and `API_KEY`, when I run
      `nv find --pattern '*_URL'`, then only `DATABASE_URL` appears.
- [ ] When I run `nv find --exact X --pattern 'Y*'`, the command exits non-zero
      with a usage error explaining the flags conflict.
- [ ] When I run `nv find --exact` (empty query), every key in scope is listed.
- [ ] When I run `nv find --pattern '['` (invalid glob), the command prints an
      error and exits non-zero.
- [ ] Given `dburl` and a key `DATABASE_URL`, when I run `nv find dburl` (no
      flags), the fuzzy match behaves exactly as before the change.
- [ ] Given a key that exists in both auth and billing, when I run
      `nv find --exact KEY -s auth`, then only auth's occurrence appears.
- [ ] Given a service with `docker/.env` in `commands.find.skip_files`, when I
      run `nv find --pattern 'DB_*'`, then no key from `docker/.env` appears.

## Edge cases

- Both `--exact` and `--pattern` passed → clap usage error, exit non-zero.
- Empty/blank query with either flag → every key in scope, no mode applied.
- Glob pattern `*` → every key in scope.
- Glob patterns with `/` (e.g. `a/b`) simply never match a key name; key names
  are single tokens.
- Invalid glob → clear error message, exit non-zero.
- Key names differing only by case → matched by both modes.
- No matches in any mode → `No matches.` on stderr, exit 0.
- `skip_files` files → their keys are absent from the index before matching,
  in all modes.

## Open questions

- (none)

## Assistant-config note

No assistant-config change is required. This spec extends an existing
command's CLI surface with new flags; it introduces no cross-cutting rule,
convention, workflow step, or project guarantee. `README.md` user docs will be
updated during implementation (per the plan).
