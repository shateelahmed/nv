# Tasks: `nv changes` command

- **Spec:** [spec.md](./spec.md)
- **Plan:** [plan.md](./plan.md)

Work through tasks top to bottom. Keep each task small enough to complete and
verify independently. Mark `[x]` when done.

## Tasks

^- [x] **T1 — `commands.changes` config schema and merge helpers**
  - Scope: Add `ChangesConfig { master_branch: Option<String>, skip_files:
    Vec<String> }`, wire `changes: Option<ChangesConfig>` into
    `CommandsConfig`, and add `changes_master_branch_for(&self, service) ->
    Option<&str>` (per-service overrides global; caller defaults to `master`)
    plus `changes_skip_files_for(&self, service) -> Vec<&str>` (global +
    per-service merged, deduplicated) mirroring the compare/find helpers.
  - Files: `src/config.rs`
  - Verify: `cargo test config` — new tests for global/per-service/merge of
    `master_branch` and `skip_files`, and empty-when-unconfigured; `cargo build`
    clean.

^- [x] **T2 — `src/git.rs` branch-content helpers**
  - Scope: `repo_root(dir) -> Result<PathBuf>` (`git rev-parse
    --show-toplevel`), `branch_exists(repo, branch) -> Result<bool>`
    (`git rev-parse --verify --quiet refs/heads/<branch>`), `read_file_at(repo,
    branch, rel) -> Result<Option<String>>` (`git cat-file -e` then `git show`;
    `None` when missing on the branch), and `rel_to_repo(repo, abs) ->
    Result<String>` for repo-root-relative paths. Register `pub mod git;`. All
    git calls pass arguments (no shell); invalid UTF-8 read lossy.
  - Files: `src/git.rs`, `src/lib.rs`/`src/main.rs` module registration
  - Verify: `cargo test git` — tests create a throwaway repo in a temp dir
    (`git init -b master`, commit, new branch, modified file) and assert repo
    root discovery, branch existence (present/absent), reading an existing file,
    and `None` for a file missing on a branch. (Transient `dead_code` warnings
    are expected until T4 wires the command.)

^- [x] **T3 — change computation and markdown rendering (pure core)**
  - Scope: In `src/cli/changes.rs`, pure functions: `branch_diff(from_pairs,
    to_pairs, kind) -> FileDiff` (created/deleted/updated), `aggregate` across
    files (service-wide `secrets_deleted`, `configmap_deleted`,
    `configmap_to_values`), `annotate` applying `(from secrets)` /
    `(from configmap)` (with the plain-text value from the `--to` configmap),
    and `render_markdown(report, from_branch) -> String` producing the
    `## <env>` (per environment, derived from the folder path; branch fallback)
    / `### configmap` / `### secrets` / `<strong>` layout
    (only non-empty sections; configmap bullets `key: value`, secrets bullets
    bare `key`, secrets never have an Updated section, newlines escaped).
  - Files: `src/cli/changes.rs` (+ `mod changes;` in `src/cli/mod.rs`)
  - Verify: `cargo test changes` — unit tests covering created/updated/deleted,
    secrets redaction, both annotation directions, plain-text value lookup, and
    the exact markdown layout from the spec example.

^- [x] **T4 — CLI surface, `run()`, terminal output, and prompts**
  - Scope: Add `Command::Changes { from: String, to: Option<String> }` with
    `--from` required and `--to` optional; dispatch to `changes::run(&cli,
    &from, to)`. Implement `run`: banner; exactly-one-`--service` check (error
    before any git work); resolve service; effective `--to` (flag, else
    `changes_master_branch_for`, else `master`); repo-root + branch validation
    with clear errors; collect ConfigMap/Secret files excluding merged
    `changes.skip_files`; read both branch contents; compute + annotate; render
    the standard tree (`+`/`-` labels in added/removed colors, updated as a
    `- old`/`+ new` pair, annotations appended); `No changes found.` when empty
    (no prompt); otherwise prompt for a markdown report (skipped with
    `--dry-run`) and a file name defaulting to `<service>-env-changes.md`,
    written into the service root.
  - Files: `src/cli/mod.rs`, `src/cli/changes.rs`
  - Verify: `cargo build` clean (no warnings); `cargo test changes`; smoke test
    against a temp per-service repo with two branches covering: default `--to`,
    `(from secrets)`/`(from configmap)` moves, secrets redaction, `--dry-run`,
    markdown write into the service root with default and custom names, and the
    no-`--service` / not-a-repo / missing-branch error paths.

^- [x] **T5 — Documentation**
  - Scope: Add a `changes` row to the command table and an example under the
    README compare section; document `commands.changes` (`master_branch`,
    `skip_files`) in `nv.yml.example` (global + per-service).
  - Files: `README.md`, `nv.yml.example`
  - Verify: `cargo build`; docs read correctly.

^- [x] **T6 — Full verification**
  - Scope: Run the standard checks and walk the spec's acceptance criteria.
  - Files: —
  - Verify: `cargo build` (no warnings), `cargo test`, `cargo clippy`,
    `cargo fmt --check`; all acceptance criteria in spec 015 met.

## Verification checklist

- [x] `cargo build` succeeds with no warnings (zero warnings in new code;
      the only `cargo clippy` warnings are 6 pre-existing ones in
      `src/cli/duplicates.rs` / `src/cli/find.rs`, present before this spec).
- [x] `cargo test` passes (260 unit + 13 integration, all green).
- [x] `cargo clippy` is clean for all new code (`changes.rs`, `git.rs`).
- [x] `cargo fmt --check` clean.
- [x] All acceptance criteria in the spec are met.

## Post-spec fixes

### Interactive prompt fix

During T6 verification a real interactive bug was found and fixed: dialoguer's
`Confirm` returns on the `y` keypress without consuming the Enter
(`wait_for_newline` defaults to `false`), so the leaked Enter immediately
accepted the default file name in the following `Input` prompt — users could
never type a custom name. Fixed by adding `.wait_for_newline(true)` to the
`Confirm` in `changes::run`; verified end-to-end via `expect` that typing
`y` + Enter then a custom file name writes `<name>` in the service root.

### T7 — Per-environment classification + environment filter

Comparing against a real service (`pol-payment-core-ms`,
`feature/gitleaks` → `master`) showed the keys-finding logic was wrong:
annotations crossed environments (a dev secret annotated `(from configmap)`
because of qa1/uat1 deletions), the configmap Deleted count was inflated (46 vs
the expected 13), and moved keys were double-counted. Fixed and folded into the
spec under "Post-approval revisions":

- [x] **T7.1 — Per-environment grouping:** move detection/suppression scoped to
      the file's parent directory (`env_group`, `GroupMeta`), not service-wide.
- [x] **T7.2 — Move suppression:** configmap Deleted items for keys that still
      live in the same environment's secrets on `--from` are suppressed.
- [x] **T7.3 — Wording:** `(from secrets)` → `(from secret)`; dropped
      `in plain text` (`KEY: value (from configmap)`).
- [x] **T7.4 — Always-render subsections:** empty Created/Updated/Deleted
      subsections of a present kind render; H2 heading param added.
- [x] **T7.5 — `--environment` + config:** `Command::Changes { environment }`,
      `ChangesConfig.environment`, `changes_environment_for` merge helper, env
      filtering in `run`, no-match error.
- [x] **T7.6 — Tests & docs:** unit tests (annotations, empty sections, config
      helper), smoke tests (counts 8→7, `--environment` flag/config/error), spec,
      plan, tasks, README, nv.yml.example.
- [x] **T7.7 — Verification:** `cargo build`/`test`/`clippy`/`fmt` clean;
      real-repo run for `pol-payment-core-ms` dev matches the reference output.
- [x] **T7.8 — Always environment-wise markdown:** `render_markdown` now groups
      `FileReport`s by environment derived from the folder path (`env_name`,
      `ENV_WRAPPER_DIRS`, fallback to `--from` branch), rendering one `## <env>`
      section per environment sorted by name (e.g. `dev`, `prod`, `qa1`);
      `write_markdown_file`/`run` drop the heading param. Unit tests for
      multi-environment segmentation + branch fallback; verified on
      `pol-payment-core-ms` (all 5 environments render; `--environment dev`
      still yields a single `## dev`).
- [x] **T7.9 — Placeholder values + branches header:** a secrets Created key
      moved out of a configmap is annotated `(from configmap)` only when the
      configmap holds a real value — redacted placeholders (`xxxx…`, empty)
      render as a bare key (`has_real_value`). The markdown report opens with a
      `# Comparing branches: <from> → <to>` line. Unit test
      `build_report_skips_placeholder_configmap_moves`; verified on
      `pol-payment-core-ms` (SSL_IV and prod placeholder keys now bare; real
      values like `BKASH_SFTP_PASSWORD: 62Q%dvWxTs79 (from configmap)` keep
      their annotation).
- [x] **T7.10 — Merged "Updated" sections + `(No changes)` markers:** Created
      and Updated changes share one `<strong>Updated</strong>` section (sorted
      by key); secrets use the "Updated" label for their created keys. Every
      label renders even when empty — environments (`## <env> (No changes)`),
      kinds (`### configmap (No changes)`), and subsections
      (`<strong>Updated</strong> (No changes)`) — with the marker in green
      (`kind_sections`, `render_subsection`, `no_changes_marker`). Unit tests
      `markdown_marks_empty_labels_no_changes`,
      `markdown_environment_with_no_changes`; verified on `pol-payment-core-ms`
      (all 5 envs render both kinds with merged Updated/Deleted).
