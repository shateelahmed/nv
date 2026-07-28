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
- Tree-style vertical lines (`├──`, `└──`, `│`) indicate indentation hierarchy.
- Diff previews show `+`/`-` indicators with dedicated colors (green/red).
- File names always include the full path relative to the service root
  (e.g., `docker/.env`, not just `.env`).
- The format is configurable via `nv.yml` colors, same as `find`.
- Commands that scan files MUST show a spinner during processing and print a
  summary line with statistics (files scanned, folders scanned) after completion.

## Non-goals

- Machine-readable output formats (JSON, YAML) — out of scope for now.
- Changing the hierarchical format used by `find`/`leaks` (it is the standard).

## Progress and statistics

Commands that scan multiple files (e.g., `nv leaks`, `nv fake-secrets`,
`nv duplicates`, `nv unused`) MUST follow this pattern:

1. **Spinner**: Display a spinner during the scan using `indicatif` with the
   template `{spinner:.green} {msg}`. The spinner should show the current
   file being processed or a relevant status message.

2. **Summary line**: After scanning completes, print a summary line to stderr:
   ```
   Scanned N files in M folders.
   ```
   Or for commands that also count keys (e.g., `nv unused`):
   ```
   Scanned N files in M folders for K keys.
   ```
   Where `N` is the number of files scanned, `M` is the number of
   directories traversed, and `K` is the number of keys found. Use singular
   "file", "folder", or "key" when count is 1.

3. **Cleanup**: The spinner MUST be cleared before printing results using
   `finish_and_clear()`.

## Uniform output convention (golden rule #6)

All commands that display data or previews MUST follow this structure with
tree-style vertical lines:

```text
service_name/
├── docker/.env           ← full path relative to service root
│   ├── DB_PASSWORD = secret
│   └── API_KEY = sk-test
└── configmap.yml
    └── DB_PASSWORD: secret
```

- File names always include the full path relative to the service root.
  A file at `auth/docker/.env` is shown as `docker/.env` under `auth/`.
- Tree characters (`├──`, `└──`, `│`) indicate the hierarchy.
- `├──` is used for items that have siblings below; `└──` for the last item.
- `│` continues the vertical line for non-last items; spaces for last items.
- Tree lines are colored to match the text they lead to:
  - File-level branches (`├──`, `└──`): service color (magenta)
  - Key-level branches (`├──`, `└──`): file color (cyan)
  - Diff-level branches (`├──`, `└──`): file color (cyan)
  - Continuation lines (`│`): parent's text color (service or file color)

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

```text
service_name/
├── docker/.env
│   ├── - OLD_LINE
│   └── + NEW_LINE
└── configmap.yml
    └── - OLD_LINE
```

- Lines removed from the file are prefixed with `- ` and colored `removed`.
- Lines added to the file are prefixed with `+ ` and colored `added`.
- Unchanged lines are omitted (same as current behavior).
- Tree characters show the hierarchy, same as `find`.

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
