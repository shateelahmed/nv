# CLAUDE.md

Guidance for Claude Code when working in this repository.

## Project

`nv` (pronounced *envy*) is a Rust CLI + TUI that configures environment variables
across multiple microservices. It edits `.env`, `.env.example`, `configmap*.yml`,
and `secrets*.yml` **while preserving comments and formatting**.

## Golden rules (do not violate)

1. **Preserve formatting.** Never re-serialize whole files. Edits locate a key and
   change only its value line (or insert one line). Comments, ordering, and
   unrelated lines MUST remain byte-identical. All parsing/writing goes through
   `src/parser/`.
2. **YAML writes are line-oriented.** Do not add a comment-preserving YAML
   serializer. `serde_yaml` is only for reading structure.
3. **`.env.example` never gets real secret values** — generated secrets are empty
   there.
4. **Secrets are raw strings.** No base64 for Kubernetes `data:`.
5. **Every command reports its config source** (`nv.yml` vs `command-line`).

## Spec-driven development

Feature work follows **specify → plan → tasks → implement** (see
[specs/README.md](specs/README.md)). Do not write feature code before an approved
`spec.md` exists. Use the slash commands in `.claude/commands/`: `/specify`,
`/plan`, `/tasks`, `/implement`. Artifacts live in `specs/NNN-slug/`.

**Keep both assistants in sync (spec 001).** When a new spec introduces or changes
a durable rule, convention, workflow step, or project guarantee, apply the
equivalent change to **both** assistant configs in the same change set: the Claude
files (`CLAUDE.md`, `.claude/commands/*`) and the Copilot files
(`.github/copilot-instructions.md`, `.github/prompts/*`). Syntax may differ;
behavior must not. If a spec has no cross-cutting rule, note that no
assistant-config change is required.

## Rust conventions

- Edition 2024. `gen` is reserved — the generate command module is
  `src/cli/generate.rs`.
- `rand` 0.10: `random()` / `random_range()` are on the `rand::RngExt` trait;
  `rand::rng()` gives a thread RNG.
- Use `anyhow::Result` at boundaries; avoid `unwrap()`/`panic!` outside tests.
- Document items with `///`; keep functions small. Tests live in
  `#[cfg(test)] mod tests` next to the code. Parser/edit changes require
  formatting-preservation tests.

## Build & verify

```sh
cargo build      # no warnings
cargo test       # all green
cargo clippy     # clean
cargo fmt        # formatted
```

`cargo` writes to `~/.cargo` and `target/`; run it with normal (non-sandboxed)
filesystem access.

## Module layout

| Path | Responsibility |
| --- | --- |
| `src/model.rs` | Core domain types. |
| `src/config.rs` | `nv.yml` schema, load/save. |
| `src/color.rs` | ANSI color support and color configuration. |
| `src/discovery.rs` | Resolve services and files. |
| `src/parser/` | Formatting-preserving dotenv & YAML editors. |
| `src/search.rs` | Fuzzy search. |
| `src/secret.rs` | Secret generation. |
| `src/edit.rs` | Change sets, diffs, apply. |
| `src/cli/` | CLI commands + wizard. |
| `src/tui/` | Interactive UI. |
