# Plan: `nv compare --reorder`

- **Spec:** [spec.md](./spec.md)
- **Status:** Draft

## Overview

`--reorder` extends `nv compare` with a write mode. After the base file is
resolved (unchanged logic), the command computes the base's key order, collects
the base service's other files of matching kinds as peer targets, and computes a
reordered version of each peer. Each peer's new content goes through the shared
`ChangeSet` → diff-preview → confirm → apply flow, so nothing is written without
a diff review and confirmation.

The core new machinery is a formatting-preserving line reorder in the parser:
key units (a key line, its attached comment block, and its trailing blank lines)
are permuted to the base order, while every non-attached comment line and
non-key line stays at its absolute position.

## Architecture & modules

| Module | Change | Responsibility |
| --- | --- | --- |
| `src/parser/reorder.rs` | new | Generic "reorder key units within a line region" scan + placement, parameterized by format rules. |
| `src/parser/dotenv.rs` | edit | `reorder(content, order)` entry point + dotenv scan rules. |
| `src/parser/yaml.rs` | edit | `reorder(content, order)` entry point + flat/k8s scan rules. |
| `src/parser/mod.rs` | edit | Dispatch `reorder(content, kind, order)` to the right sub-parser. |
| `src/edit.rs` | edit | `ChangeSet::reorder(targets, order)`: read each peer, compute reordered content, skip unreadable files. |
| `src/cli/compare.rs` | edit | `--reorder` branch: `--service` validation, build base order, collect peer targets, preview + apply. |
| `src/cli/mod.rs` | edit | Add `--reorder` flag with `conflicts_with_all`, extend dispatch. |
| `README.md` | edit | Document `nv compare --reorder`. |

## Data flow

```
nv compare <FILE_PATH> --reorder --service <svc>
  → cli/mod.rs: Compare { file_path, values: false, order: false,
                         comments: false, reorder: true }
  → compare::run(cli, file_path, false, false, false, true)
      1. context::resolve(cli)         → ctx; print_banner(ctx.source)
      2. if cli.services.is_empty()    → bail "--reorder requires --service …"
      3. resolve base file (existing logic: kind matching, skip_files,
         "not found" listing, multi-service disambiguation)
      4. base_pairs = parser::parse(base_content, base_kind)
      5. base_order = keys in file order, first occurrence (spec: target order)
      6. peers = base_service.files where kind ∈ peer_kinds(base_kind)
               && display != base.display && !file_is_skipped(...)
      7. changes = ChangeSet::reorder(&peers, &base_order)
           for each peer: read (skip on error) → parser::reorder(content, kind,
             base_order) → FileChange{old, new}
      8. context::preview_and_apply(cli, &changes, &colors, use_color)
             render_diff → --dry-run stop → "Apply these changes?" → apply()
```

`parser::reorder(content, kind, order)`:
1. Split lines, detect newline, remember trailing newline (same pattern as
   `set_value`).
2. For dotenv: whole file is one region. For YAML: k8s shape splits into per-block
   regions (`stringData:`, `data:`); flat shape is one region.
3. Each region is scanned into **key units** and **orphans** (see reorder model).
4. Key units are permuted: base-present keys in `order`-rank order, then extras
   in their current relative order.
5. Orphans keep their absolute indexes; remaining positions are filled
   top-to-bottom with the permuted units' lines.

## Key decisions & trade-offs

- **Decision:** Reorder lives in a new `src/parser/reorder.rs` behind a
  parameterized region scanner; `dotenv.rs`/`yaml.rs` supply key detection and
  comment-attachment rules, and `mod.rs` dispatches like `set_value`.
  — **Because:** golden rule #1 mandates all parsing/writing go through
  `src/parser/`; the scan is ~90% shared across formats (spec: "Same kind
  table", "What moves with a key"). — **Alternatives:** one bespoke function per
  format duplicated the tricky scan logic; a full YAML round-trip would violate
  golden rule #2 (no comment-preserving YAML serializer).
- **Decision:** A key unit = attached comment block (consecutive prose `#` lines
  directly above, per spec-012 attachment rules) + the key line + trailing blank
  lines up to the next non-blank line. — **Because:** variable-height units
  cannot be swapped while a fixed line separates them; carrying trailing blanks
  makes every unit self-contained, so reordering never gains or loses lines and
  the total height is invariant (spec: "What moves with a key", acceptance
  "byte-identical every non-key line").
- **Decision:** Every other line — un-attached comments (headers separated by
  blanks, commented-out assignments, comments broken by an indent boundary) and
  non-key lines — is an **orphan** pinned to its absolute line index. — **Because:**
  the spec requires blank-line-separated headers and commented-out assignments
  to stay in place; pinning orphans and filling the remaining slots with the
  permuted units is always well-defined (total height matches) and preserves
  every orphan byte-for-byte (spec: "Comment lines that do NOT attach … keep
  their positions", acceptance criteria for headers and commented-out lines).
- **Decision:** Keys never cross regions (k8s blocks are reordered
  independently). — **Because:** moving a key from `stringData:` to `data:`
  changes semantics and could break k8s expectations; the spec lists it as a
  non-goal.
- **Decision:** k8s reorder moves only direct children (lines at the block's
  child indent); deeper-indented content is treated as fixed orphan lines.
  — **Because:** nested mappings are not expected in env files and must never be
  rearranged; the parser's own block model (`collect_block_children`) treats
  children by indent, and a conservative rule keeps the file safe.
- **Decision:** `ChangeSet::reorder` skips peers it cannot read instead of
  treating them as empty (unlike `read_or_empty`). — **Because:** the spec says
  unreadable peers are silently skipped; writing an empty file for a peer that
  vanished from disk would be destructive.
- **Decision:** `--service` is enforced at runtime with `bail!`, not clap
  `requires`. — **Because:** `services` is a global repeatable arg; a runtime
  check gives a clear message (`--reorder requires --service …`), exit code 1,
  matching the spec, while clap `conflicts_with_all` still handles the flag
  combinations at exit code 2.
- **Decision:** `--reorder` peers are selected from the base service directly,
  ignoring `service_filter`. — **Because:** the spec scopes reordering to the
  base file's own service; `--all`/`--file` have no meaning for the write mode.

## Dependencies

No new crates. Uses `std`, existing `similar` (via `ChangeSet::render_diff`),
and existing parser helpers.

## Risks & mitigations

- **Risk:** Reordering past a pinned orphan line can interleave a key unit's
  lines around the orphan (unit no longer contiguous in the output).
  — **Mitigation:** this only occurs when a commented-out assignment / header
  sits between two keys of unequal block heights; behavior is deterministic,
  byte-preserving, and covered by a unit test documenting the exact output.
- **Risk:** A unit with an attached comment block swapping with a bare key
  changes where blank separators land. — **Mitigation:** trailing blanks are
  part of the moving unit, so each key keeps its spacing; accepted as the
  defined semantics and locked in by tests.
- **Risk:** Missing/unreadable peer files. — **Mitigation:** `ChangeSet::reorder`
  skips them silently (spec).
- **Risk:** The diff preview for a heavily reordered file is large.
  — **Mitigation:** same `TextDiff` rendering as `set`/`remove`/`gen`; the user
  confirms or aborts; `--dry-run` shows it without writing.

## Testing strategy

- **Parser unit tests** (per format, in the sub-parser test modules):
  - dotenv: simple reorder; extras to bottom preserving relative order; attached
    comment block moves with its key; blank-line-separated header stays put;
    commented-out assignment stays put and breaks attachment; already-in-order
    returns identical content; CRLF preserved; trailing newline preserved;
    duplicate keys use first occurrence.
  - flat YAML: same core cases; empty-value keys are not treated as keys.
  - k8s YAML: children reordered within each block independently; block header,
    indentation, and comments at block level untouched; no key crosses blocks;
    empty block unchanged.
  - Shared: orphan + variable-height unit interleaving produces the documented
    exact output (regression test).
- **`edit.rs` tests:** `ChangeSet::reorder` produces effective changes only for
  files whose content differs; missing/unreadable peers are skipped; `apply`
  writes exactly the changed peers.
- **`compare.rs` tests:** base order dedup; peer target selection excludes the
  base file and skipped files.
- **Integration/manual:** build a temp service with `nv.yml`, run
  `nv compare <file> --reorder --service <svc>` and check: diff shown, confirm
  prompt, file rewritten; `--dry-run` does not write; declining aborts;
  `--yes` writes; no `--service` errors with the required message; combining
  `--reorder` with `--values`/`--order`/`--comments` errors with exit code 2.
- **Verify:** `cargo build` (no warnings), `cargo test`, `cargo clippy`,
  `cargo fmt --check`.

## Rollout / migration

No `nv.yml` schema changes; `compare.skip_files` already applies. `--reorder` is
purely opt-in — plain `nv compare` stays read-only. The reorder algorithm is new
code; existing files are only ever touched when the user explicitly runs
`--reorder`, so there is no migration path for existing data.
