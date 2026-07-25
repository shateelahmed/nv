//! `nv.yml` configuration: schema, loading, saving, and the first-run wizard.
//!
//! The structs below describe the shape of `nv.yml`. The `Serialize` /
//! `Deserialize` derives (from the `serde` library) automatically convert
//! between these Rust structs and YAML text, so we never parse YAML by hand
//! here. `#[serde(...)]` attributes fine-tune that conversion.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::color::ColorConfig;

/// The file name `nv` looks for in the current directory.
pub const CONFIG_FILE: &str = "nv.yml";

/// Per-service file selection, keyed by [`FileKind`] label.
///
/// When a kind is absent, files of that kind are auto-discovered by pattern.
/// `Option<Vec<String>>` means "maybe a list": `None` = not configured,
/// `Some(list)` = use exactly this list. `skip_serializing_if` keeps `None`
/// fields out of the written YAML so the file stays tidy.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ServiceFiles {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dotenv: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dotenv_example: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub configmap: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret: Option<Vec<String>>,
}

/// An explicitly configured service entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceConfig {
    pub name: String,
    /// Path relative to `services_root`; defaults to `name`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<ServiceFiles>,
}

/// Secret generation format.
///
/// `#[serde(rename_all = "lowercase")]` means these variants are written in YAML
/// as `hex`, `base64`, `alnum`. `#[default]` marks the value used when the field
/// is omitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum SecretFormat {
    Hex,
    #[default]
    Base64,
    Alnum,
}

/// A per-key secret generation preset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretPreset {
    // `#[serde(default = "...")]` fills in this value when it is absent in YAML.
    #[serde(default = "default_length")]
    pub length: usize,
    #[serde(default)]
    pub format: SecretFormat,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub charset: Option<String>,
}

/// Default secret length (in characters or bytes) when unspecified.
fn default_length() -> usize {
    32
}

/// The `nv.yml` document — the top-level shape of the whole config file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    /// Root directory containing service folders.
    pub services_root: String,
    /// Folder names to skip when auto-discovering services.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ignore: Vec<String>,
    /// Explicit service list; when empty, every subfolder is a service.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub services: Vec<ServiceConfig>,
    /// Per-key secret generation presets. A `BTreeMap` keeps keys sorted so the
    /// written file has a stable, predictable order.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub secrets: BTreeMap<String, SecretPreset>,
    /// False alarms reported by `nv leaks`. Maps service name → list of key
    /// names to ignore on future runs.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub false_alarms: BTreeMap<String, Vec<String>>,
    /// Color configuration for CLI output.
    #[serde(default)]
    pub colors: ColorConfig,
}

impl Config {
    /// Absolute path to the configured services root, resolved against `base`
    /// (the directory containing `nv.yml`).
    pub fn services_root_abs(&self, base: &Path) -> PathBuf {
        let root = Path::new(&self.services_root);
        if root.is_absolute() {
            root.to_path_buf()
        } else {
            base.join(root)
        }
    }

    /// Look up the secret preset for a key, if any.
    pub fn secret_preset(&self, key: &str) -> Option<&SecretPreset> {
        self.secrets.get(key)
    }

    /// Check whether a key in a specific service is marked as a false alarm.
    pub fn is_false_alarm(&self, service: &str, key: &str) -> bool {
        self.false_alarms
            .get(service)
            .map(|keys| keys.iter().any(|k| k == key))
            .unwrap_or(false)
    }

    /// Mark a key in a specific service as a false alarm, creating entries as
    /// needed. Returns `true` if the key was newly added.
    pub fn add_false_alarm(&mut self, service: &str, key: &str) -> bool {
        let keys = self.false_alarms.entry(service.to_string()).or_default();
        if keys.iter().any(|k| k == key) {
            false
        } else {
            keys.push(key.to_string());
            true
        }
    }
}

/// Locate `nv.yml` starting from `start` and walking up to the filesystem root.
///
/// This lets you run `nv` from a subdirectory and still find the project's
/// config, similar to how `git` finds `.git`.
pub fn find_config(start: &Path) -> Option<PathBuf> {
    let mut dir = Some(start);
    // Walk upward one parent at a time until we find the file or run out.
    while let Some(d) = dir {
        let candidate = d.join(CONFIG_FILE);
        if candidate.is_file() {
            return Some(candidate);
        }
        dir = d.parent();
    }
    None
}

/// Load and parse a config file.
///
/// `with_context` attaches a human-friendly message if a step fails; the `?`
/// then returns that error to the caller.
pub fn load(path: &Path) -> Result<Config> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading config {}", path.display()))?;
    let config: Config = serde_yaml::from_str(&text)
        .with_context(|| format!("parsing config {}", path.display()))?;
    Ok(config)
}

/// Serialize and write a config file (used by `nv init` / the wizard).
pub fn save(path: &Path, config: &Config) -> Result<()> {
    let text = serde_yaml::to_string(config).context("serializing config")?;
    std::fs::write(path, text).with_context(|| format!("writing config {}", path.display()))?;
    Ok(())
}
