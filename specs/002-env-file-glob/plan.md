# Plan: Support `.env*` file glob

- **Spec:** [spec.md](./spec.md)
- **Status:** Approved

## Overview

Simplify `detect_kind()` to treat any `.env*` file as a dotenv file (excluding
swap/backup artifacts), and ensure the no-real-secrets rule applies to any file
containing `.example` in its name.

## Architecture & modules

| Module | Change | Responsibility |
| --- | --- | --- |
| `src/parser/mod.rs` | edit | Simplify `detect_kind()` glob matching |
| `src/model.rs` | no change | `FileKind::is_example()` already covers `DotenvExample` |
| docs (`CLAUDE.md`, etc.) | edit | Update env file list from `.env`, `.env.example` to `.env*` |

## Key decisions

- **Keep `FileKind` as two dotenv variants.** `Dotenv` and `DotenvExample`
  remain sufficient because `detect_kind()` routes `.example`-containing names
  to `DotenvExample`. No new enum variant needed.
- **Exclude swap/backup by suffix.** Files ending in `~`, `.swp`, `.swo`,
  `.bak`, `.tmp` are skipped even if they start with `.env.`.

## Testing strategy

- Unit tests in `src/parser/mod.rs` for `detect_kind()` covering:
  `.env.local`, `.env.testing.example`, `.env.swp`, `.env~`, `.env.` (trailing dot)
- Existing tests continue to pass unchanged.

## Rollout / migration

Documentation-only update for the file type list. No config or file-format
changes.
