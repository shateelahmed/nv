//! Resolve the effective run context (services, config source) from CLI flags.
//!
//! Every command needs the same setup: figure out where config comes from
//! (`nv.yml` or the command line), then load the list of services. This module
//! does that once and hands back a [`Context`] the command can use.

use std::path::PathBuf;

use anyhow::{Result, bail};

use super::Cli;
use crate::color::ColorConfig;
use crate::config::{self, Config};
use crate::discovery;
use crate::model::{ConfigSource, FileKind, Service};

/// The resolved context for a command invocation.
pub struct Context {
    /// Whether settings came from `nv.yml` or the command line.
    pub source: ConfigSource,
    /// The loaded config, if any (`None` in command-line mode).
    pub config: Option<Config>,
    /// All discovered services and their files.
    pub services: Vec<Service>,
    /// Directory paths are resolved against (config dir or working dir).
    #[allow(dead_code)]
    pub base: PathBuf,
}

impl Context {
    /// Effective service-name filter (empty when `--all`).
    pub fn service_filter(&self, cli: &Cli) -> Vec<String> {
        if cli.all {
            Vec::new()
        } else {
            cli.services.clone()
        }
    }

    /// Effective file-kind filter (empty when `--all` or none specified).
    pub fn kind_filter(&self, cli: &Cli) -> Result<Vec<FileKind>> {
        if cli.all {
            return Ok(Vec::new());
        }
        cli.files.iter().map(|f| parse_kind(f)).collect()
    }
}

/// Resolve services and config source honoring `--no-config` and `--root`.
///
/// Priority: `--no-config` forces command-line mode; otherwise we look for an
/// `nv.yml`; if there isn't one we fall back to scanning `--root` (or the
/// current directory).
pub fn resolve(cli: &Cli) -> Result<Context> {
    let cwd = std::env::current_dir()?;

    if cli.no_config {
        // Explicitly ignore any nv.yml and scan the given (or current) folder.
        let root = cli
            .root
            .clone()
            .map(PathBuf::from)
            .unwrap_or_else(|| cwd.clone());
        let services = discovery::discover_scanned(&root, &[])?;
        return Ok(Context {
            source: ConfigSource::CommandLine,
            config: None,
            services,
            base: cwd,
        });
    }

    match config::find_config(&cwd) {
        // Found an nv.yml: load it and discover services relative to it.
        Some(path) => {
            let config = config::load(&path)?;
            let base = path.parent().unwrap_or(&cwd).to_path_buf();
            let services = discovery::discover(&config, &base)?;
            Ok(Context {
                source: ConfigSource::NvYml,
                config: Some(config),
                services,
                base,
            })
        }
        // No nv.yml anywhere: behave like command-line mode.
        None => {
            let root = cli
                .root
                .clone()
                .map(PathBuf::from)
                .unwrap_or_else(|| cwd.clone());
            let services = discovery::discover_scanned(&root, &[])?;
            Ok(Context {
                source: ConfigSource::CommandLine,
                config: None,
                services,
                base: cwd,
            })
        }
    }
}

/// Print the standard config-source banner.
pub fn print_banner(source: ConfigSource) {
    eprintln!("{}", source.banner());
}

/// Preview a change set and, unless `--dry-run`, apply it (prompting for
/// confirmation unless `--yes`).
///
/// This is the shared "safe write" path used by `set`, `gen`, `remove`, and
/// `leaks --clean`: show the hierarchical colorized diff, stop for `--dry-run`,
/// ask for confirmation, then write.
pub fn preview_and_apply(
    cli: &Cli,
    changes: &crate::edit::ChangeSet,
    colors: &ColorConfig,
    use_color: bool,
) -> Result<()> {
    if changes.is_empty() {
        eprintln!("Nothing to change.");
        return Ok(());
    }

    // Show the hierarchical colorized diff so the user sees exactly what will change.
    println!("{}", changes.render_diff(colors, use_color));

    if cli.dry_run {
        eprintln!("Dry run: no files written.");
        return Ok(());
    }

    // Unless `--yes` was passed, require an explicit confirmation.
    if !cli.yes {
        let confirmed = dialoguer::Confirm::new()
            .with_prompt("Apply these changes?")
            .default(false)
            .interact()?;
        if !confirmed {
            eprintln!("Aborted.");
            return Ok(());
        }
    }

    let written = changes.apply()?;
    eprintln!("Wrote {written} file(s).");
    Ok(())
}

/// Parse a file-kind label into a [`FileKind`].
pub fn parse_kind(label: &str) -> Result<FileKind> {
    match label.to_ascii_lowercase().as_str() {
        "dotenv" | "env" | ".env" => Ok(FileKind::Dotenv),
        "dotenv_example" | "example" | ".env.example" => Ok(FileKind::DotenvExample),
        "configmap" | "config" => Ok(FileKind::ConfigMap),
        "secret" | "secrets" => Ok(FileKind::Secret),
        other => bail!(
            "unknown file kind '{other}' (expected: dotenv, dotenv_example, configmap, secret)"
        ),
    }
}
