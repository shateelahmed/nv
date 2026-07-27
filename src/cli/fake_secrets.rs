//! `nv fake-secrets` — list keys with placeholder values or misfiled in
//! secrets files.

use std::collections::BTreeMap;
use std::fs;

use anyhow::{Result, bail};
use regex::Regex;

use super::{Cli, context};
use crate::color::{self, ColorConfig, colorize};
use crate::config::{self, CONFIG_FILE};
use crate::model::FileKind;
use crate::parser;

/// Regex matching keys in dotenv/YAML files.
///
/// Captures:
/// - Group 1: key name
/// - Group 2: YAML value (after `:`)
/// - Group 3: dotenv value (after `=`)
fn key_value_pattern() -> Regex {
    Regex::new(
        r"(?m)^\s*(?:export\s+)?([A-Za-z0-9_][A-Za-z0-9_]+)[^\S\n]*(?::\s*(.+)|=[^\S\n]*(.*))$",
    )
    .expect("hardcoded regex is valid")
}

/// Regex matching the secret key naming pattern.
///
/// Matches keys like `DB_PASSWORD`, `API_KEY`, `JWT_SECRET`, etc.
fn secret_key_pattern() -> Regex {
    Regex::new(r"(?m)^[A-Za-z0-9_][A-Z0-9_]+_(?:KEY|PASSWORD|SECRET|TOKEN|ID|USERNAME)$")
        .expect("hardcoded regex is valid")
}

/// Check if a value looks like a placeholder.
///
/// Returns `true` for values like `xxx`, `changeme`, `placeholder`, etc.
/// The check is case-insensitive.
fn is_placeholder(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return true;
    }
    let lower = trimmed.to_lowercase();

    // Three or more x characters.
    if trimmed.len() >= 3 && trimmed.chars().all(|c| c == 'x' || c == 'X') {
        return true;
    }

    // Common placeholder patterns.
    matches!(
        lower.as_str(),
        "changeme"
            | "change_me"
            | "change-me"
            | "placeholder"
            | "dummy"
            | "test"
            | "example"
            | "fake"
            | "secret"
    )
}

/// A single detected fake secret.
struct FakeSecret {
    service: String,
    file_display: String,
    key: String,
    value: String,
}

/// Grouped display structure: service -> file -> list of key/value pairs.
type FakeSecretMap<'a> = BTreeMap<&'a str, BTreeMap<&'a str, Vec<(&'a str, &'a str)>>>;

/// Handle `nv fake-secrets`: scan configmap and secrets files for placeholder
/// values and misfiled keys.
///
/// Flags:
/// - `--false-alarm <KEY>`: mark a key as a false alarm (saved to nv.yml).
pub fn run(cli: &Cli, false_alarm: &Option<String>) -> Result<()> {
    let ctx = context::resolve(cli)?;
    context::print_banner(ctx.source);

    let service_filter = ctx.service_filter(cli);

    // Only scan configmap and secrets files.
    let target_kinds = [FileKind::ConfigMap, FileKind::Secret];

    let kv_pattern = key_value_pattern();
    let secret_re = secret_key_pattern();

    let mut fake_secrets: Vec<FakeSecret> = Vec::new();

    for service in &ctx.services {
        // Apply service filter.
        if !service_filter.is_empty() && !service_filter.iter().any(|s| s == &service.name) {
            continue;
        }

        for file in &service.files {
            if !target_kinds.contains(&file.kind) {
                continue;
            }
            let content = match fs::read_to_string(&file.path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            // For secrets files, only consider keys under the "data" section.
            // Use the YAML parser which already handles this correctly.
            // For configmaps, we still use the regex approach to catch all
            // key-value pairs.
            let pairs = match file.kind {
                FileKind::Secret => parser::parse(&content, file.kind),
                _ => {
                    // For configmaps, use regex to find all key-value pairs
                    let mut result = Vec::new();
                    for cap in kv_pattern.captures_iter(&content) {
                        let key = cap[1].to_string();
                        let value = cap
                            .get(3)
                            .or_else(|| cap.get(2))
                            .map(|m| m.as_str().trim().to_string())
                            .unwrap_or_default();
                        result.push(parser::ParsedPair { key, value });
                    }
                    result
                }
            };

            for pair in pairs {
                let key = pair.key;
                let value = pair.value;

                let detected = match file.kind {
                    // Category 1: Placeholder values in configmaps, but only for
                    // keys that would NOT be caught by `nv leaks` (i.e., keys
                    // that don't match the secret key naming pattern).
                    FileKind::ConfigMap => !secret_re.is_match(&key) && is_placeholder(&value),
                    // Category 2: Non-secret keys in secrets files.
                    FileKind::Secret => !secret_re.is_match(&key),
                    _ => false,
                };

                if !detected {
                    continue;
                }

                // Skip keys marked as false alarms in nv.yml.
                if let Some(ref cfg) = ctx.config
                    && cfg.is_false_alarm(&service.name, &key)
                {
                    continue;
                }

                fake_secrets.push(FakeSecret {
                    service: service.name.clone(),
                    file_display: file.display.clone(),
                    key,
                    value,
                });
            }
        }
    }

    // Handle --false-alarm: mark the key in all matching fake secrets and save.
    if let Some(fa_key) = false_alarm {
        let matching: Vec<&FakeSecret> = fake_secrets.iter().filter(|f| f.key == *fa_key).collect();
        if matching.is_empty() {
            bail!("no fake secret found with key '{fa_key}'");
        }
        return mark_false_alarm(cli, &ctx, &matching);
    }

    if fake_secrets.is_empty() {
        eprintln!("No fake secrets found.");
        return Ok(());
    }

    let use_color = color::should_use_color();
    let colors = ctx
        .config
        .as_ref()
        .map(|c| c.colors.clone())
        .unwrap_or_default();

    print_fake_secrets(&fake_secrets, &colors, use_color);

    Ok(())
}

/// Print fake secrets grouped by service, then by file, with tree-style
/// vertical lines.
///
/// Each file is shown with its full path relative to the service root.
/// Tree lines are colored to match the text they lead to.
/// Counts are shown next to service names and file names.
fn print_fake_secrets(fake_secrets: &[FakeSecret], colors: &ColorConfig, use_color: bool) {
    let mut grouped: FakeSecretMap = BTreeMap::new();

    for fs in fake_secrets {
        grouped
            .entry(&fs.service)
            .or_default()
            .entry(&fs.file_display)
            .or_default()
            .push((&fs.key, &fs.value));
    }

    for (service, files) in &grouped {
        // Count fake secrets in this service.
        let service_count: usize = files.values().map(|k| k.len()).sum();
        println!(
            "{} {}",
            colorize(&format!("{}/", service), colors.service_root, use_color),
            colorize(
                &format!("({})", service_count),
                colors.service_root,
                use_color
            )
        );

        let file_count = files.len();
        for (i, (file_display, keys)) in files.iter().enumerate() {
            let is_last_file = i + 1 == file_count;
            let branch = if is_last_file {
                "└── "
            } else {
                "├── "
            };
            let pipe = if is_last_file { "    " } else { "│   " };

            // File-level branch and pipe in service color (parent node).
            print!("{}", colorize(branch, colors.service_root, use_color));
            println!(
                "{} {}",
                colorize(file_display, colors.file, use_color),
                colorize(&format!("({})", keys.len()), colors.file, use_color)
            );

            for (j, (key, value)) in keys.iter().enumerate() {
                let is_last_key = j + 1 == keys.len();
                let key_branch = if is_last_key {
                    "└── "
                } else {
                    "├── "
                };
                let key_display = colorize(key, colors.key, use_color);
                let value_display = colorize(value, colors.value, use_color);

                // Pipe in service color (continuation below service), branch in file color.
                print!("{}", colorize(pipe, colors.service_root, use_color));
                print!("{}", colorize(key_branch, colors.file, use_color));
                println!("{} = {}", key_display, value_display);
            }
        }
    }

    eprintln!("\n{} fake secret(s) found.", fake_secrets.len());
}

/// Mark one or more keys as false alarms in nv.yml.
fn mark_false_alarm(
    _cli: &Cli,
    _ctx: &context::Context,
    fake_secrets: &[&FakeSecret],
) -> Result<()> {
    let config_path = config::find_config(&std::env::current_dir()?)
        .unwrap_or_else(|| std::env::current_dir().unwrap().join(CONFIG_FILE));

    let mut cfg = if config_path.exists() {
        config::load(&config_path)?
    } else {
        bail!("no nv.yml found; create one first with `nv init`");
    };

    let mut added = 0usize;
    for fs in fake_secrets {
        if cfg.add_false_alarm(&fs.service, &fs.key) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_xxx() {
        assert!(is_placeholder("xxx"));
        assert!(is_placeholder("XXXX"));
        assert!(is_placeholder("xxx "));
        assert!(is_placeholder(" xxx"));
    }

    #[test]
    fn placeholder_changeme() {
        assert!(is_placeholder("changeme"));
        assert!(is_placeholder("CHANGE_ME"));
        assert!(is_placeholder("change-me"));
    }

    #[test]
    fn placeholder_other_patterns() {
        assert!(is_placeholder("placeholder"));
        assert!(is_placeholder("dummy"));
        assert!(is_placeholder("test"));
        assert!(is_placeholder("example"));
        assert!(is_placeholder("fake"));
        assert!(is_placeholder("secret"));
    }

    #[test]
    fn placeholder_empty() {
        assert!(is_placeholder(""));
        assert!(is_placeholder("   "));
        assert!(is_placeholder("\t"));
    }

    #[test]
    fn not_placeholder() {
        assert!(!is_placeholder("sk-test-123"));
        assert!(!is_placeholder("postgres://localhost"));
        assert!(!is_placeholder("hello world"));
        assert!(!is_placeholder("xx"));
        assert!(!is_placeholder("ab"));
    }

    #[test]
    fn secret_key_pattern_matches() {
        let re = secret_key_pattern();
        assert!(re.is_match("DB_PASSWORD"));
        assert!(re.is_match("API_KEY"));
        assert!(re.is_match("JWT_SECRET"));
        assert!(re.is_match("AUTH_TOKEN"));
        assert!(re.is_match("USER_ID"));
        assert!(re.is_match("ADMIN_USERNAME"));
    }

    #[test]
    fn secret_key_pattern_no_match() {
        let re = secret_key_pattern();
        assert!(!re.is_match("LOG_LEVEL"));
        assert!(!re.is_match("PORT"));
        assert!(!re.is_match("DATABASE_URL"));
        assert!(!re.is_match("API"));
        assert!(!re.is_match("KEY"));
    }

    #[test]
    fn key_value_pattern_matches_dotenv() {
        let re = key_value_pattern();
        let input = "DB_PASSWORD=secret123\nAPI_KEY=sk-test\nLOG_LEVEL=debug\n";
        let keys: Vec<String> = re
            .captures_iter(input)
            .map(|cap| cap[1].to_string())
            .collect();
        assert_eq!(keys, vec!["DB_PASSWORD", "API_KEY", "LOG_LEVEL"]);
    }

    #[test]
    fn key_value_pattern_matches_yaml() {
        let re = key_value_pattern();
        let input = "  DB_PASSWORD: hunter2\n  API_KEY: sk-live\n";
        let keys: Vec<String> = re
            .captures_iter(input)
            .map(|cap| cap[1].to_string())
            .collect();
        assert_eq!(keys, vec!["DB_PASSWORD", "API_KEY"]);
    }

    #[test]
    fn key_value_pattern_matches_export() {
        let re = key_value_pattern();
        let input = "export JWT_TOKEN=abc123\n";
        let keys: Vec<String> = re
            .captures_iter(input)
            .map(|cap| cap[1].to_string())
            .collect();
        assert_eq!(keys, vec!["JWT_TOKEN"]);
    }

    #[test]
    fn false_alarm_roundtrip() {
        let mut cfg = config::Config::default();
        assert!(!cfg.is_false_alarm("auth", "API_KEY"));
        assert!(cfg.add_false_alarm("auth", "API_KEY"));
        assert!(cfg.is_false_alarm("auth", "API_KEY"));
        // Adding again returns false (already present).
        assert!(!cfg.add_false_alarm("auth", "API_KEY"));
    }

    #[test]
    fn secrets_file_only_considers_data_section() {
        let content = "\
apiVersion: v1
kind: Secret
metadata:
  name: my-secret
  labels:
    app: my-app
data:
  DB_PASSWORD: c2VjcmV0
  API_KEY: c2stdGVzdA==
type: Opaque
";
        let pairs = parser::parse(content, FileKind::Secret);
        let keys: Vec<&str> = pairs.iter().map(|p| p.key.as_str()).collect();
        assert_eq!(keys, vec!["DB_PASSWORD", "API_KEY"]);
    }

    #[test]
    fn secrets_file_with_stringdata_section() {
        let content = "\
apiVersion: v1
kind: Secret
metadata:
  name: my-secret
stringData:
  DB_PASSWORD: secret
  API_KEY: sk-test
";
        let pairs = parser::parse(content, FileKind::Secret);
        let keys: Vec<&str> = pairs.iter().map(|p| p.key.as_str()).collect();
        assert_eq!(keys, vec!["DB_PASSWORD", "API_KEY"]);
    }

    #[test]
    fn secrets_file_metadata_keys_not_included() {
        let content = "\
apiVersion: v1
kind: Secret
metadata:
  name: my-secret
  DB_PASSWORD: should-not-be-here
data:
  API_KEY: c2stdGVzdA==
";
        let pairs = parser::parse(content, FileKind::Secret);
        let keys: Vec<&str> = pairs.iter().map(|p| p.key.as_str()).collect();
        assert_eq!(keys, vec!["API_KEY"]);
    }
}
