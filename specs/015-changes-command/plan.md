# Plan: `nv changes` command

- **Spec:** [spec.md](./spec.md)
- **Status:** Approved

## Overview

`nv changes` reads a service's configmap and secrets files at two git branches
of that service's repository, computes created/updated/deleted key sets, flags
keys that moved between the two file kinds, and reports the result in the
standard terminal tree format. On request it writes a markdown report into the
service's root folder.

Reading branch content is done non-destructively with the `git` CLI
(`git -C <repo> show <branch>:<path>`); the working tree is never touched. No
new crates are needed.

## Architecture & modules

| Module | Change | Responsibility |
| --- | --- | --- |
| `src/config.rs` | edit | `ChangesConfig` (`master_branch`, `environment`, `skip_files`), wire into `CommandsConfig`, merge helpers. |
| `src/git.rs` | new | Repo-root discovery, branch existence, reading a file's content at a branch via the `git` CLI. |
| `src/cli/changes.rs` | new | Command handler: branch diff computation, per-environment cross-kind annotation, terminal rendering, markdown generation and writing, prompts. |
| `src/cli/mod.rs` | edit | `Command::Changes` variant (`--from` required, `--to` and `--environment` optional) and dispatch. |
| `README.md` | edit | Document the command and config. |
| `nv.yml.example` | edit | Document `commands.changes`. |

## Data flow

1. CLI parses `nv changes --service NAME --from BRANCH [--to BRANCH]
   [--environment ENV]`.
2. `changes::run` prints the config-source banner, resolves the `Context`, and
   requires exactly one `--service`.
3. Effective `--to` = provided value, else the service's effective
   `master_branch` config, else `master`.
4. Effective environment filter = `--environment`, else the service's effective
   `environment` config, else none (all environments).
5. Locate the service, validate its repo root and that both branches exist.
6. Collect the service's `ConfigMap`/`Secret` files, excluding `changes.skip_files`
   matches (merged global + per-service) and any file outside the selected
   environment.
7. For each file, read its content at `--to` and `--from` (missing on a branch →
   empty); parse keys/values with the existing parser.
8. Compute per-file Created/Deleted/Updated; aggregate per-environment
   (grouped by the file's parent directory) "deleted from secrets" /
   "deleted from configmap" sets and the `--to` configmap values.
9. Suppress a configmap Deleted item whose key still lives in the same
   environment's secrets on the `--from` branch (a move, reported only on the
   secrets side).
10. Annotate within each environment: configmap Created/Updated → `(from secret)`;
    secrets Created → `(from configmap)` with the old configmap value.
11. Render the terminal tree; print the change count. If nothing changed, print
    `No changes found.` and stop.
12. Unless `--dry-run`, prompt for a markdown report, then for a file name
    (default `<service>-env-changes.md`) and write it into the service root:
    a `# Comparing branches: <from> → <to>` header, then per-environment sections
    (`## <env>` per environment, derived from each file's folder path, sorted
    by name). Empty labels render `(No changes)` in green; Created and Updated
    changes share one `<strong>Updated</strong>` section.

## Key decisions & trade-offs

- **Decision:** Shell out to the `git` CLI instead of adding the `git2` crate.
  — **Because:** no new dependency, git is guaranteed present (the feature is
  inherently about git), and the operations are simple subcommands. —
  **Alternatives:** `git2` (heavy native dependency, build complexity); parsing
  `.git` internals (fragile, unsupported).
- **Decision:** Read branch content via `git show <branch>:<path>`.
  — **Because:** non-destructive; the working tree and user's checkout state are
  never modified (spec non-goal). — **Alternatives:** `git checkout` (mutates the
  tree), temporary worktrees (slow, leaky).
- **Decision:** Resolve the repo root with `git rev-parse --show-toplevel` and
  compute file paths relative to it. — **Because:** handles both "service dir is
  the repo" and nested layouts; `git show` pathspecs are repo-root-relative. —
  **Alternatives:** assuming `service.path` is the repo root (breaks when it
  isn't).
- **Decision:** Validate both branches via `git rev-parse --verify --quiet
  refs/heads/<branch>`. — **Because:** spec scopes to local branches; a missing
  branch must error before any scan. — **Alternatives:** silently treating a
  missing branch as empty (misleading output).
- **Decision:** Reuse `parser::parse` for key/value extraction, as `nv compare`
  does. — **Because:** one source of truth for "what counts as a key"; matches
  compare's kind table for configmap/secrets. — **Alternatives:** a new custom
  reader (divergent key semantics).
- **Decision:** Compute diffs, annotations, and markdown with pure functions
  operating on parsed branch contents. — **Because:** the logic is the risky
  part and must be unit-testable without git or files. — **Alternatives:**
  inlining everything in `run` (untestable, hard to review).
- **Decision:** Config additions follow the existing `commands.compare` /
  `commands.find` pattern (`Option` fields, `skip_serializing_if`, merge
  helpers). — **Because:** consistent with the established schema and keeps
  `nv.yml` output tidy. — **Alternatives:** a bespoke schema (inconsistent).
- **Decision:** In the terminal, Updated configmap keys render as a
  `- KEY = old` / `+ KEY = new` pair (compare `--values` convention); in
  markdown they render as `key: new_value` per the spec example.
  — **Because:** the terminal must satisfy the uniform `+`/`-` tree convention
  while the markdown follows the approved layout. — **Alternatives:** a `~`
  marker (breaks uniform output), two bullets in markdown (deviates from
  example).
- **Decision:** Values with embedded newlines are rendered single-line
  (newlines escaped as `\n`) in both outputs. — **Because:** multiline YAML
  scalars would otherwise break tree items and markdown bullets. —
  **Alternatives:** raw multiline rendering (breaks the formats).
- **Decision:** Group files by parent directory (relative to the service root)
  for move detection and suppression, so annotations never cross environments.
  — **Because:** real services store one configmap/secrets pair per environment
  folder (`deploy/dev/kubernetes`), and service-wide aggregation caused bogus
  annotations and inflated Deleted counts. — **Alternatives:** service-wide
  aggregation (the original buggy approach).
- **Decision:** Suppress a configmap Deleted item when the same key lives in the
  same environment's secrets on the `--from` branch. — **Because:** the move is
  already reported as a secrets Created; listing it again as configmap Deleted
  double-counts it. — **Alternatives:** reporting both sides (misleading counts).
- **Decision:** Restrict the scan with `--environment <ENV>` /
  `commands.changes.environment`, matched as any path segment equal to the
  environment name. — **Because:** users review per-environment diffs (the
  reference output was dev-only). — **Alternatives:** scanning all environments
  always (the default, still supported).
- **Decision:** Always segment the markdown report by environment, deriving the
  `## <env>` title from each file's folder path (the first segment that is not a
  container folder, e.g. `deploy/dev/kubernetes` → `dev`), falling back to the
  `--from` branch name for files outside any environment folder. — **Because:**
  the reference output is a per-environment list; an environment filter narrows
  the scan but must not change the layout. — **Alternatives:** a single H2 from
  the filter/branch (the original design, dropped after the environment-wise
  reference was confirmed).

## Dependencies

No new crates. Requires the `git` executable at runtime (already a standard dev
tool). The `git` subprocess is invoked with `std::process::Command` and
argument passing (no shell), so branch/file names cannot inject commands.

## Risks & mitigations

- **Risk:** git not installed. — **Mitigation:** clear error when spawning
  `git` fails.
- **Risk:** service directory not inside a repo, or a branch missing.
  — **Mitigation:** validate repo root and both branches up front with explicit
  error messages before scanning.
- **Risk:** a file was renamed between branches. — **Mitigation:** treated as
  delete + create under key-name semantics (documented behavior, not a bug).
- **Risk:** very large branch contents. — **Mitigation:** pure string parsing
  like the existing compare command; no extra copies beyond one read per file
  per branch.
- **Risk:** path/branch names with special characters. — **Mitigation:**
  argument passing without a shell; failures surface as git errors with context.

## Testing strategy

- Unit tests: `config.rs` merge helpers (`changes_master_branch_for`,
  `changes_environment_for`, `changes_skip_files_for`); `git.rs` against
  throwaway repositories created in a temp dir (`git init`, commit, branch);
  `changes.rs` pure diff/annotation logic and markdown rendering (including
  empty-subsection rendering and per-environment suppression).
- Integration/manual: smoke test against a temp per-service repo with two
  branches (like spec 014), covering default `--to`, move annotations, secrets
  redaction, `--dry-run`, `--environment` filtering and its no-match error,
  markdown write to the service root, and error paths.

## Rollout / migration

New optional `commands.changes` keys only; existing `nv.yml` files remain valid
(serde defaults). `master_branch` defaults to `master`, preserving current
behavior when unset. No file-format migration. `README.md` and `nv.yml.example`
gain documentation for the new command and config.
