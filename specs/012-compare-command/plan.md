# Plan: `nv compare` command

- **Spec:** [spec.md](./spec.md)
- **Status:** Implemented

## Overview

Add a new `compare` subcommand that parses a user-specified file, discovers all
files of matching kinds across all services, and produces a key-level diff in
the standard tree format. The implementation follows the same patterns as
`duplicates.rs` (read-only scan, tree output, no edits).

## Architecture & modules

| Module | Change | Responsibility |
| --- | --- | --- |
| `src/cli/mod.rs` | edit | Register `Command::Compare` variant and dispatch path. |
| `src/cli/compare.rs` | **new** | Core comparison logic: find base file, gather peers, diff, render. |
| `src/model.rs` | edit | (optional) Add helper to find a file by display path across services. |

No new dependencies. Uses existing `parser::parse()`, `display::render_tree()`,
and `color::colored_kv_label()`.

## Data flow

```
user provides <file-path>
  ↓
resolve base file: search ctx.services for matching file.display
  (if ambiguous without --service → error with hint)
  ↓
determine base kind → compute peer kinds
  ↓
for each service:
  for each file with matching kind and different from base:
    parse file content → Vec<ParsedPair>
    compute diff:
      keys in base but not in peer → - items (removed color)
      keys in peer but not in base → + items (added color)
      (if --values) keys in both with diff values → -/+ pair
  ↓
group by service → build TreeService/TreeFile/TreeItem
  ↓
render_tree()
```

## Key decisions & trade-offs

- **File matching by `file.display`:** The base file's path (relative to service
  root) is used to find it in the service list. This matches how `find_target`
  works in `model.rs` — **Because:** consistent with existing conventions.
- **Peer kinds table per spec:** Dotenv matches Dotenv + DotenvExample; ConfigMap
  matches only ConfigMap. — **Because:** users want to compare `.env` templates
  across services, not cross-kind.
- **Tree items use `added`/`removed` colors directly:** Instead of the
  `TreeItem.color` convention used by `find`/`leaks` (where the whole line is
  one color), diff items embed the color in the label via ANSI escape codes.
  — **Because:** the `+`/`-` prefix and the key+value need distinct colors, and
  the tree renderer's outer `colorize` would override a single `item.color` for
  the entire line. The same approach is used by `ChangeSet::render_diff`.
- **`--service` disambiguates path collisions:** If the same relative path
  exists in multiple services, the user must specify `--service` — **Because:**
  clear error over silent ambiguity.
- **Silently skip unreadable peer files:** Same behavior as leaks/duplicates.

## Dependencies

None. Reuses `parser::parse()`, `display::render_tree()`, `color` module.

## Testing strategy

- Unit tests for: peer kind resolution, diff computation (key-only, with values),
  empty/edge cases.
- Test `render_comparison` via `Output::String` (like `display` tests).
- Manual: run against real project files.

## Assistant configs

This spec introduces a new command (`compare`) and the `--values` flag. It does
not change any durable rule, convention, or project guarantee. No
assistant-config change is required.
