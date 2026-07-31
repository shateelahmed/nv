# Plan: `nv find` command (`-s` scoping, legend removal, `skip_files`)

- **ID:** 013-find-command
- **Spec:** [spec.md](./spec.md)
- **Status:** Implemented
- **Author:** shateel
- **Date:** 2026-07-31

## Overview

Retroactive spec for the existing `nv find` command plus three behavior
changes:

1. **Remove the color legend** — `find.rs` previously printed a
   `Color legend:` block (via `print_legend()` / `colorize_color_name()`)
   before the result tree. Both helpers and the `AnsiColor` import are
   deleted; the tree now prints immediately after the banner.
2. **`-s` service scoping** — find previously searched all services
   unconditionally. It now filters `Context::services` by
   `Context::service_filter(cli)` (empty when `--all`) before building the
   search index, so `nv find -s auth <query>` only searches the auth service.
3. **`skip_files` filtering** — `commands.find.skip_files` (global, merged
   with per-service entries) excludes matching files from the search index.
   The glob-matching helpers previously private to `compare.rs`
   (`matches_skip_pattern`, `file_is_skipped`, `build_skip_files`) are moved
   to `context.rs` so both commands share one implementation.

## Architecture & modules

| Module | Change | Responsibility |
| --- | --- | --- |
| `src/config.rs` | edit | Add `FindConfig { skip_files }`, `CommandsConfig::find`, `Config::find_skip_files_for()` merge helper + tests. |
| `src/cli/context.rs` | edit | Host shared skip helpers `matches_skip_pattern`, `file_is_skipped`, `build_skip_files`. |
| `src/cli/compare.rs` | edit | Use the shared helpers from `context.rs`. |
| `src/cli/find.rs` | edit | Remove legend helpers; filter services via `service_filter`; drop files matching `find.skip_files` before `build_index`. |
| `src/color.rs` | none | `AnsiColor::name()` remains used by the `Display` impl. |
| `src/search.rs` | none | Index builder and matcher unchanged; they operate on whatever service slice they receive. |
| `nv.yml.example` | edit | Document global + per-service `commands.find.skip_files`. |

## Data flow

```
nv find -s auth db
  → Context::service_filter(cli) → ["auth"]   (empty when --all / no -s)
  → filter ctx.services by name → Vec<Service>
  → per service: build_skip_files(find.skip_files, global + per-service)
  → drop files matching a skip pattern from service.files
  → search::build_index(&services) → Vec<EnvKey>   (auth only, no skipped files)
  → search::search(&index, "db") → matches, relevance-sorted
  → display::render_tree(...)                      (no legend)
```

## Key decisions & trade-offs

- **Filter services before building the index, not after searching.** —
  Because: scoping at the index level keeps `search.rs` unchanged and makes
  `--all` fall out of `service_filter()` naturally. — Alternatives: filtering
  results post-search (would still build the full index and waste work).
- **Clone filtered services into a `Vec<Service>`** — Because:
  `build_index()` takes `&[Service]`, and borrowing `Vec<&Service>` would
  require a signature change rippling to other callers. `Service` derives
  `Clone`. — Alternatives: change `build_index` to accept `&[&Service]`.
- **Drop skipped files before `build_index`, not during iteration.** —
  Because: `build_index` reads each file in `service.files`; removing matched
  files up front keeps it generic and gives a single source of truth. —
  Alternatives: teach `build_index` a skip-list parameter (couples search to
  config).
- **Share the skip helpers in `context.rs` instead of duplicating them in
  find.** — Because: `compare.rs` already implemented identical matching; per
  spec 010 the project dedups shared command logic (cf.
  `context::mark_false_alarm`). `file_is_skipped` takes the service root path
  so callers can borrow `service.path` disjointly from `service.files` (needed
  for `Vec::retain`). — Alternatives: keep two copies in each command (drift
  risk).
- **Delete the legend rather than gate it behind a flag** — Because: the tree
  already colorizes each role; the legend added noise on every run. —
  Alternatives: a `--legend` opt-in flag (no demand; keep output minimal).
- **Reuse the global `-s`/`--service` flag instead of adding a find-local
  flag** — Because: consistent with `set`, `remove`, `generate`, `leaks`, etc.
  — Alternatives: a subcommand-local `--scope` flag (duplicates existing
  machinery).

Each decision traces to the spec's Service scoping, Configuration, and Output
requirements.

## Dependencies

None — uses only existing modules (`context`, `search`, `display`, `color`).

## Risks & mitigations

- **Risk:** Filtering services changes TUI-facing shared code paths. —
  **Mitigation:** find only filters its own `run()`; `Context::service_filter`
  is already used by other commands and is unchanged.
- **Risk:** Removing the legend leaves `AnsiColor::name()` unused. —
  **Mitigation:** it is still used by `fmt::Display` (color config printing),
  so it stays.

## Testing strategy

- Existing unit tests in `src/cli/find.rs` (grouping by service/folder,
  duplicate keys) cover the renderer; the `print_legend` test is removed with
  the legend.
- `config.rs`: 4 tests for `find_skip_files_for` (empty, global, per-service,
  merge), mirroring the compare tests.
- `compare.rs`: the moved skip-pattern tests now call the shared
  `context::` helpers.
- Manual smoke test against a scratch multi-service tree:
  - `find db` lists both services, no legend.
  - `find -s auth db` lists only auth.
  - `find -s auth <key-in-billing-only>` prints `No matches.`.
  - Global `find.skip_files: [docker/.env]` hides nested-file keys.
  - Per-service `find.skip_files: [.env]` hides only that service's keys.

## Rollout / migration

New optional `commands.find.skip_files` key in `nv.yml` (global and/or
per-service). No existing config breaks; when unset, find behaves as before.
Output format is unchanged except for the removed legend lines.
