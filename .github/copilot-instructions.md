# Copilot instructions for `nv`

`nv` (pronounced *envy*) is a Rust CLI + TUI for configuring environment variables
across multiple microservices. It edits `.env`, `.env.example`, `configmap*.yml`,
and `secrets*.yml` files **while preserving comments and formatting**.

## Golden rules

1. **File edits must preserve formatting.** Never re-serialize whole files.
   Editing works by locating a key and changing only its value line (or inserting
   a single new line). Unrelated lines, comments, and ordering MUST stay
   byte-identical. All new parsing/writing goes through `src/parser/`.
2. **YAML is edited line-by-line, not via a round-trip serializer.** There is no
   comment-preserving YAML serializer in the stack; do not introduce one for
   writes. `serde_yaml` is only used where structure reading is acceptable.
3. **`.env.example` never receives real secret values.** Generated secrets are
   written empty there.
4. **Secrets are raw strings.** No base64 encode/decode of Kubernetes `data:`.
5. **Every command reports its config source** (`nv.yml` vs `command-line`).

## Spec-driven development

Follow the workflow in [`../specs/README.md`](../specs/README.md):
**specify → plan → tasks → implement**. Do not write feature code before a spec
exists and is approved. Use the `/specify`, `/plan`, `/tasks`, and `/implement`
prompts. Artifacts live in `specs/NNN-slug/`.

**Keep both assistants in sync (spec 001).** When a new spec introduces or changes
a durable rule, convention, workflow step, or project guarantee, apply the
equivalent change to **both** assistant configs in the same change set: the
Copilot files (`.github/copilot-instructions.md`, `.github/prompts/*`) and the
Claude files (`CLAUDE.md`, `.claude/commands/*`). Syntax may differ; behavior must
not. If a spec has no cross-cutting rule, note that no assistant-config change is
required.

## Rust conventions

- Edition 2024. `gen` is a reserved keyword — the generate command module is
  `src/cli/generate.rs`.
- `rand` 0.10: `random()` and `random_range()` come from the `rand::RngExt` trait;
  use `rand::rng()` for a thread RNG.
- Prefer `anyhow::Result` at boundaries; return errors, don't `panic!`/`unwrap()`
  outside tests.
- Keep functions small and documented with `///`. Match the existing module
  layout (see the table in `README.md`).
- Add unit tests next to the code (`#[cfg(test)] mod tests`). Parser and edit
  logic changes MUST include tests proving formatting is preserved.

## Build & verify

Run these before considering work complete:

```sh
cargo build      # no warnings
cargo test       # all green
cargo clippy     # clean
cargo fmt        # formatted
```

Note: `cargo` needs to write to `~/.cargo` and `target/`; run it outside any
restrictive sandbox.

## Behavior expectations

- New keys are auto-created. In Kubernetes YAML they are inserted under
  `data:`/`stringData:` at the correct indentation; `stringData:` is preferred
  when both exist.
- `nv gen` broadcasts one shared secret by default; `--unique` produces a distinct
  secret per target.
- Filters: no `--service`/`--file` means "all"; `--all` explicitly ignores
  filters.
