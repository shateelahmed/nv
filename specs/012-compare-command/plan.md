# Plan: `nv compare` command

- **Spec:** [spec.md](./spec.md)
- **Status:** Implemented
- **Updated:** 2026-07-31

## Overview

Add a new `compare` subcommand that parses a user-specified file, discovers all
files of matching kinds across all services, and produces a key-level diff in
the standard tree format. The implementation follows the same patterns as
`duplicates.rs` (read-only scan, tree output, no edits).

## Architecture & modules

| Module | Change | Responsibility |
| --- | --- | --- |
| `src/cli/mod.rs` | edit | Register `Command::Compare` variant and dispatch path. |
| `src/cli/compare.rs` | edit | Core comparison logic: find base file, gather peers, diff, render. |
| `src/config.rs` | edit | Add `CompareConfig` with `skip_files` and register under `commands.compare`. |
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
    skip if file matches a merged skip_files pattern
    parse file content → Vec<ParsedPair>
    compute diff:
      keys in base but not in peer → - items (removed color)
      keys in peer but not in base → + items (added color)
      (if --values) keys in both with diff values → -/+ pair
      (if --order) keys in both appearing too early → -/+ pair with
        base/peer positions
      (order_diffs extends the diff list when --order is set)
      (if --comments) comment_diffs replaces the other modes entirely:
        for each key in both files, compare the normalized attached comment →
        -/+ pair when they differ
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
- **`skip_files` follows the `unused` pattern:** A `CompareConfig` with a
  `skip_files: Vec<String>` list is merged globally + per-service, and matches
  via `glob::Pattern` against the relative path and the file name. The same
  helper approach as `special_secret_keys_for` — **Because:** consistent with
  the established `unused` command, and reuses the same glob-matching semantics
  users already know.
- **The base file is never silently filtered by `skip_files`:** If the
  requested file matches `skip_files`, the command errors with
  `file '<path>' is excluded by compare.skip_files.` instead of comparing —
  **Because:** silently ignoring the user's explicit request would be
  confusing. The error's `Available <kind> files:` tree filters to files of the
  same kind, names that kind (`env`, `configmap`, `secrets`) in the header, and
  omits the requested file so the user can pick a valid alternative.
- **Not-found error lists files via `render_tree`:** The message is
  `file '<path>' not found.` and the available files are rendered with the same
  `TreeService`/`TreeFile`/`render_tree` machinery (items empty) — **Because:**
  golden rule 6 requires the uniform output format everywhere, not a
  comma-separated hint. The tree is embedded in the error string so the message
  prints first, it respects `--service` so only the requested services' files
  are shown — **Because:** an unrequested service's files would just be noise
  when the user already scoped the command — and file counts are suppressed via
  `render_tree(..., show_file_counts: false)` since a `(0)` count after a bare
  filename in a diagnostic listing is meaningless noise.
- **`--order` only checks keys present in both files:** `order_diffs` walks the
  peer file and reports a key when its 1-based base position is smaller than
  the largest base position already seen among keys reported in the peer walk —
  **Because:** keys missing from either file are already covered by the key-only
  comparison, and reporting relative regressions (a key moving "too early"
  relative to a predecessor) is the useful signal without flooding the output
  with every positional change.
- **`--order` uses the first occurrence of duplicate keys:** later duplicates
  are ignored for ordering — **Because:** dotenv duplicates are a parse-level
  quirk; the first occurrence is the canonical position.
- **Order lines render as their own `-`/`+` pair with positions:** Each
  `OutOfOrder` diff item becomes two `TreeItem`s, `- KEY (#base_pos)` in
  `removed` and `+ KEY (#peer_pos)` in `added`, mirroring the `Different`
  pair format — **Because:** a single item cannot carry two colors, and keeping
  the pair parallel with the value-diff output keeps the tree uniform.
- **`--order` conflicts with `--values`:** clap rejects combining them with
  `conflicts_with`, even though each independently extends the diff list —
  **Because:** both checks signal "this file drifted from the base," and
  showing value and order noise at once obscures the single most relevant
  drift signal. A key with both a different value and an order mismatch is
  reported by whichever single flag the user chose.
- **`--comments` compares per-key attached comments:** a new
  `parser::parse_comments` extracts each key's comment (consecutive `#` lines
  directly above plus the inline `#` comment on the key's own line, normalized
  and joined with spaces), and `comment_diffs` compares only keys present in
  both files — **Because:** the tool is key-centric, so "compare the comments"
  most usefully means "which key lost/changed its documentation", and a bare
  set-of-comments diff would not tie a comment to the key it documents.
- **`--comments` is mutually exclusive with `--values`/`--order`:**
  `#[arg(long, conflicts_with_all = ["values", "order"])]` — **Because:** it
  swaps the comparison dimension entirely (comment text instead of key/value),
  so combining it with the value/order checks would be ambiguous.
- **Comment comparison is normalized:** leading `#` markers and whitespace are
  stripped and block+inline parts are joined with a single space, so `# DB`
  above a key plus `# prod` inline compares equal to a single `# DB prod`
  comment — **Because:** the same documentation expressed as a block versus an
  inline comment should not read as drift.
- **Comment attachment is format-aware:** in dotenv and flat YAML only
  top-level `#` lines directly above a key attach; in k8s manifests only
  comment lines at child indentation inside `data:`/`stringData:` blocks attach
  to the following child. Unattached comments (blank-line-separated, or a
  header above `data:`) are ignored — **Because:** attaching docs to the wrong
  key would produce misleading diffs.

## Dependencies

None. Reuses `parser::parse()`, `display::render_tree()`, `color` module.

## Testing strategy

- Unit tests for: peer kind resolution, diff computation (key-only, with values),
  empty/edge cases.
- Unit tests for `order_diffs`: identical order, swapped pair, fully reversed,
  missing/extra keys ignored, duplicate first-occurrence, label rendering with
  and without color.
- Unit tests for comment extraction (dotenv + flat/k8s YAML): block above key,
  inline, combined, blank-line breaks block, header ignored, quoted `#` in
  values not treated as comments.
- Unit tests for `comment_diffs`: identical comments, changed comment, comment
  added/removed on one side, key-missing-in-peer ignored, label rendering with
  and without color.
- Test `render_comparison` via `Output::String` (like `display` tests).
- Manual: run against real project files.

## Assistant configs

This spec adds `skip_files` configuration for the `compare` command and refines
the not-found error message. Both follow the existing patterns (`unused`
`skip_files`, uniform tree output per golden rule 6). It does not change any
durable rule, convention, or project guarantee. No assistant-config change is
required.
