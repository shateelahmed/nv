//! `nv.yml` configuration: schema, loading, saving, and the first-run wizard.
//!
//! The structs below describe the shape of `nv.yml`. The `Serialize` /
//! `Deserialize` derives (from the `serde` library) automatically convert
//! between these Rust structs and YAML text, so we never parse YAML by hand
//! here. `#[serde(...)]` attributes fine-tune that conversion.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize, Serializer};

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

/// Configuration for the `nv leaks` command.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LeakConfig {
    /// False alarms reported by `nv leaks`. Keys in this list are skipped
    /// on future runs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub false_alarms: Vec<String>,
}

/// An explicitly configured service entry.
///
/// The service name is the map key in `nv.yml`; the value holds optional
/// overrides for path, files, and per-service command configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ServiceConfig {
    /// Path relative to `services_root`; defaults to the service name (map key).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<ServiceFiles>,
    /// Per-service command-specific configuration (leaks, unused, etc.).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commands: Option<CommandsConfig>,
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

/// Serialize services map so empty `ServiceConfig` values become `null`
/// (YAML `key:`) instead of `{}`.
fn serialize_services<S: Serializer>(
    services: &BTreeMap<String, ServiceConfig>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeMap;
    let is_default = |svc: &ServiceConfig| -> bool {
        svc.path.is_none() && svc.files.is_none() && svc.commands.is_none()
    };
    let count = services.values().filter(|s| !is_default(s)).count();
    let mut map = serializer.serialize_map(Some(count))?;
    for (name, svc) in services {
        if is_default(svc) {
            map.serialize_entry(name, &())?;
        } else {
            map.serialize_entry(name, svc)?;
        }
    }
    map.end()
}

/// Configuration for the `nv unused` command.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UnusedConfig {
    /// Additional directories to skip when searching for key usage.
    /// These are merged with the built-in defaults (`.git`, `target`,
    /// `vendor`, `node_modules`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skip_dirs: Vec<String>,
    /// Files to skip when searching for key usage.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skip_files: Vec<String>,
}

/// Global command-specific configuration, nested under `commands:` in nv.yml.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CommandsConfig {
    /// Configuration for the `nv leaks` command.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leaks: Option<LeakConfig>,
    /// Configuration for the `nv unused` command.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unused: Option<UnusedConfig>,
}

/// The `nv.yml` document — the top-level shape of the whole config file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    /// Root directory containing service folders.
    pub services_root: String,
    /// Folder names to skip when auto-discovering services.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ignore: Vec<String>,
    /// Explicit service map; when empty, every subfolder is a service.
    #[serde(
        default,
        skip_serializing_if = "BTreeMap::is_empty",
        serialize_with = "serialize_services"
    )]
    pub services: BTreeMap<String, ServiceConfig>,
    /// Per-key secret generation presets. A `BTreeMap` keeps keys sorted so the
    /// written file has a stable, predictable order.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub secrets: BTreeMap<String, SecretPreset>,
    /// Global command-specific configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commands: Option<CommandsConfig>,
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
    /// Checks both global `commands.leaks.false_alarms` and per-service
    /// `commands.leaks.false_alarms`.
    pub fn is_false_alarm(&self, service: &str, key: &str) -> bool {
        // Check global false_alarms
        let global_match = self
            .commands
            .as_ref()
            .and_then(|cmd| cmd.leaks.as_ref())
            .map(|leaks| leaks.false_alarms.iter().any(|k| k == key))
            .unwrap_or(false);

        if global_match {
            return true;
        }

        // Check per-service false_alarms
        self.services
            .get(service)
            .and_then(|svc| svc.commands.as_ref())
            .and_then(|cmd| cmd.leaks.as_ref())
            .map(|leaks| leaks.false_alarms.iter().any(|k| k == key))
            .unwrap_or(false)
    }

    /// Mark a key in a specific service as a false alarm, creating entries as
    /// needed. Returns `true` if the key was newly added.
    pub fn add_false_alarm(&mut self, service: &str, key: &str) -> bool {
        let svc = self.services.entry(service.to_string()).or_default();

        let cmd = svc.commands.get_or_insert_with(CommandsConfig::default);
        let leaks = cmd.leaks.get_or_insert_with(LeakConfig::default);

        if leaks.false_alarms.iter().any(|k| k == key) {
            false
        } else {
            leaks.false_alarms.push(key.to_string());
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
    let raw = serde_yaml::to_string(config).context("serializing config")?;
    // serde_yaml writes `key: null` for empty map values. The cleaner YAML
    // form is `key:` (bare key with no value). Process line by line to avoid
    // touching unrelated nulls.
    let mut text: String = raw
        .lines()
        .map(|line| {
            if line.starts_with("  ") && line.ends_with(": null") {
                line[..line.len() - 4].trim_end().to_string()
            } else {
                line.to_string()
            }
        })
        .fold(String::new(), |mut acc, s| {
            if !acc.is_empty() {
                acc.push('\n');
            }
            acc.push_str(&s);
            acc
        });
    text.push('\n');
    std::fs::write(path, text).with_context(|| format!("writing config {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_false_alarm_from_commands() {
        let yaml = r#"
services_root: .
commands:
  leaks:
    false_alarms:
      - MY_OFFER_CHANNEL_ID
"#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert!(config.is_false_alarm("any-service", "MY_OFFER_CHANNEL_ID"));
        assert!(!config.is_false_alarm("any-service", "OTHER_KEY"));
    }

    #[test]
    fn global_and_per_service_false_alarms_merge() {
        let yaml = r#"
services_root: .
commands:
  leaks:
    false_alarms:
      - GLOBAL_KEY
services:
  auth:
    commands:
      leaks:
        false_alarms:
          - LOCAL_KEY
"#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert!(config.is_false_alarm("auth", "GLOBAL_KEY"));
        assert!(config.is_false_alarm("auth", "LOCAL_KEY"));
        assert!(!config.is_false_alarm("other", "LOCAL_KEY"));
    }

    #[test]
    fn empty_service_config_serializes_as_null() {
        let yaml = r#"
services_root: .
services:
  auth:
  billing:
    path: pay
"#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        let serialized = serde_yaml::to_string(&config).unwrap();
        // serde_yaml produces `auth: null`; save() post-processes to `auth:`
        assert!(
            serialized.contains("auth: null\n"),
            "serde_yaml should produce 'auth: null', got:\n{serialized}"
        );
        assert!(
            serialized.contains("billing:\n    path: pay"),
            "service with path should keep it"
        );
    }

    #[test]
    fn save_strips_null_for_empty_services() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nv.yml");
        let yaml = r#"
services_root: .
services:
  auth:
  billing:
    path: pay
"#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        save(&path, &config).unwrap();
        let written = std::fs::read_to_string(&path).unwrap();
        assert!(
            written.contains("  auth:\n"),
            "should write 'auth:' not 'auth: null', got:\n{written}"
        );
        assert!(
            !written.contains("auth: null"),
            "should not contain 'auth: null'"
        );
    }
}
