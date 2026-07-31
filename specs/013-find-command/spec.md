# Spec: `nv find` command

- **ID:** 013-find-command
- **Status:** Implemented
- **Author:** shateel
- **Date:** 2026-07-31

## Summary

`nv find <query>` fuzzy-searches environment-variable keys across services and
prints matches in a hierarchical, colorized tree. The command predates the
spec-driven workflow; this spec retroactively documents it, including the
`-s`/`--service` scoping and the removal of the color legend.

## Problem / Motivation

- Users need to locate a key without knowing which service or file it lives in.
- When a key is known to live in a specific service, searching every service
  adds noise; a `-s`/`--service` flag must scope the search to that service.
- The color legend duplicated information the tree already conveys (each line
  is already colorized by role) and added visual noise to every invocation.

## Goals

- Fuzzy-match a query against service names, key names, and file paths.
- Scope the search to one or more named services via `-s`/`--service`.
- Print results in the uniform hierarchical tree format with no legend.

## Non-goals

- Editing files (find is read-only).
- Filtering by file kind (`-f`/`--file` is not honored by find).
- Changing the fuzzy-matching algorithm.
- TUI changes.

## User stories

- As a developer, I want to type a partial key like `dburl` and see which
  services and files define `DATABASE_URL`, so I can locate it quickly.
- As a developer, I want to run `nv find -s auth db` to search only the auth
  service, so results from unrelated services don't clutter the output.
- As a developer, I want uncluttered output without a legend, so the results
  start immediately.

## Behavior & requirements

### CLI surface

```
nv find [OPTIONS] <QUERY>

Arguments:
  QUERY  Search query (matches service, key, and file name).
         Defaults to "" (lists every key in scope).

Options:
  -s, --service <NAME>  Restrict to these services (repeatable).
                        Empty means all services.
  --all                 Target every service, ignoring --service.
```

The `-s`/`--service` flag is the shared global flag already defined on the
top-level `Cli` struct; find consumes it via `Context::service_filter()`.

### Service scoping

- When `-s`/`--service` is given, the search index MUST only contain keys from
  the named services. Services not named MUST be excluded.
- When no `-s` is given (empty filter), the search index MUST contain all
  services.
- When `--all` is given, the service filter is treated as empty (all services),
  regardless of any `-s` flags.
- Service names in the filter that do not match any discovered service MUST be
  ignored (no error).

### Matching

- An empty query MUST return every key in scope (unchanged index order).
- A non-empty query MUST fuzzy-match against a haystack of
  `"<service> <KEY> <file_display>"` using the nucleo matcher, with
  case-insensitive matching, returning results ordered by descending relevance.

### Output

- Results MUST be grouped by service, then by file, using the uniform
  hierarchical tree format shared by all commands:

```
service_name/
├── .env
│   └── DATABASE_URL = postgres://db:5432/app
└── docker/
    └── .env
        └── API_KEY = sk-...
```

- Each key line MUST be colorized with the key/value colors (same roles as
  every other command).
- The output MUST NOT include a color legend. No legend header, color-name
  listing, or separator is printed before the tree.
- When there are no matches in scope, the command MUST print `No matches.` to
  stderr and exit successfully.

## Acceptance criteria

- [ ] Given a query `db` and no `-s`, `nv find db` lists matching keys grouped
      under every service that contains a match, with no legend lines printed.
- [ ] Given `-s auth`, `nv find <query>` lists matches only from the auth
      service, even when other services contain the same key.
- [ ] Given `-s auth` and a key that exists only in billing, `nv find -s auth
      <key>` prints `No matches.`.
- [ ] Given `-s billing --all`, the search covers all services (the filter is
      ignored).
- [ ] Given an empty query, `nv find` lists every key in scope.
- [ ] Given a service name in `-s` that does not exist, the command runs
      without error and prints `No matches.` (empty scope).

## Edge cases

- No `-s` → all services are searched.
- Empty query → every key in scope is listed.
- Filter names matching no service → empty scope, `No matches.`.
- Unreadable files → skipped silently by the index builder.
- No matches → `No matches.` on stderr, exit code 0.
- Colors disabled (piped output or `--no-color`) → tree lines without ANSI
  codes; the legend is absent either way.

## Open questions

- (none)
