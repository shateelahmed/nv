# Plan: `nv find` command (`-s` scoping + legend removal)

- **ID:** 013-find-command
- **Spec:** [spec.md](./spec.md)
- **Status:** Implemented
- **Author:** shateel
- **Date:** 2026-07-31

## Overview

Retroactive spec for the existing `nv find` command plus two behavior changes:

1. **Remove the color legend** — `find.rs` previously printed a
   `Color legend:` block (via `print_legend()` / `colorize_color_name()`)
   before the result tree. Both helpers and the `AnsiColor` import are
   deleted; the tree now prints immediately after the banner.
2. **`-s` service scoping** — find previously searched all services
   unconditionally. It now filters `Context::services` by
   `Context::service_filter(cli)` (empty when `--all`) before building the
   search index, so `nv find -s auth <query>` only searches the auth service.

## Architecture & modules

| Module | Change | Responsibility |
| --- | --- | --- |
| `src/cli/find.rs` | edit | Remove legend helpers; filter services via `service_filter` before `build_index`. |
| `src/color.rs` | none | `AnsiColor::name()` remains used by the `Display` impl. |
| `src/search.rs` | none | Index builder and matcher unchanged; they operate on whatever service slice they receive. |

## Data flow

```
nv find -s auth db
  → Context::service_filter(cli) → ["auth"]   (empty when --all / no -s)
  → filter ctx.services by name → Vec<Service>
  → search::build_index(&services) → Vec<EnvKey>   (auth only)
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
- **Delete the legend rather than gate it behind a flag** — Because: the tree
  already colorizes each role; the legend added noise on every run. —
  Alternatives: a `--legend` opt-in flag (no demand; keep output minimal).
- **Reuse the global `-s`/`--service` flag instead of adding a find-local
  flag** — Because: consistent with `set`, `remove`, `generate`, `leaks`, etc.
  — Alternatives: a subcommand-local `--scope` flag (duplicates existing
  machinery).

Each decision traces to the spec's Service scoping and Output requirements.

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
- Manual smoke test against a scratch multi-service tree:
  - `find db` lists both services, no legend.
  - `find -s auth db` lists only auth.
  - `find -s auth <key-in-billing-only>` prints `No matches.`.

## Rollout / migration

No config (`nv.yml`) or file-format implications. Output format is unchanged
except for the removed legend lines.
