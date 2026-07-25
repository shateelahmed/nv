# Spec: Uniform output format

- **ID:** 005-uniform-output
- **Status:** Implemented
- **Author:** shateel
- **Date:** 2026-07-25

## Summary

All commands that display data or previews MUST use the same hierarchical
colorized format. This replaces the raw unified diff (`---`/`+`/`-`) used by
`set`, `gen`, `remove`, and `leaks --clean` with the same hierarchical tree
used by `find` and `leaks`.

## Problem / Motivation

The codebase had two visually distinct output styles:
- `find`/`leaks` used a hierarchical colorized tree (service → file → key = value)
- `set`/`gen`/`remove` dry-run used a raw unified diff (plain `---`/`+`/`-` lines, no color)

This inconsistency made the CLI feel disjointed and the diff previews hard to
scan quickly.

## Goals

- All commands produce output in the same hierarchical colorized format.
- Diff previews show `+`/`-` indicators with dedicated colors (green/red).
- Subfolder grouping is used when file paths contain `/`.
- The format is configurable via `nv.yml` colors, same as `find`.

## Non-goals

- Machine-readable output formats (JSON, YAML) — out of scope for now.
- Changing the hierarchical format used by `find`/`leaks` (it is the standard).

## Uniform output convention (golden rule #6)

All commands that display data or previews MUST follow this structure:

```
service_name/
  subfolder/           ← when file path contains /
    filename
      KEY = value      ← for data display (find, leaks)
      + KEY = new      ← for diff additions
      - KEY = old      ← for diff removals
  filename
    KEY = value
```

### Color roles

| Role | Default | Used for |
| --- | --- | --- |
| `service_root` | Magenta | Service name |
| `subfolder` | Blue | Subfolder names |
| `file` | Cyan | File names |
| `key` | Green | Key names |
| `value` | Yellow | Key values |
| `added` | Green | `+` diff lines |
| `removed` | Red | `-` diff lines |

All colors are configurable in `nv.yml` under `colors:`. Color is disabled
when `NO_COLOR` is set or stdout is not a TTY.

### Diff preview format

When showing a preview of changes (used by `set`, `gen`, `remove`, `leaks --clean`):

```
service_name/
  filename
    - OLD_LINE
    + NEW_LINE
```

- Lines removed from the file are prefixed with `- ` and colored `removed`.
- Lines added to the file are prefixed with `+ ` and colored `added`.
- Unchanged lines are omitted (same as current behavior).
- The hierarchical grouping (service → subfolder → file) is the same as `find`.

## Acceptance criteria

- [ ] `nv set KEY VALUE --dry-run` shows a hierarchical colorized preview, not
      a raw unified diff.
- [ ] `nv gen KEY --dry-run` shows a hierarchical colorized preview.
- [ ] `nv remove KEY -e --dry-run` shows a hierarchical colorized preview.
- [ ] `nv leaks --clean --dry-run` shows a hierarchical colorized preview.
- [ ] All previews use the same color roles as `nv find`.
- [ ] Adding new color roles (`added`, `removed`) to `nv.yml` works.
- [ ] `NO_COLOR` disables all colors in previews.

## Implementation notes

- `ChangeSet::render_diff` is updated to accept `&ColorConfig` and `use_color`.
- `context::preview_and_apply` is updated to accept and pass through color config.
- All command handlers pass the resolved color config to `preview_and_apply`.
