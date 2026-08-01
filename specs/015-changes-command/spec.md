# Spec: `nv changes` command

- **ID:** 015-changes-command
- **Status:** Approved
- **Author:** shateel
- **Date:** 2026-08-01

## Summary

A `nv changes --service <name> --from <branch> [--to <branch>]` command that lists
every environment-key change in a service's configmap and secrets files by
comparing two git branches of that service's repository. Results are shown in the
terminal and optionally written to a markdown report.

## Problem / Motivation

Platform engineers need a quick, readable summary of what environment keys
changed in a service between two branches (typically a feature branch and the
master branch) — keys created, updated, or deleted in `configmap*.yml` and
`secrets*.yml`. Doing this by hand requires `git diff` against each file plus
manual grouping, and it misses meaningful cross-kind signals such as a secret
that was moved into a configmap (or vice versa). `nv changes` makes this a
single command and surfaces those moves explicitly.

## Goals

- List created, updated, and deleted env keys for a service's configmap and
  secrets files between two branches of that service's git repository.
- Default the baseline branch to the service's configured master branch name
  (global or per-service `nv.yml` config), falling back to `master`.
- Support per-service and global `skip_files` filtering, matching `nv compare`.
- Flag keys that moved between file kinds (`(from secrets)` / `(from configmap)`).
- Show secret values only when a secret moved out of a configmap; otherwise
  redact them.
- Render results in the standard terminal tree format, then optionally write a
  markdown report.

## Non-goals

- Comparing dotenv / dotenv_example files (configmap and secrets only).
- Comparing across services in one invocation.
- Writing, applying, or migrating any file content.
- Any network or remote git operations (local repository branches only).
- Terminal output that matches the markdown format (the markdown layout is for
  the generated file only).

## User stories

- As a platform engineer, I want to see which configmap keys a branch creates,
  updates, or deletes relative to master, so I can review a PR's env changes.
- As a developer, I want the command to tell me when a key moved from secrets
  into a configmap (or the reverse), so I know the secret's value is now exposed
  or still safe.
- As a release manager, I want a markdown file I can attach to a PR describing
  all env changes for a service.

## Behavior & requirements

### Invocation

```
nv changes --service <NAME> --from <BRANCH> [--to <BRANCH>] [--environment <ENV>]

Options:
  --from <BRANCH>       Branch holding the NEW state (required).
  --to <BRANCH>         Baseline branch. Defaults to the service's configured
                        master branch name, or 'master' when not configured.
  --environment <ENV>   Restrict the scan to one environment folder, matched as
                        a path segment (e.g. `dev` for
                        `deploy/dev/kubernetes`). Defaults to the service's
                        configured `commands.changes.environment`, or all
                        environments when not configured.
```

- `--service` is the standard repeatable global flag. Exactly one service MUST be
  selected: if none is given the command MUST error (exit 1) before reading any
  repository, and if more than one is given it MUST error.
- `--from` is required; omitting it is a clap parse error (exit 2).
- `--to` is optional and defaults as described above.
- The command MUST print the standard config-source banner (`nv.yml` vs
  `command-line`) like every other command.

### Branch semantics

- `--from` is the "new" state and `--to` is the baseline.
- **Created**: key present in `--from`, absent in `--to`.
- **Deleted**: key present in `--to`, absent in `--from`.
- **Updated**: key present in both with different values (configmap only).
- `--from == --to` yields no changes.

### Scope

- Only the selected service's `configmap*.yml`/`.yaml` and `secrets*.yml`/`.yaml`
  files participate (the file kinds `ConfigMap` and `Secret`). Dotenv files are
  never scanned.
- Files are read from each branch of the service's own git repository (each
  service directory is its own repo). A file that does not exist on a branch is
  treated as empty for that branch.
- The service directory MUST be inside a git repository, otherwise the command
  MUST error with a clear message.
- Both branches MUST exist in that repository, otherwise the command MUST error
  with a clear message.

### Key changes

- Keys are matched by name. For configmap files, values are compared; a value
  present on one branch and absent on the other side is just created/deleted.
- **Configmap items** show the key's value from the `--from` branch for Created
  and Updated; Deleted items show the key name only (no value).
- **Secrets items** are redacted: Created and Deleted items show the key name
  only. Secrets values are NEVER shown except in the "moved from configmap" case
  below. Secrets have no Updated section: a key present in both branches is
  omitted even if its (hidden) value differs.

### Cross-kind move annotations

A key moved between kinds is annotated so the reader knows its value now lives
in a different file kind.

- **Configmap `(from secret)`:** when a key name was Deleted from the same
  environment's secrets files AND the same key was Created or Updated in a
  configmap between the branches, that configmap item is annotated
  `(from secret)`.
- **Secrets `(from configmap)`:** when a key name was Deleted from the same
  environment's configmap files AND the same key was Created in secrets between
  the branches, that secrets item is annotated `(from configmap)` AND shows the
  plain-text value the key had in the `--to` branch's configmap (`KEY: value
  (from configmap)`).
- Annotations never appear on Deleted items.
- Move detection and suppression are scoped **per environment**: the
  environment is the file's parent directory relative to the service root
  (e.g. `deploy/dev/kubernetes`), so a key moving in `dev` is never influenced
  by `prod` files.
- When a key moved from a configmap into secrets, the configmap Deleted item is
  suppressed — the move is reported only on the secrets side (Created), so it is
  not double-counted as both removed and added.

### Configuration (`nv.yml`)

New `changes` block under `commands:`, configurable globally and per-service,
mirroring the existing `compare`/`find` pattern.

**Global:**
```yaml
commands:
  changes:
    master_branch: main
    environment: dev
    skip_files:
      - deploy/secrets.yml
```

**Per-service:**
```yaml
services:
  auth:
    commands:
      changes:
        master_branch: master
        environment: prod
        skip_files:
          - configmap.local.yml
```

- `master_branch`: the branch name used when `--to` is omitted. Per-service value
  MUST override the global value; when neither is set, `master` is used.
- `environment`: restricts the scan to one environment folder (matched as a path
  segment of the file's path relative to the service root). Per-service value
  MUST override the global value; when neither is set, all environments are
  scanned. The `--environment` flag overrides both.
- `skip_files`: files excluded from the scan. Global and per-service lists MUST
  be merged (deduplicated), matching `compare.skip_files` semantics: glob
  patterns (`*`, `**`, `?`), matched against the path relative to the service
  root and the bare file name. Files matching a pattern are silently skipped.
- `skip_files` has no built-in defaults.

### Terminal output

Results MUST use the standard hierarchical colorized tree format (uniform output
rule): grouped by service, then by file (full path relative to the service
root), with tree-style vertical lines and `+`/`-` indicators in `added`/`removed`
colors. Suggested shape:

```
auth/
├── deploy/configmap.yml
│   ├── + NEW_KEY = value
│   ├── - DB_PASSWORD = old_value        (updated pair)
│   ├── + DB_PASSWORD = new_value        (updated pair)
│   ├── + MOVED_KEY = value (from secret)
│   └── - DELETED_KEY
└── deploy/secrets.yml
    ├── + MOVED_KEY = value (from configmap)
    └── - DELETED_SECRET
```

- Created items: `+ KEY = value`; Deleted items: `- KEY`; Updated items: a
  `- KEY = old_value` / `+ KEY = new_value` pair, consistent with `nv compare
  --values`.
- Annotation text is appended to the item label (`(from secret)` /
  `(from configmap)`). The moved-secret value is shown directly with the
  annotation; there is no `in plain text` phrasing.
- When there is nothing to report, the command prints `No changes found.` and
  does NOT prompt for a markdown file.
- When an `--environment`/configured environment matches no configmap/secrets
  file of the service, the command MUST error with a clear message.

### Markdown report prompt

- After the terminal output, the command MUST prompt: "Generate a markdown list
  of changes?" (default: no).
- On acceptance, the command MUST prompt for a file name, defaulting to
  `<service-name>-env-changes.md` (e.g. `auth-env-changes.md`), written to the
  root folder of the specified service (the service's own directory).
- `--dry-run` MUST skip the prompt and write nothing.
- Declining the prompt writes nothing.

### Markdown report format

The generated file opens with a line naming the compared branches, then is
always segmented by environment. Each environment with at least one change gets
its own `## <env>` section, and environments are ordered alphabetically (e.g.
`dev`, `optimization1`, `prod`, `qa1`, `uat1`). The environment name is derived
from the file's folder path — the first path segment that is not a container
folder such as `deploy/` or `kubernetes/` (`deploy/dev/kubernetes` → `dev`).
Files outside any environment folder fall back to the `--from` branch name as
their section title. Inside a section the layout from the example applies,
grouped by file kind with `<strong>` subsection headers:

```markdown
# Comparing branches: feature/gitleaks → master

## dev

### configmap
---
<strong>Updated</strong>

- key: value
- key2: value2 (from secret)

<strong>Deleted</strong>

- Key

### secrets
---
<strong>Updated</strong>

- key
- key2: value2 (from configmap)

<strong>Deleted</strong>

- Key

## prod

### configmap
---
...
```

- Every environment with changes is emitted, whether or not an environment
  filter is selected; `--environment` (or `commands.changes.environment`) only
  restricts which files are scanned, so the report still renders as `## <env>`
  per environment.
- Every label is rendered even when it has no changes, marked `(No changes)` in
  green: an environment with no changes renders just `## <env> (No changes)`, a
  kind with no changes renders just `### configmap (No changes)` (no `---` or
  subsections), and an empty subsection still renders its `<strong>` label.
- Created and Updated changes are merged into one `<strong>Updated</strong>`
  section (sorted by key); secrets use the same "Updated" label for their
  created keys, and secrets never have true value updates.
- Configmap bullets render as `- key: value` (Created/Updated use the `--from`
  value, Deleted renders the bare key).
- Secrets bullets render as `- key`; the moved-from-configmap case renders as
  `- key: value (from configmap)` only when the configmap holds a real value —
  redacted placeholders (`xxxx…`) or empty values render the bare key.
- This is a file artifact, not terminal display, so the uniform terminal-output
  rule does not apply to it.

## Acceptance criteria

- [ ] Given a service with `deploy/configmap.yml` containing `NEW_KEY` only on
      branch `dev` (absent on `master`), when `nv changes --service auth --from
      dev --to master` runs, the output shows `NEW_KEY` under configmap Created.
- [ ] Given a key present on `master` but absent on `dev`, when the command
      runs, the key appears under configmap Deleted as a bare key (no value).
- [ ] Given a key whose value differs between the branches, when the command
      runs, the key appears under configmap Updated showing both the old and new
      value.
- [ ] Given a key deleted from the service's secrets files and added to a
      configmap between the branches, when the command runs, the configmap item
      is annotated `(from secret)`.
- [ ] Given a key deleted from the service's configmap files and added to
      secrets between the branches, when the command runs, the secrets item is
      annotated `(from configmap)` and shows the plain-text value the key had in
      the `--to` branch's configmap; the configmap Deleted item is suppressed
      (not double-counted).
- [ ] Given a key moved from a configmap to secrets in one environment while
      the same key moves in the opposite direction in another environment, when
      the command runs, each move is detected within its own environment only
      and no annotation leaks across environments.
- [ ] Given a secrets key present in both branches, when the command runs, the
      key does NOT appear (no Updated section for secrets).
- [ ] Given a secrets key created on the from branch with no configmap move,
      when the command runs, the secrets item shows only the key name (no
      value).
- [ ] Given `--to` omitted and `commands.changes.master_branch: main`, when the
      command runs, the baseline is `main`.
- [ ] Given `--to` omitted and no `master_branch` config, when the command runs,
      the baseline is `master`.
- [ ] Given a per-service `master_branch` and a different global one, when the
      command runs for that service, the per-service value is used.
- [ ] Given `commands.changes.skip_files: [deploy/secrets.yml]` and a change in
      that file, when the command runs, the file is not scanned and its keys do
      not appear.
- [ ] Given global and per-service `skip_files` entries, when the command runs,
      both are honored (merged).
- [ ] Given `--environment dev` and a service with changes in both
      `deploy/dev/` and `deploy/prod/`, when the command runs, only the dev
      changes are reported and the markdown H2 is `## dev`.
- [ ] Given `commands.changes.environment: dev` and no `--environment` flag,
      when the command runs, only the dev changes are reported.
- [ ] Given a per-service `commands.changes.environment` and a different global
      one, when the command runs for that service, the per-service value is
      used; `--environment` overrides both.
- [ ] Given `--environment staging` and no `deploy/staging/` files in the
      service, when the command runs, it errors with a clear message.
- [ ] Given a dotenv change between the branches, when the command runs, dotenv
      files are never reported.
- [ ] Given `--dry-run`, when the command runs and would prompt for a markdown
      file, it does not prompt and writes nothing.
- [ ] Given changes and the markdown prompt accepted with the default name, a
      file named `<service-name>-env-changes.md` is written in the specified
      service's root folder with the `##`/`###`/`<strong>` layout and one
      `## <env>` section per environment.
- [ ] Given changes and the markdown prompt accepted with a custom file name,
      the file is written under that name.
- [ ] Given no changes, when the command runs, `No changes found.` is printed
      and no markdown prompt appears.
- [ ] Given no `--service`, when the command runs, it errors (exit 1) before
      touching the repository.
- [ ] Given a service whose directory is not inside a git repository, when the
      command runs, it errors with a clear message.
- [ ] Given a `--from` or `--to` branch that does not exist, when the command
      runs, it errors with a clear message.
- [ ] Given a configmap file that exists on one branch but not the other, when
      the command runs, the missing branch is treated as empty and all its keys
      are reported Created (or Deleted) accordingly.
- [ ] Given the config-source banner, when any invocation runs, the standard
      `Config source: nv.yml` / `command-line` banner is printed.

## Edge cases

- No `--service` → error before any git operation (exit 1). More than one
  `--service` → error.
- `--from` missing → clap parse error (exit 2).
- `--to` defaults to the effective `master_branch` config, else `master`.
- Service directory not a git repository → clear error.
- Branch (from or to) not found → clear error.
- File missing on a branch → treated as empty for that branch; its keys become
  Created (missing on `--to`) or Deleted (missing on `--from`).
- Files matching `changes.skip_files` → silently excluded.
- `--environment`/`commands.changes.environment` filters to files whose path
  contains a segment equal to the environment name; no matching file → clear
  error.
- Key moved between kinds → `(from secret)` on the configmap side (Created or
  Updated) / `(from configmap)` on the secrets side (Created), with the plain
  text value for the secrets side coming from the `--to` branch's configmap.
  The configmap Deleted item for a configmap→secrets move is suppressed.
- Move detection and suppression are scoped per environment (the file's parent
  directory relative to the service root).
- Secrets values are redacted everywhere except the moved-from-configmap case.
- Secrets have no Updated section; same-key-in-both-branches is omitted.
- Deleted items never carry annotations and show no values.
- Multiple configmap/secrets files in a service are scanned; annotation checks
  are per-environment by key name across the two kinds.
- Empty report → `No changes found.` and no markdown prompt.
- `--dry-run` suppresses the markdown prompt and any file write.
- The markdown file is written to the service's root folder (not the current
  directory).
- Markdown bullets deduplicate by key name within a file kind.

## Open questions

None — all behavior decisions were confirmed before writing:

- `--from`/`--to` flag naming and `--to` default behavior.
- Annotation semantics: move detection by same key name across kinds, scoped
  per environment.
- Secrets redaction rule and absence of a secrets Updated section.
- Terminal output uses the standard tree format; the markdown layout is file-only.
- Per-service git repositories; local branches only.

## Post-approval revisions

After approval, comparing against a real service
(`pol-payment-core-ms`, `feature/gitleaks` → `master`) surfaced classification
bugs that were fixed and folded back into this spec:

1. **Per-environment scoping.** Move detection was service-wide, so keys moving
   in one environment were annotated/deleted based on unrelated environments.
   Grouping now uses the file's parent directory (e.g. `deploy/dev/kubernetes`).
2. **Suppressed configmap Deleted on moves.** A key moved configmap→secrets was
   reported as both configmap Deleted and secrets Created; the configmap Deleted
   item is now suppressed (the move is reported once, on the secrets side).
3. **Annotation wording.** `(from secrets)` → `(from secret)`; dropped the
   `in plain text` phrasing (`KEY: value (from configmap)`).
4. **Always-render subsections.** Empty Created/Updated/Deleted subsections of a
   present kind are rendered (a configmap with only updates still shows an empty
   Created list).
5. **Environment filter.** Added `--environment <ENV>` and
   `commands.changes.environment` to restrict the scan to one environment
   folder.
6. **Always environment-wise markdown.** The markdown report is now always
   segmented by environment (`## dev`, `## prod`, `## qa1`, …), derived from
   each file's folder path with a fallback to the `--from` branch name; an
   environment filter only narrows which files are scanned, never the
   per-environment layout.
7. **Branches header + placeholder suppression.** The report opens with
   `# Comparing branches: <from> → <to>`, and the `(from configmap)` annotation
   (with its value) is only shown for real configmap values — placeholder
   values like `xxxxxxxxxxxxxxxxx` render as a bare secrets key.
8. **Merged "Updated" sections + `(No changes)` markers.** Created and Updated
   changes share one `Updated` section (secrets' created keys also use
   "Updated"), and every environment/kind/subsection label renders even when
   empty, marked `(No changes)` in green.

## Assistant-config sync

No assistant-config change is required. This spec adds a new command and a new
`commands.changes` config block but introduces no new cross-cutting rule beyond
conventions already captured in both assistant configs (uniform terminal output,
config-source banner, `skip_files` semantics inherited from `compare`). It does
require updating the user-facing docs (`README.md`, `nv.yml.example`) and a new
`src/cli/changes.rs` module.
