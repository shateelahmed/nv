# nv

> **nv** (pronounced *envy*) — configure environment variables across multiple microservices from your terminal.

`nv` finds, sets, and generates environment variables spread across many services
and file formats — `.env*` files, `configmap*.yml`, and `secrets*.yml` —
while **preserving comments and formatting**. It offers both a scriptable CLI and
an interactive TUI.

## New here? A few terms

- **Environment variable** — a named setting your app reads at runtime, like
  `DATABASE_URL` or `JWT_SECRET`.
- **Microservice** — in `nv`, just a folder that holds one app's config files.
  Point `nv` at a parent directory and each subfolder is treated as a service.
- **Env files** — the files `nv` edits: `.env*` (simple `KEY=value` lines,
  including `.env`, `.env.local`, `.env.example`, `.env.testing.example`, etc.)
  and `configmap*.yml` / `secrets*.yml` (Kubernetes-style YAML). You don't need
  to know Kubernetes to use them.
- **CLI vs TUI** — the **CLI** is one-line commands you type (great for scripts);
  the **TUI** is the interactive full-screen menu you get by running `nv` with no
  arguments.

## Features

- **Fuzzy find** any key across every service.
- **Set a value** in one, several, or all services — and in any of their env
  files — in a single command (great for shared values).
- **Generate secrets** (hex / base64 / alphanumeric / custom charset) and write
  them to the keys of your choice, with an option for a distinct secret per target.
- **Formatting-preserving edits**: comments, ordering, and untouched lines stay
  byte-identical. New keys are auto-created (YAML keys land under `data:` /
  `stringData:`).
- **Example files stay safe**: generated secrets are written *empty* into any
  file containing `.example` in its name (e.g., `.env.example`,
  `.env.testing.example`).
- **Preview & confirm**: see a diff before anything is written; `--dry-run` and
  `--yes` for automation.
- **Transparent config source**: every command tells you whether it read from
  `nv.yml` or the command line.
- **Colorized output**: hierarchical, color-coded output with configurable colors
  for services, folders, files, keys, and values. Respects `NO_COLOR` standard.

## Getting started (no Rust experience needed)

`nv` is a small program written in Rust. You don't need to know Rust to use it —
you just need to build it once, then run it. Here's the whole process.

### 1. Install Rust

Rust comes with a build tool called **cargo**, which you'll use to compile `nv`.
If you don't have it yet, install it from [rustup.rs](https://rustup.rs):

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Follow the prompts (the defaults are fine), then **restart your terminal** so the
`cargo` command becomes available. Check it worked:

```sh
cargo --version
```

If that prints a version number, you're ready.

### 2. Build nv

From inside this project folder (the one containing `Cargo.toml`):

```sh
cargo build --release
```

The first build downloads dependencies and takes a minute or two. When it
finishes, the program exists at `./target/release/nv`.

### 3. Run nv

You have three options, from easiest to most convenient:

**Option A — run the built file directly** (works immediately, no setup):

```sh
./target/release/nv --help
```

**Option B — let cargo run it for you** (handy while developing):

```sh
cargo run --release -- --help
```

Everything after `--` is passed to `nv`, e.g. `cargo run --release -- find db`.

**Option C — install it so you can just type `nv` anywhere** (recommended):

```sh
cargo install --path .
```

This copies `nv` into `~/.cargo/bin`. That folder must be on your `PATH` for the
bare `nv` command to work.

- If you installed Rust with **rustup**, it's already set up — just open a new
  terminal.
- If you installed Rust another way (e.g. **Homebrew**, where `cargo` lives in
  `/opt/homebrew/bin`), `~/.cargo/bin` is usually *not* on your `PATH`. Add it
  once with the line for your shell, then restart the terminal:

  ```sh
  # zsh (default on macOS)
  echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> ~/.zshrc

  # bash
  echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> ~/.bashrc
  ```

Then, from any directory:

```sh
nv --help
```

> **"command not found: nv"?**
> That message means your shell can't find an `nv` on your `PATH` yet. It does
> **not** mean anything is broken. Either run the binary by its path
> (`./target/release/nv`), use `cargo run --release -- ...`, or make sure
> `~/.cargo/bin` is on your `PATH` using the `export PATH=...` line above (this
> is the common case when Rust was installed via Homebrew rather than rustup).
> Verify with `echo $PATH | tr ':' '\n' | grep cargo` — if it prints nothing,
> the folder isn't on your `PATH` yet.

## Quick start

Once you can run `nv` (see above), try it out. If you haven't installed it, just
replace `nv` with `./target/release/nv` in these examples.

```sh
# Launch the interactive, full-screen UI.
# On the first run it asks where your microservices live and offers to save an
# nv.yml config for you.
nv

# Prefer to set up the config separately? Run the wizard on its own:
nv init

# See every available command and option at any time:
nv --help
```


## CLI

```
nv [GLOBAL OPTIONS] [COMMAND]
```

> In the examples below, `nv` assumes you installed it (Option C above). If you
> didn't, use `./target/release/nv` or `cargo run --release --` instead.

### Commands

| Command | Description |
| --- | --- |
| `nv` | Launch the interactive TUI. |
| `nv init` | Create/update `nv.yml` via a wizard. |
| `nv find <query>` | Fuzzy-find a key across all services. |
| `nv set <KEY> <VALUE>` | Set a key's value across selected services/files. |
| `nv gen <KEY>` | Generate a secret and set it on a key. |

### Global options

| Option | Description |
| --- | --- |
| `-s, --service <NAME>` | Restrict to a service (repeatable). Empty = all. |
| `-f, --file <KIND>` | Restrict to a file kind: `dotenv`, `dotenv_example`, `configmap`, `secret` (repeatable). Empty = all. |
| `--all` | Target every service and file, ignoring filters. |
| `--root <DIR>` | Services root when there is no `nv.yml` or with `--no-config`. |
| `--no-config` | Ignore `nv.yml` and drive everything from the command line. |
| `--dry-run` | Show the diff without writing. |
| `-y, --yes` | Apply without an interactive confirmation. |

### `nv gen` options

| Option | Description |
| --- | --- |
| `--length <N>` | Characters (alnum/charset) or random bytes (hex/base64). |
| `--format <FMT>` | `hex`, `base64`, or `alnum`. |
| `--charset <CHARS>` | Custom character set (overrides `--format`). |
| `--unique` | Generate a distinct secret per target instead of one shared value. |

### Examples

```sh
# Find every key that fuzzy-matches "db"
nv find db

# Set the same DB URL in every service's .env and configmap
nv set DATABASE_URL postgres://db:5432/app

# Set a value only in the auth service's .env
nv set LOG_LEVEL debug --service auth --file dotenv

# Preview a change without writing
nv --dry-run set FEATURE_FLAG on

# Generate one shared 48-byte base64 JWT secret across all services
nv gen JWT_SECRET --length 48 --format base64 --yes

# Generate a distinct 32-char alphanumeric secret per service
nv gen SESSION_KEY --length 32 --format alnum --unique --yes
```

The `nv find` command displays results in a hierarchical, colorized format:

```
Color legend:
  magenta microservice root
  blue subfolder
  cyan file
  green key name
  yellow value

auth/
  src/
    .env
      DATABASE_URL = postgres://db:5432/app
      LOG_LEVEL = debug
  docker/
    .env
      API_KEY = sk-...
  .env.example
      DATABASE_URL =
      LOG_LEVEL =
```

## Configuration (`nv.yml`)

`nv` looks for `nv.yml` in the current directory (walking upward). Every subfolder
of `services_root` is treated as a microservice unless you list services or
ignores explicitly.

```yaml
# Root directory containing service folders.
services_root: ./services

# Folders to skip during auto-discovery.
ignore:
  - node_modules
  - .git

# Optional explicit service list. Omit to auto-discover every subfolder.
services:
  - name: auth
    # path defaults to `name` when omitted
    path: auth
    # Omit `files` to auto-discover by filename pattern.
    files:
      dotenv: [.env]
      dotenv_example: [.env.example]
      configmap: [configmap.yml]
      secret: [secrets.yml]

# Per-key secret generation presets used by `nv gen`.
secrets:
  JWT_SECRET:
    length: 48
    format: base64      # hex | base64 | alnum
  SESSION_KEY:
    length: 32
    format: alnum

# Color configuration for CLI output (nv find).
colors:
  service_root: magenta  # microservice root folder names
  subfolder: blue        # subfolder names within a service
  file: cyan             # file names
  key: green             # env variable key names
  value: yellow          # env variable values
```

### Color configuration

The `colors` section in `nv.yml` customizes the terminal output colors for
`nv find`. Available colors: `black`, `red`, `green`, `yellow`, `blue`,
`magenta`, `cyan`, `white`.

Colors are automatically disabled when:
- The `NO_COLOR` environment variable is set (see [no-color.org](https://no-color.org/))
- Output is not a terminal (e.g., piped to a file)

Use `--no-config` on any command to ignore `nv.yml` and rely purely on
`--root` / `--service` / `--file`. The active source is always printed as
`Config source: nv.yml` or `Config source: command-line`.

## TUI flow

Fuzzy-find a key → select services → select file kinds → choose *set* or
*generate* → review the diff → confirm & apply.

| Key | Action |
| --- | --- |
| type / `Backspace` | Filter (find screen) or edit value |
| `Up` / `Down` | Move selection / scroll the preview |
| `Space` | Toggle a service or file kind |
| `a` | Toggle all |
| `Enter` | Advance / apply |
| `y` / `n` | Confirm / cancel on the preview screen |
| `Esc` | Step back, or quit from the find screen |

## Development

```sh
cargo build            # compile
cargo test             # run the unit test suite
cargo clippy           # lint
cargo fmt              # format
```

### Project layout

| Path | Responsibility |
| --- | --- |
| `src/model.rs` | Core domain types. |
| `src/config.rs` | `nv.yml` schema, load/save. |
| `src/color.rs` | ANSI color support and color configuration. |
| `src/discovery.rs` | Resolve services and their files. |
| `src/parser/` | Formatting-preserving dotenv & YAML editors. |
| `src/search.rs` | Fuzzy search over env keys. |
| `src/secret.rs` | Configurable secret generation. |
| `src/edit.rs` | Change sets, diffs, and safe application. |
| `src/cli/` | CLI commands and the first-run wizard. |
| `src/tui/` | Interactive terminal UI. |

## Spec-driven development

This repo uses a lightweight spec-driven workflow. Feature work flows through
four stages — **specify → plan → tasks → implement** — with artifacts stored under
[`specs/`](specs/). See [`specs/README.md`](specs/README.md) for the full process.

Both assistants share the same templates and conventions:

- **GitHub Copilot**: repo instructions in
  [`.github/copilot-instructions.md`](.github/copilot-instructions.md) and slash-command
  prompts in [`.github/prompts/`](.github/prompts/) (`/specify`, `/plan`, `/tasks`,
  `/implement`).
- **Claude Code CLI**: project context in [`CLAUDE.md`](CLAUDE.md) and slash commands
  in [`.claude/commands/`](.claude/commands/) (`/specify`, `/plan`, `/tasks`,
  `/implement`).

## License

Licensed under the [MIT License](LICENSE).
