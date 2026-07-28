//! Core domain types shared across `nv`.
//!
//! This module holds the plain data structures the rest of the program passes
//! around: what a service is, what an env file is, and so on. Keeping them in
//! one place means every other module speaks the same "language".

use std::fmt;
use std::path::PathBuf;

use anyhow::Result;

/// The four kinds of env-bearing files `nv` understands.
///
/// An `enum` is a type that is exactly one of a fixed set of variants. The
/// `#[derive(...)]` line auto-generates common behavior so we don't write it by
/// hand: `Debug` (printable for debugging), `Clone`/`Copy` (cheap to duplicate),
/// `PartialEq`/`Eq` (comparable with `==`), and `Hash` (usable as a map key).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileKind {
    /// `.env`
    Dotenv,
    /// `.env.example`
    DotenvExample,
    /// `configmap*.yml` / `configmap*.yaml`
    ConfigMap,
    /// `secrets*.yml` / `secrets*.yaml`
    Secret,
}

impl FileKind {
    /// Human-readable label used in the config file and UI.
    pub fn label(self) -> &'static str {
        // `match` must cover every variant, so the compiler guarantees we never
        // forget one if a new file kind is added later.
        match self {
            FileKind::Dotenv => "dotenv",
            FileKind::DotenvExample => "dotenv_example",
            FileKind::ConfigMap => "configmap",
            FileKind::Secret => "secret",
        }
    }

    /// Whether this file kind is a template/example that should never receive
    /// real generated secret values (they are written empty instead).
    pub fn is_example(self) -> bool {
        // `matches!` is shorthand for "does this value match that pattern?".
        matches!(self, FileKind::DotenvExample)
    }

    /// Whether this file kind is parsed as YAML.
    pub fn is_yaml(self) -> bool {
        matches!(self, FileKind::ConfigMap | FileKind::Secret)
    }
}

// Implementing `Display` lets us use a `FileKind` directly in `{}` formatting,
// e.g. `println!("{kind}")` prints the label.
impl fmt::Display for FileKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// A single env file belonging to a service.
///
/// A `struct` groups related fields together. `PathBuf` is an owned filesystem
/// path (like an owned `String`, but for paths).
#[derive(Debug, Clone)]
pub struct EnvFile {
    pub kind: FileKind,
    /// Absolute path on disk.
    pub path: PathBuf,
    /// Path relative to the service directory, for display.
    pub display: String,
}

/// A discovered microservice and its env files.
#[derive(Debug, Clone)]
pub struct Service {
    pub name: String,
    /// Absolute path to the service directory.
    // `#[allow(dead_code)]` silences the "unused field" warning: we keep this
    // as useful metadata even though nothing reads it yet.
    #[allow(dead_code)]
    pub path: PathBuf,
    /// Every env file that belongs to this service (a growable list).
    pub files: Vec<EnvFile>,
}

/// A key/value pair discovered inside a specific file of a specific service,
/// used to build the fuzzy-search index and display results.
#[derive(Debug, Clone)]
pub struct EnvKey {
    pub service: String,
    pub file_display: String,
    /// Kind and path of the source file, retained for callers that act on a
    /// specific search hit.
    #[allow(dead_code)]
    pub file_kind: FileKind,
    #[allow(dead_code)]
    pub file_path: PathBuf,
    pub key: String,
    pub value: String,
}

/// Where the effective configuration for a command came from.
///
/// Used to print the "Config source: ..." banner so the user always knows
/// whether settings came from `nv.yml` or straight from the command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigSource {
    /// Loaded from an `nv.yml` file.
    NvYml,
    /// Derived purely from command-line flags (`--no-config` or no config present).
    CommandLine,
}

impl ConfigSource {
    pub fn banner(self) -> &'static str {
        match self {
            ConfigSource::NvYml => "Config source: nv.yml",
            ConfigSource::CommandLine => "Config source: command-line",
        }
    }
}

/// Find an [`EnvFile`] by service name and display path.
///
/// This is used by multiple commands (encrypt, decrypt, unused) to resolve
/// a `(service, file)` CLI pair into the concrete file on disk.
pub fn find_target<'a>(
    services: &'a [Service],
    service_name: &str,
    file_path: &str,
) -> Result<&'a EnvFile> {
    let service = services
        .iter()
        .find(|s| s.name == service_name)
        .ok_or_else(|| anyhow::anyhow!("service '{service_name}' not found"))?;

    service
        .files
        .iter()
        .find(|f| f.display == file_path)
        .ok_or_else(|| anyhow::anyhow!("file '{file_path}' not found in service '{service_name}'"))
}

impl fmt::Display for ConfigSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ConfigSource::NvYml => "nv.yml",
            ConfigSource::CommandLine => "command-line",
        })
    }
}
