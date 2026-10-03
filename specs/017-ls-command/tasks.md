# Tasks: `nv ls` command

- **Spec:** [spec.md](./spec.md)
- **Plan:** [plan.md](./plan.md)

Work through tasks top to bottom. Keep each task small enough to complete and
verify independently. Mark `[x]` when done.

## Tasks

- [x] **T1 — Add `Ls` variant to the CLI**
  - Scope: Add a `Ls { environment: String }` variant to `Command` (uses global
    `-s`/`--service`). Dispatch to `ls::run` in `run()`.
  - Files: `src/cli/mod.rs`
  - Verify: `cargo build`; `nv ls` and `nv ls -e dev` exit with errors.

- [x] **T2 — Create `ls.rs` with service lookup and env matching**
  - Scope: New file `src/cli/ls.rs`. Resolve the context, find the service by
    name (bail if not found), and match the environment string against each
    file's path via segment matching. Collect configmap/secrets key names from
    matching files using `parser::parse`.
  - Files: `src/cli/ls.rs`
  - Verify: `cargo test cli::ls::tests` — 5 tests pass.

- [x] **T3 — Collect `.env` keys**
  - Scope: All `FileKind::Dotenv` files are included (skip `DotenvExample`).
    Parsed via `parser::parse` and merged into the key set.
  - Files: `src/cli/ls.rs`

- [x] **T4 — Collect Dockerfile ARG/ENV keys**
  - Scope: Glob for `Dockerfile*` under the service root. Parse each for
    `ARG` and `ENV` directive names via regex. Unit-tested with multiline
    ENV and empty content cases.
  - Files: `src/cli/ls.rs`

- [x] **T5 — Output and end-to-end**
  - Scope: Deduplicated via `BTreeSet` (alphabetical). One key per line.
    `No keys found.` on stderr when empty. Verified on real repo:
    `pol-payment-core-ms` with `-e dev` and `-e prod`.
  - Files: `src/cli/ls.rs`

- [x] **T6 — Document `nv ls` in README**
  - Scope: Row in commands table and two examples under `# Examples`.
  - Files: `README.md`

## Verification checklist

- [x] `cargo build` succeeds with no warnings.
- [x] `cargo test` passes (282 unit + 13 integration).
- [x] `cargo clippy` is clean.
- [x] `cargo fmt` applied.
- [x] All acceptance criteria in the spec are met.
