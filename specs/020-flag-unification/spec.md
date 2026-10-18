# Feature Specification: 020 - Flag unification and canonical CLI surface

**Feature Branch**: `020-flag-unification`
**Created**: Sun Oct 18 2026
**Status**: Implemented
**Input**: User request: "I want all commands to follow the same uniform pattern for flags and everything."

## User Story

As a user of nv, I want all commands to share a consistent, predictable flag surface so that once I learn the global flags (service filtering, file-kind filtering, dry-run/yes), they work the same way across every command.

## Requirements

### Functional Requirements

1. **All service targeting** uses only the global `-s/--service` (repeatable) and global `--all`. No subcommand may declare its own `-s/--service` flags.
2. **All file-kind targeting** uses only the global `-f/--file` (repeatable; values: `dotenv`, `dotenv_example`, `configmap`, `secret`) and global `--all`. No subcommand may declare parallel scope flags that overlap this (e.g., `-e/-c/-x/-a`).
3. **Encrypt/Decrypt** use the global service filter (`-s/--service`) and a new command-specific `--path` (or accept path via positional args as specified) to identify the exact file relative to the service; they **do not** use `-S`/`-F`.
4. **Ls** and **Missing** retain command-specific environment scoping: `-e/--environment` (optional/required as previously documented). This does not conflict with file-kind targeting because it scopes by environment path segments, not file kinds.
5. **Remove** drops its subcommand flags `-e/-c/-x/-a` and instead respects global `-f/--file` and `--all`. Behavior: when no file kinds are specified and not `--all`, the command behaves consistently with the chosen convention (defaulting to the same target set as documented).
6. **Unused** and **Duplicates** drop their subcommand `-s/--service` flags and rely solely on global service filtering.
7. **The global `--all`** is the single mechanism to target everything (services and files), ignoring filters. All commands honor it uniformly.
8. **"Exactly one service" checks** are centralized (or applied uniformly). `ls` honors `--all` consistently with other commands (if `--all` is given, `ls` does not require exactly one service).
9. **Backwards compatibility** is intentionally broken for removed/renamed flags as part of this unification (the goal is a canonical surface).

### Non-Functional Requirements

1. **Consistency**: flag names, casing, and semantics are identical across all subcommands.
2. **Clarity**: global flags remain in `Cli` with `global = true`; command-specific flags are minimal and orthogonal (environment, paths, mode flags like `--clean`, `--flat`, etc.).
3. **Durability**: this is a durable CLI convention change (per spec 001). Assistant configs must be updated together if they document CLI flags.

## Acceptance Criteria

### AC1: Remove uses only global flags
- Given `nv remove KEY` with no global `-f/--file` and no `--all`, When parsed and run, Then the command applies a consistent default (all file kinds) and does not require subcommand `-e/-c/-x/-a`.
- Given `nv remove KEY -f configmap`, When run, Then only configmap files are targeted.
- Given `nv remove KEY --all`, When run, Then all services and file kinds are targeted (subject to global filters).

### AC2: Unused/Duplicates rely on global -s/--all
- Given `nv unused -s auth`, When run, Then it uses global service filtering (no subcommand `-s` flag exists).
- Given `nv duplicates --all`, When run, Then all services are scanned.

### AC3: Encrypt/Decrypt use -s and --path
- Given `nv encrypt --key k --service auth --path src/.env.example` (or equivalent with global `-s auth`), Then the command accepts `--path` (not `-F`) and uses global `-s`.
- Given `nv decrypt --key k -s auth --path src/.env.example`, Then it succeeds with the unified flag shape.

### AC4: Single-service commands reject `--all` uniformly
- Given `nv ls --all -e dev`, When run, Then it fails with the same "exactly one `--service`" error that `nv changes` uses (both `ls` and `changes` are single-service and share one helper). The error message is identical across commands.
- Given `nv ls -s auth -s billing -e dev`, When run, Then it fails with the same error (more than one service).

### AC5: Help text and error messages are consistent
- Every command's help reflects the unified surface; error messages about filters are consistent.

### AC6: Assistant configs updated
- Per spec 001, since this changes a durable CLI convention (flag naming/semantics), both assistant configs (`.github/*` and `CLAUDE.md`/`.claude/*`) are updated together in the same change set.

## Open Questions

- Default file kinds for `remove` when no `-f` and no `--all` are specified: should it default to all kinds (current behavior when `-s` given without `-e/-c/-x`)? Yes.
- Should global `--all` gain a short `-a`? Optional, but can reduce friction after removing Remove's `-a`. Decision: include `-a` as a short alias for global `--all` for uniformity.
