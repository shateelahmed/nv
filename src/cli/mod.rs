//! Command-line interface: argument parsing, context resolution, and handlers.
//!
//! We use the `clap` library with its "derive" style: the `Cli` struct and
//! `Command` enum below *describe* the command line, and `clap` generates the
//! parser and `--help` text from them automatically. Each `#[arg(...)]` /
//! `#[command(...)]` attribute configures one flag or subcommand.

mod compare;
pub mod context;
mod duplicates;
mod encrypt;
mod fake_secrets;
mod find;
mod generate;
mod leaks;
mod remove;
mod set;
mod unused;
pub mod wizard;

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::config::SecretFormat;

/// `nv` — configure environment variables across multiple microservices.
///
/// These are the "global" options: they work with any subcommand. `global =
/// true` lets them appear before or after the subcommand on the command line.
#[derive(Debug, Parser)]
#[command(name = "nv", version, about, long_about = None)]
pub struct Cli {
    /// Ignore nv.yml and drive everything from the command line.
    #[arg(long, global = true)]
    pub no_config: bool,

    /// Services root directory (used when there is no nv.yml or with
    /// --no-config). Defaults to the current directory.
    #[arg(long, global = true, value_name = "DIR")]
    pub root: Option<String>,

    /// Restrict to these services (repeatable). Empty means all services.
    #[arg(long = "service", short = 's', global = true, value_name = "NAME")]
    pub services: Vec<String>,

    /// Restrict to these file kinds: dotenv, dotenv_example, configmap, secret
    /// (repeatable). Empty means all files.
    #[arg(long = "file", short = 'f', global = true, value_name = "KIND")]
    pub files: Vec<String>,

    /// Target every service and file, ignoring --service/--file filters.
    #[arg(long, global = true)]
    pub all: bool,

    /// Show what would change without writing.
    #[arg(long, global = true)]
    pub dry_run: bool,

    /// Apply changes without an interactive confirmation prompt.
    #[arg(long, short = 'y', global = true)]
    pub yes: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

/// The subcommands `nv` supports. Each variant becomes `nv <name>` on the CLI,
/// and its fields become that subcommand's arguments.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Create or update nv.yml via an interactive wizard.
    Init,

    /// Fuzzy-find a key across all services.
    Find {
        /// Search query (matches service, key, and file name).
        #[arg(default_value = "")]
        query: String,
    },

    /// Set a key to a value across the selected services and files.
    Set {
        /// The environment variable key.
        key: String,
        /// The value to set.
        value: String,
    },

    /// Generate a random secret and set it on a key.
    Gen {
        /// The environment variable key.
        key: String,
        /// Number of characters (alnum/charset) or random bytes (hex/base64).
        #[arg(long)]
        length: Option<usize>,
        /// Output format.
        #[arg(long, value_enum)]
        format: Option<FormatArg>,
        /// Custom character set (overrides --format).
        #[arg(long)]
        charset: Option<String>,
        /// Generate a distinct secret for each target instead of sharing one.
        #[arg(long)]
        unique: bool,
    },

    /// List keys that look like secrets in example and configmap files.
    Leaks {
        /// Remove detected keys from configmaps and set empty values in
        /// example files. Shows a preview before applying.
        #[arg(long)]
        clean: bool,
        /// Mark a detected key as a false alarm (saved to nv.yml so it is
        /// skipped on future runs).
        #[arg(long = "false-alarm", value_name = "KEY")]
        false_alarm: Option<String>,
    },

    /// List keys with placeholder values or misfiled in secrets files.
    FakeSecrets {
        /// Mark a detected key as a false alarm (saved to nv.yml so it is
        /// skipped on future runs).
        #[arg(long = "false-alarm", value_name = "KEY")]
        false_alarm: Option<String>,
    },

    /// Remove environment variable keys from files.
    Remove {
        /// The key(s) to remove (repeatable).
        #[arg(required = true)]
        keys: Vec<String>,
        /// Target env files (.env*).
        #[arg(short = 'e')]
        env: bool,
        /// Target configmap files.
        #[arg(short = 'c')]
        configmap: bool,
        /// Target secrets files.
        #[arg(short = 'x')]
        secrets: bool,
        /// Target all services and file types.
        #[arg(short = 'a')]
        all: bool,
    },

    /// Encrypt all values in selected .env.example files.
    Encrypt {
        /// Encryption key (required).
        #[arg(long)]
        key: String,
        /// The service name (required).
        #[arg(short = 'S')]
        service: String,
        /// The file path relative to the service directory (required).
        #[arg(short = 'F')]
        file: String,
    },

    /// Decrypt all values in selected .env.example files.
    Decrypt {
        /// Decryption key (required).
        #[arg(long)]
        key: String,
        /// The service name (required).
        #[arg(short = 'S')]
        service: String,
        /// The file path relative to the service directory (required).
        #[arg(short = 'F')]
        file: String,
    },

    /// List env keys not referenced in the codebase.
    Unused {
        /// The service name(s) to scan (repeatable).
        #[arg(short = 's', long = "service")]
        services: Vec<String>,
        /// Remove unused keys (with preview).
        #[arg(long)]
        clean: bool,
    },

    /// List env keys that appear multiple times.
    Duplicates {
        /// The service name(s) to scan (repeatable).
        #[arg(short = 's', long = "service")]
        services: Vec<String>,
    },

    /// Compare an env file against other files of the same kind.
    Compare {
        /// Path to the base file, relative to the services root.
        file_path: String,
        /// Also compare values for keys present in both files.
        #[arg(long)]
        values: bool,
    },
}

/// CLI mirror of [`SecretFormat`].
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum FormatArg {
    Hex,
    Base64,
    Alnum,
}

impl From<FormatArg> for SecretFormat {
    fn from(f: FormatArg) -> Self {
        match f {
            FormatArg::Hex => SecretFormat::Hex,
            FormatArg::Base64 => SecretFormat::Base64,
            FormatArg::Alnum => SecretFormat::Alnum,
        }
    }
}

/// Parse arguments and dispatch to the appropriate handler.
///
/// `Cli::parse()` reads the process arguments (and exits with help/errors if
/// they're invalid). We then route to the matching command function. When no
/// subcommand is given (`None`), we launch the interactive TUI.
pub fn run() -> Result<()> {
    let cli = Cli::parse();

    match &cli.command {
        Some(Command::Init) => wizard::run_init(&cli),
        Some(Command::Find { query }) => find::run(&cli, query),
        Some(Command::Set { key, value }) => set::run(&cli, key, value),
        Some(Command::Gen {
            key,
            length,
            format,
            charset,
            unique,
        }) => generate::run(&cli, key, *length, *format, charset.clone(), *unique),
        Some(Command::Leaks { clean, false_alarm }) => leaks::run(&cli, *clean, false_alarm),
        Some(Command::FakeSecrets { false_alarm }) => fake_secrets::run(&cli, false_alarm),
        Some(Command::Remove {
            keys,
            env,
            configmap,
            secrets,
            all,
        }) => remove::run(&cli, keys, *env, *configmap, *secrets, *all),
        Some(Command::Encrypt { key, service, file }) => {
            encrypt::run_encrypt(&cli, key, service, file)
        }
        Some(Command::Decrypt { key, service, file }) => {
            encrypt::run_decrypt(&cli, key, service, file)
        }
        Some(Command::Unused { services, clean }) => unused::run(&cli, services, *clean),
        Some(Command::Duplicates { services }) => duplicates::run(&cli, services),
        Some(Command::Compare { file_path, values }) => compare::run(&cli, file_path, *values),
        None => crate::tui::launch(&cli),
    }
}
