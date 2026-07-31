# Spec: `nv compare --reorder`

- **ID:** 014-compare-reorder
- **Status:** Approved
- **Author:** shateel
- **Date:** 2026-07-31

## Summary

A `--reorder` flag on `nv compare <file-path>` that rewrites the other files of
the same kind in the base file's service so their keys follow the base file's
order, moving keys the base doesn't have to the bottom of each file. The user
sees a diff preview and confirms before anything is written.

## Problem / Motivation

Developers keep env files in sync across microservices. `nv compare --order`
already *reports* out-of-order keys, but there is no way to *fix* them. Manually
reordering keys across a service's env files is tedious and error-prone, and a
careless editor rewrite destroys comments and formatting.

`nv compare` is today read-only (see spec 012, "Non-goals"). This spec lifts
that restriction for a single, explicitly-requested, confirm-before-write
operation.

## Goals

- Reorder peer files' keys to match the base file's key order.
- Move keys absent from the base file to the bottom of each peer, preserving
  their relative order.
- Keep values, comments, blank lines, and formatting intact — only key lines and
  their attached comment blocks move.
- Require an explicit `--service` so the operation is scoped and unambiguous.
- Show the diff and ask for confirmation before writing, like `set`/`remove`/
  `gen`.

## Non-goals

- Adding or removing keys (no sync, only reordering).
- Reordering files outside the base file's service.
- Moving keys between YAML blocks (`data:` ↔ `stringData:`).
- Changing any value, comment, or blank line.
- Altering the read-only behavior of plain `nv compare` (without `--reorder`).

## User stories

- As a platform engineer, I want to make all configmaps in a service match the
  key order of a reference configmap, so the files are uniform and `--order`
  stops flagging them.
- As a developer, I want my service's `.env` files reordered to match
  `.env.example`, with a preview first, so I can keep templates tidy without
  hand-editing each file.

## Behavior & requirements

### Invocation

`nv compare <FILE_PATH> --reorder` MUST require `--service`; without it the
command errors with a clear message (e.g. `--reorder requires --service to
identify the base file's service.`).

`--reorder` MUST be mutually exclusive with `--values`, `--order`, and
`--comments` (clap `conflicts_with_all`); combining them is a parse error like
the existing flag conflicts.

### Scope: which files are reordered

- The base file is resolved exactly as in plain `nv compare`: same kind-matching
  table, `compare.skip_files` handling, "file not found" listing, and
  multi-service disambiguation error all apply unchanged.
- ONLY files in the base file's own service are reordered. With multiple
  `--service` flags, the base must be found in exactly one of them and only that
  service's files are reordered.
- Peer files are the base service's other files of a matching kind (dotenv and
  dotenv_example are mutually matching kinds; configmap and secret match
  themselves), excluding the base file and files matching the service's merged
  `compare.skip_files`.
- Unreadable peer files are silently skipped (same as plain compare).

### Target order

- The base file's key order is its recognized keys in file order (first
  occurrence wins for duplicates), matching how `--order` reads the base.
- For a peer, each key that exists in the base keeps the base's relative order.
- Keys in the peer that do NOT exist in the base are **extras**: they are moved
  to the bottom of the peer, preserving their current relative order. Nothing is
  added and nothing is removed.
- Concretely, a peer's new order is: [keys present in both files, sorted by
  base-file position] followed by [extras in their current relative order].
- A peer already in that order is left unchanged (no diff, not rewritten).

### What moves with a key

- A key is moved together with its **attached comment block**: the consecutive
  `#` lines directly above it, using the exact attachment rules from
  `--comments` (spec 012): prose `#` lines attach; a blank line, a commented-out
  assignment (`# KEY=value` in dotenv, `# KEY: value` in YAML), or an
  indentation break stops the block. The inline `# comment` on the key line is
  part of the line and moves with it.
- Comment lines that do NOT attach to a key, and blank lines, keep their
  positions in the file; only key lines and attached comment blocks move. This
  guarantees the moving units stay contiguous and every non-moving line is
  byte-identical.
- Only the lines of the moving unit are relocated; each line's text is never
  rewritten. Values are therefore byte-identical (including raw secret strings
  and empty example-file values — no secret generation or encoding occurs).

### YAML shapes

- **Flat YAML:** top-level `key: value` entries are reordered; their attached
  comment blocks move with them.
- **Kubernetes manifests:** the scalar children of each top-level block
  (`stringData:`, then `data:`) are reordered independently, within their own
  block only. Keys never move between blocks. The base file's relative order is
  read across both blocks (stringData children first, then data children); each
  peer block follows that order restricted to the keys it contains, with that
  block's extras at the bottom. A block whose keys are all extras stays
  unchanged.

### Confirmation and write path

- `--reorder` MUST use the shared safe-write flow (`preview_and_apply`):
  build a change set, render the hierarchical colorized line diff, stop for
  `--dry-run`, prompt `Apply these changes?` (default no) unless `--yes`, then
  write.
- The diff groups changed files by service and shows full relative paths with
  `+`/`-` lines in `added`/`removed` colors — the same format as
  `set`/`remove`/`gen`.
- When no peer needs reordering, the command prints `Nothing to change.` and
  writes nothing.
- The config-source banner (`nv.yml` vs `command-line`) is printed as in every
  other command.

### CLI surface

```
nv compare [OPTIONS] <FILE_PATH>

Options (existing):
  --values   Also compare values for keys present in both files.
  --order    Also check that keys present in both files appear in the same order.
             Cannot be combined with --values.
  --comments Compare comments: each key's attached comment, then every other
             comment in the files.
             Cannot be combined with --values or --order.

Options (new):
  --reorder  Rewrite other files of the same kind in the base file's service so
             their keys follow the base file's order; keys the base lacks move
             to the bottom. Requires --service. Shows a diff and asks for
             confirmation before writing. Cannot be combined with --values,
             --order, or --comments.
```

## Acceptance criteria

- [ ] Given a service with `.env.example` ordered `A, B, C` and a `.env` ordered
      `C, A, B`, when `nv compare .env.example --reorder --service <svc>` runs
      and the diff is confirmed, then `.env` is rewritten as `A, B, C`.
- [ ] Given the same files as above with `--dry-run`, when `nv compare`
      `--reorder` runs, the diff is shown and the file is NOT written.
- [ ] Given the same files as above WITHOUT `--yes`, when `nv compare
      --reorder` runs and the prompt is declined, the file is NOT written.
- [ ] Given the same files as above with `--yes`, when `nv compare --reorder`
      runs, the file IS written without prompting.
- [ ] Given a base file with keys `A, B, C` and a peer with keys `X, C, A, B`,
      when `nv compare --reorder` runs, the peer's new order is `A, B, C, X`
      (the extra `X` moves to the bottom).
- [ ] Given a base file with keys `A, B, C` and a peer with keys `X, C, A, Y, B`,
      when `nv compare --reorder` runs, the peer's new order is `A, B, C, X, Y`
      (extras keep their relative order).
- [ ] Given a peer file with keys in the same relative order as the base, when
      `nv compare --reorder` runs, `Nothing to change.` is printed and the file
      is not touched.
- [ ] Given a key with a `# comment` block directly above it, when
      `nv compare --reorder` runs and the key moves, the comment block moves
      with it and stays directly above it.
- [ ] Given a blank-line-separated `# header` above the first key, when
      `nv compare --reorder` runs, the header keeps its position and does not
      follow any moved key.
- [ ] Given a commented-out assignment line (`# KEY=value`) between a comment
      block and a key, when `nv compare --reorder` runs, the line stays in place
      (it never attaches) and the block above it does not move with the key.
- [ ] Given a peer where only the order changes, when `nv compare --reorder`
      runs and writes, every non-key line in the result is byte-identical to
      before and every value is unchanged.
- [ ] Given a `configmap.yml` with a `data:` block whose children are
      out of order, when `nv compare --reorder` runs, the children are reordered
      within the block and the block header/indentation are preserved.
- [ ] Given a `secrets.yml` with both `stringData:` and `data:` blocks, when
      `nv compare --reorder` runs, children are reordered independently within
      each block and no key moves between blocks.
- [ ] Given a peer matching the service's `compare.skip_files`, when
      `nv compare --reorder` runs, the file is NOT reordered.
- [ ] Given `nv compare --reorder` WITHOUT `--service`, the command errors with
      `--reorder requires --service` (exit code 1).
- [ ] Given `nv compare --reorder --values`, `--order`, or `--comments`, the
      command errors with a clap parse error (exit code 2).
- [ ] Given the base file path not found, when `nv compare --reorder` runs, the
      error matches plain `nv compare` (`file '<path>' not found.` plus the
      available-files tree).
- [ ] Given the base file listed under `compare.skip_files`, when
      `nv compare --reorder` runs, the command errors with
      `file '<path>' is excluded by compare.skip_files.`
- [ ] Given two `.env` files in the same service where the second is the base
      and the first is out of order, when `nv compare --reorder` runs against
      the second, only the first file is rewritten (the base is never touched).

## Edge cases

- No `--service` → runtime error `--reorder requires --service to identify the
  base file's service.` before any file is read or written.
- `--reorder` with `--values`/`--order`/`--comments` → clap parse error
  (exit 2), mirroring the existing `--comments` conflicts.
- Base file appears in multiple services → the standard "found in multiple
  services: … Use --service to disambiguate." error; nothing is written.
- Empty base file (no keys) → every peer key is an extra, so no peer changes;
  `Nothing to change.`
- Peer with no keys in common with the base → its keys are all extras and
  already at the bottom, so it stays unchanged.
- Duplicate keys → first occurrence sets the order; the key block follows the
  first occurrence.
- Dotenv ↔ DotenvExample: both directions are valid peer relationships within
  the base service, matching the plain `compare` kind table.
- Empty-value flat YAML entries and other lines the parser does not recognize as
  keys are not reorderable; their lines stay where they are (consistent with the
  keys `nv compare` reports).
- Unreadable peer file → skipped silently.
- Files matching `compare.skip_files` → excluded from reordering; if the base
  itself matches, the command errors as in plain compare.
- A peer already in target order → no diff, not rewritten, not reported.

## Open questions

None — all behavior decisions were confirmed before writing:

- Peer scope: same service only.
- Missing-key rule: extras (keys in the peer absent from the base) move to the
  bottom of that peer, preserving relative order; nothing is added or removed.
- Comments: attached comment blocks move with their key.

## Assistant-config sync

No assistant-config change is required. This spec extends an existing command's
flags and reuses the established formatting-preservation, uniform-output, and
confirm-before-write conventions already captured in both assistant configs; it
introduces no new durable cross-cutting rule.
