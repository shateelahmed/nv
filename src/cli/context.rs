//! Resolve the effective run context (services, config source) from CLI flags.
//!
//! Every command needs the same setup: figure out where config comes from
//! (`nv.yml` or the command line), then load the list of services. This module
//! does that once and hands back a [`Context`] the command can use.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{Result, bail};
use glob::Pattern;
use indicatif::{ProgressBar, ProgressStyle};

use super::Cli;
use crate::color::ColorConfig;
use crate::config::{self, CONFIG_FILE, Config};
use crate::discovery;
use crate::model::{ConfigSource, EnvFile, FileKind, Service};

/// The resolved context for a command invocation.
pub struct Context {
    /// Whether settings came from `nv.yml` or the command line.
    pub source: ConfigSource,
    /// The loaded config, if any (`None` in command-line mode).
    pub config: Option<Config>,
    /// All discovered services and their files.
    pub services: Vec<Service>,
    /// Directory paths are resolved against (config dir or working dir).
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

    /// Return the color configuration (cheap copy of 7 small enums).
    pub fn colors(&self) -> ColorConfig {
        self.config.as_ref().map(|c| c.colors).unwrap_or_default()
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

/// Print the standard config-source banner, including the full path to the
/// `nv.yml` that was loaded (command-line mode has no path).
pub fn print_banner(ctx: &Context) {
    match ctx.source {
        ConfigSource::NvYml => {
            let path = ctx.base.join(CONFIG_FILE);
            eprintln!("Config source: {}", path.display());
        }
        ConfigSource::CommandLine => eprintln!("Config source: command-line"),
    }
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
    if !cli.yes && !confirm("Apply these changes?", false)? {
        eprintln!("Aborted.");
        return Ok(());
    }

    let written = changes.apply()?;
    eprintln!("Wrote {written} file(s).");
    Ok(())
}

/// Prompt a yes/no confirmation that echoes the user's typed answer with the
/// terminal cursor visible at the end of the input.
///
/// dialoguer's `Confirm` hides the cursor and accepts a bare `y`/`n` keypress,
/// so the user never sees what they typed. This helper instead uses a text
/// input (which keeps the cursor visible) and validates it as
/// `y`/`yes`/`n`/`no` (case-insensitive), re-prompting on invalid answers. An
/// empty answer (Enter) falls back to `default`.
pub fn confirm(prompt: &str, default: bool) -> Result<bool> {
    let default = if default { "yes" } else { "no" };
    let answer: String = dialoguer::Input::new()
        .with_prompt(format!("{prompt} (y/n)"))
        .default(default.to_string())
        .validate_with(|input: &String| -> Result<(), String> {
            match input.trim().to_ascii_lowercase().as_str() {
                "y" | "yes" | "n" | "no" => Ok(()),
                _ => Err("please answer y/n or yes/no".to_string()),
            }
        })
        .interact_text()?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
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

/// Mark one or more keys as false alarms in nv.yml.
///
/// `pairs` is a slice of `(service_name, key_name)` tuples to mark.
pub fn mark_false_alarm(pairs: &[(&str, &str)]) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let config_path = config::find_config(&cwd).unwrap_or_else(|| cwd.join(CONFIG_FILE));

    let mut cfg = if config_path.exists() {
        config::load(&config_path)?
    } else {
        bail!("no nv.yml found; create one first with `nv init`");
    };

    let mut added = 0usize;
    for (service, key) in pairs {
        if cfg.add_false_alarm(service, key) {
            added += 1;
        }
    }

    config::save(&config_path, &cfg)?;
    eprintln!(
        "Marked {added} key(s) as false alarm(s) in {}.",
        config_path.display()
    );

    Ok(())
}

/// Check if a file path matches any skip_files pattern.
///
/// Supports glob patterns: `*` matches any characters except `/`, `**` matches
/// any characters including `/` (recursive). Matches are tried against both the
/// relative path from the service root and the bare file name.
pub fn matches_skip_pattern(relative_str: &str, name: &str, skip_files: &HashSet<String>) -> bool {
    for pattern in skip_files {
        if let Ok(glob_pattern) = Pattern::new(pattern)
            && (glob_pattern.matches(relative_str) || glob_pattern.matches(name))
        {
            return true;
        }
    }
    false
}

/// Whether an env file matches a service's merged skip_files set.
pub fn file_is_skipped(file: &EnvFile, service_root: &Path, skip_files: &HashSet<String>) -> bool {
    let relative = file.path.strip_prefix(service_root).unwrap_or(&file.path);
    let relative_str = relative.to_str().unwrap_or("");
    let name = file.path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    matches_skip_pattern(relative_str, name, skip_files)
}

/// Build the merged set of files to skip for a service from an accessor that
/// returns a command's global + per-service `skip_files` list.
pub fn build_skip_files<'a>(
    config: &'a Option<Config>,
    service: &'a str,
    accessor: impl Fn(&'a Config, &'a str) -> Vec<&'a str>,
) -> HashSet<String> {
    let mut skip: HashSet<String> = HashSet::new();
    if let Some(cfg) = config {
        for file in accessor(cfg, service) {
            skip.insert(file.to_string());
        }
    }
    skip
}

/// A shared progress tracker for commands that scan files.
///
/// Displays a spinner during scanning and prints a summary line when finished.
pub struct ScanProgress {
    pb: ProgressBar,
    files_scanned: AtomicUsize,
    dirs_scanned: AtomicUsize,
    total_keys: Option<usize>,
}

impl ScanProgress {
    /// Create a new spinner with the standard green tick style.
    pub fn start() -> Self {
        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::with_template("{spinner:.green} {msg}")
                .unwrap()
                .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"),
        );
        pb.enable_steady_tick(std::time::Duration::from_millis(80));

        Self {
            pb,
            files_scanned: AtomicUsize::new(0),
            dirs_scanned: AtomicUsize::new(0),
            total_keys: None,
        }
    }

    /// Create a new spinner with a key count for the summary line.
    pub fn start_with_keys(total_keys: usize) -> Self {
        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::with_template("{spinner:.green} {msg}")
                .unwrap()
                .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"),
        );
        pb.enable_steady_tick(std::time::Duration::from_millis(80));

        Self {
            pb,
            files_scanned: AtomicUsize::new(0),
            dirs_scanned: AtomicUsize::new(0),
            total_keys: Some(total_keys),
        }
    }

    /// Update the spinner message (e.g., current file being scanned).
    pub fn set_message(&self, msg: impl Into<String>) {
        self.pb.set_message(msg.into());
    }

    /// Increment the files-scanned counter.
    pub fn inc_files(&self) {
        self.files_scanned.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment the directories-scanned counter.
    pub fn inc_dirs(&self) {
        self.dirs_scanned.fetch_add(1, Ordering::Relaxed);
    }

    /// Finish the spinner and print the summary line.
    pub fn finish(&self) {
        self.pb.finish_and_clear();
        let files = self.files_scanned.load(Ordering::Relaxed);
        let dirs = self.dirs_scanned.load(Ordering::Relaxed);
        let file_noun = if files == 1 { "file" } else { "files" };
        let folder_noun = if dirs == 1 { "folder" } else { "folders" };

        match self.total_keys {
            Some(keys) => {
                let key_noun = if keys == 1 { "key" } else { "keys" };
                eprintln!(
                    "Scanned {} {} in {} {} for {} {}.",
                    files, file_noun, dirs, folder_noun, keys, key_noun
                );
            }
            None => {
                eprintln!(
                    "Scanned {} {} in {} {}.",
                    files, file_noun, dirs, folder_noun
                );
            }
        }
    }
}
