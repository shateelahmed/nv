//! `nv leaks` — list and clean keys that look like secrets in example and
//! configmap files.

use std::collections::BTreeMap;
use std::fs;

use anyhow::{Result, bail};
use regex::Regex;

use super::{Cli, context};
use crate::color::{self, ColorConfig};
use crate::display::{self, Output, TreeFile, TreeItem, TreeService};
use crate::edit::{self, ChangeSet};
use crate::model::FileKind;

/// Regex matching keys that suggest hardcoded secrets in dotenv/YAML files.
///
/// Matches lines like:
/// - `export DB_PASSWORD=hunter2`
/// - `API_KEY=sk-...`
/// - `JWT_SECRET: some-value`
///
/// Note: the Rust `regex` crate does not support look-around, so the original
/// negative look-ahead `(?!\s*$)` is expressed with `\S` (non-whitespace)
/// instead — the dotenv branch requires the value to start with a non-whitespace
/// character.
fn leak_pattern() -> Regex {
    // The `(?m)` flag makes `^` and `$` match line boundaries.
    Regex::new(
        r"(?m)^\s*(?:export\s+)?([A-Za-z0-9_][A-Z0-9_]+_(?:KEY|PASSWORD|SECRET|TOKEN|ID|USERNAME))[^\S\n]*(?::\s*(.+)|=[^\S\n]*(\S.*))$",
    )
    .expect("hardcoded regex is valid")
}

/// Build a regex that matches any of the given special secret keys.
///
/// Uses the same value-extraction capture groups as [`leak_pattern`] so that
/// the two passes can share the same post-processing logic:
///   - Group 1: the key name
///   - Group 2: YAML-colon value (optional)
///   - Group 3: dotenv value (optional)
fn special_secret_key_pattern(keys: &[&str]) -> Regex {
    let escaped: Vec<String> = keys.iter().map(|k| regex::escape(k)).collect();
    let pattern = format!(
        r"(?m)^\s*(?:export\s+)?({})[^\S\n]*(?::\s*(.+)|=[^\S\n]*(\S.*))$",
        escaped.join("|")
    );
    Regex::new(&pattern).expect("special key regex is valid")
}

/// A single detected potential leak.
struct Leak {
    service: String,
    file_display: String,
    key: String,
    value: String,
}

/// Grouped display structure: service -> file -> list of key/value pairs.
type LeakMap<'a> = BTreeMap<&'a str, BTreeMap<&'a str, Vec<(&'a str, &'a str)>>>;

/// Handle `nv leaks`: scan example and configmap files for keys that look like
/// hardcoded secrets and print them.
///
/// Flags:
/// - `--clean`: remove detected keys from configmaps, set empty in examples.
/// - `--false-alarm <KEY>`: mark a key as a false alarm (saved to nv.yml).
pub fn run(cli: &Cli, clean: bool, false_alarm: &Option<String>) -> Result<()> {
    let ctx = context::resolve(cli)?;
    context::print_banner(ctx.source);

    let service_filter = ctx.service_filter(cli);

    // Only scan example and configmap files — these are the files most likely
    // to contain placeholder or accidentally committed secrets.
    let target_kinds = [FileKind::DotenvExample, FileKind::ConfigMap];

    let pattern = leak_pattern();

    let progress = context::ScanProgress::start();

    let mut leaks: Vec<Leak> = Vec::new();

    for service in &ctx.services {
        // Apply service filter.
        if !service_filter.is_empty() && !service_filter.iter().any(|s| s == &service.name) {
            continue;
        }

        progress.inc_dirs();

        // Build a special-secret-keys regex if any are configured for this
        // service (global + per-service merged).
        let special_pattern: Option<Regex> = ctx.config.as_ref().and_then(|cfg| {
            let keys = cfg.special_secret_keys_for(&service.name);
            if keys.is_empty() {
                None
            } else {
                Some(special_secret_key_pattern(&keys))
            }
        });

        for file in &service.files {
            if !target_kinds.contains(&file.kind) {
                continue;
            }

            progress.set_message(format!("Scanning {}", file.display));
            progress.inc_files();

            let content = match fs::read_to_string(&file.path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            // Collect matches deduplicated by key name. Both the built-in
            // regex and the special-secret-keys regex may match a key; a key
            // should only be reported once.
            let mut file_leaks: BTreeMap<String, String> = BTreeMap::new();

            // Pass 1: built-in regex pattern.
            for cap in pattern.captures_iter(&content) {
                let key = cap[1].to_string();
                let value = cap
                    .get(3)
                    .or_else(|| cap.get(2))
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_default();
                file_leaks.entry(key).or_insert(value);
            }

            // Pass 2: special-secret-keys pattern (if configured).
            if let Some(ref sp) = special_pattern {
                for cap in sp.captures_iter(&content) {
                    let key = cap[1].to_string();
                    let value = cap
                        .get(3)
                        .or_else(|| cap.get(2))
                        .map(|m| m.as_str().to_string())
                        .unwrap_or_default();
                    file_leaks.entry(key).or_insert(value);
                }
            }

            // Push matched keys to the global leaks list, skipping false
            // alarms.
            for (key, value) in file_leaks {
                if let Some(ref cfg) = ctx.config
                    && cfg.is_false_alarm(&service.name, &key)
                {
                    continue;
                }

                leaks.push(Leak {
                    service: service.name.clone(),
                    file_display: file.display.clone(),
                    key,
                    value,
                });
            }
        }
    }

    progress.finish();

    // Handle --false-alarm: mark the key in all matching leaks and save.
    if let Some(fa_key) = false_alarm {
        let matching: Vec<&Leak> = leaks.iter().filter(|l| l.key == *fa_key).collect();
        if matching.is_empty() {
            bail!("no leak found with key '{fa_key}'");
        }
        let pairs: Vec<(&str, &str)> = matching
            .iter()
            .map(|l| (l.service.as_str(), l.key.as_str()))
            .collect();
        return context::mark_false_alarm(&pairs);
    }

    if leaks.is_empty() {
        eprintln!("No potential leaks found.");
        return Ok(());
    }

    if clean {
        return run_clean(cli, &ctx, &leaks);
    }

    let use_color = color::should_use_color();
    let colors = ctx.colors();

    print_leaks(&leaks, &colors, use_color);

    Ok(())
}

/// Print leaks grouped by service, then by file, with tree-style vertical lines.
fn print_leaks(leaks: &[Leak], colors: &ColorConfig, use_color: bool) {
    let mut grouped: LeakMap = BTreeMap::new();

    for leak in leaks {
        grouped
            .entry(&leak.service)
            .or_default()
            .entry(&leak.file_display)
            .or_default()
            .push((&leak.key, &leak.value));
    }

    let services: Vec<TreeService> = grouped
        .into_iter()
        .map(|(service_name, files)| {
            let service_count: usize = files.values().map(|k| k.len()).sum();
            let tree_files: Vec<TreeFile> = files
                .into_iter()
                .map(|(file_name, keys)| {
                    let items: Vec<TreeItem> = keys
                        .iter()
                        .map(|(k, v)| TreeItem {
                            label: format!("{} = {}", k, v),
                            color: colors.key,
                        })
                        .collect();
                    TreeFile {
                        name: file_name.to_string(),
                        count: items.len(),
                        items,
                    }
                })
                .collect();
            TreeService {
                name: service_name.to_string(),
                count: service_count,
                files: tree_files,
            }
        })
        .collect();

    let mut out = Output::Stdout;
    display::render_tree(&services, colors, use_color, &mut out);

    eprintln!("\n{} potential leak(s) found.", leaks.len());
}

/// Build and apply changes for --clean mode: remove keys from configmaps,
/// set empty values in example files. Uses the standard preview-and-confirm
/// flow.
fn run_clean(cli: &Cli, ctx: &context::Context, leaks: &[Leak]) -> Result<()> {
    let service_filter = ctx.service_filter(cli);
    let kind_filter = vec![FileKind::DotenvExample, FileKind::ConfigMap];
    let targets = edit::collect_targets(&ctx.services, &service_filter, &kind_filter);

    if targets.is_empty() {
        bail!("no matching files found");
    }

    // Collect the set of keys to clean per (service, file_display).
    let mut keys_to_clean: BTreeMap<(&str, &str), Vec<&str>> = BTreeMap::new();
    for leak in leaks {
        keys_to_clean
            .entry((&leak.service, leak.file_display.as_str()))
            .or_default()
            .push(&leak.key);
    }

    // Build a change set: for each target file, compute new content with the
    // matching keys cleaned.
    let mut changes = ChangeSet::default();
    for target in &targets {
        let file_keys =
            match keys_to_clean.get(&(target.service.as_str(), target.file.display.as_str())) {
                Some(k) => k,
                None => continue,
            };

        let old_content = edit::read_or_empty(&target.file.path)?;
        let mut new_content = old_content.clone();

        for key in file_keys {
            if target.file.kind == FileKind::ConfigMap {
                // Remove the key entirely from configmaps.
                new_content = crate::parser::remove_key(&new_content, target.file.kind, key);
            } else {
                // Set empty value in example files.
                new_content = crate::parser::set_value(&new_content, target.file.kind, key, "");
            }
        }

        if new_content != old_content {
            changes.changes.push(edit::FileChange {
                service: target.service.clone(),
                display: target.file.display.clone(),
                path: target.file.path.clone(),
                kind: target.file.kind,
                key: String::new(),
                value: String::new(),
                old_content,
                new_content,
            });
        }
    }

    let use_color = color::should_use_color();
    let colors = ctx.colors();
    context::preview_and_apply(cli, &changes, &colors, use_color)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config;

    #[test]
    fn matches_dotenv_secret_keys() {
        let re = leak_pattern();
        let input = "DB_PASSWORD=secret123\nAPI_KEY=sk-test\nLOG_LEVEL=debug\n";
        let keys: Vec<String> = re
            .captures_iter(input)
            .map(|cap| cap[1].to_string())
            .collect();
        assert_eq!(keys, vec!["DB_PASSWORD", "API_KEY"]);
    }

    #[test]
    fn matches_export_prefix() {
        let re = leak_pattern();
        let input = "export JWT_TOKEN=abc123\n";
        let keys: Vec<String> = re
            .captures_iter(input)
            .map(|cap| cap[1].to_string())
            .collect();
        assert_eq!(keys, vec!["JWT_TOKEN"]);
    }

    #[test]
    fn matches_yaml_syntax() {
        let re = leak_pattern();
        let input = "  DB_PASSWORD: hunter2\n  API_KEY: sk-live\n";
        let keys: Vec<String> = re
            .captures_iter(input)
            .map(|cap| cap[1].to_string())
            .collect();
        assert_eq!(keys, vec!["DB_PASSWORD", "API_KEY"]);
    }

    #[test]
    fn skips_empty_values() {
        let re = leak_pattern();
        let input = "DB_PASSWORD=\nAPI_KEY=\n";
        let keys: Vec<String> = re
            .captures_iter(input)
            .map(|cap| cap[1].to_string())
            .collect();
        assert!(keys.is_empty());
    }

    #[test]
    fn skips_unrelated_keys() {
        let re = leak_pattern();
        let input = "LOG_LEVEL=debug\nPORT=8080\nDATABASE_URL=postgres://localhost\n";
        let keys: Vec<String> = re
            .captures_iter(input)
            .map(|cap| cap[1].to_string())
            .collect();
        assert!(keys.is_empty());
    }

    #[test]
    fn false_alarm_roundtrip() {
        let mut cfg = config::Config::default();
        assert!(!cfg.is_false_alarm("auth", "DB_PASSWORD"));
        assert!(cfg.add_false_alarm("auth", "DB_PASSWORD"));
        assert!(cfg.is_false_alarm("auth", "DB_PASSWORD"));
        // Adding again returns false (already present).
        assert!(!cfg.add_false_alarm("auth", "DB_PASSWORD"));
    }

    #[test]
    fn special_secret_key_pattern_matches_dotenv() {
        let re = special_secret_key_pattern(&["MYAPP_CREDENTIALS", "SOME_SALT"]);
        let input = "MYAPP_CREDENTIALS=supersecret\nLOG_LEVEL=debug\nSOME_SALT=abc\n";
        let keys: Vec<String> = re
            .captures_iter(input)
            .map(|cap| cap[1].to_string())
            .collect();
        assert_eq!(keys, vec!["MYAPP_CREDENTIALS", "SOME_SALT"]);
    }

    #[test]
    fn special_secret_key_pattern_matches_yaml() {
        let re = special_secret_key_pattern(&["MYAPP_CREDENTIALS"]);
        let input = "MYAPP_CREDENTIALS: supersecret\n";
        let keys: Vec<String> = re
            .captures_iter(input)
            .map(|cap| cap[1].to_string())
            .collect();
        assert_eq!(keys, vec!["MYAPP_CREDENTIALS"]);
    }

    #[test]
    fn special_secret_key_pattern_respects_export_prefix() {
        let re = special_secret_key_pattern(&["MYAPP_CREDENTIALS"]);
        let input = "export MYAPP_CREDENTIALS=supersecret\n";
        let keys: Vec<String> = re
            .captures_iter(input)
            .map(|cap| cap[1].to_string())
            .collect();
        assert_eq!(keys, vec!["MYAPP_CREDENTIALS"]);
    }

    #[test]
    fn special_secret_key_pattern_skips_empty_values() {
        let re = special_secret_key_pattern(&["MYAPP_CREDENTIALS"]);
        let input = "MYAPP_CREDENTIALS=\nOTHER_KEY=value\n";
        let keys: Vec<String> = re
            .captures_iter(input)
            .map(|cap| cap[1].to_string())
            .collect();
        assert!(keys.is_empty());
    }

    #[test]
    fn special_secret_key_pattern_extracts_values() {
        let re = special_secret_key_pattern(&["MYAPP_CREDENTIALS"]);
        let input = "MYAPP_CREDENTIALS=supersecret\n";
        let caps: Vec<String> = re
            .captures_iter(input)
            .map(|cap| {
                let key = cap[1].to_string();
                let value = cap
                    .get(3)
                    .or_else(|| cap.get(2))
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_default();
                format!("{}={}", key, value)
            })
            .collect();
        assert_eq!(caps, vec!["MYAPP_CREDENTIALS=supersecret"]);
    }

    #[test]
    fn special_secret_key_does_not_interfere_with_regex_pattern() {
        // A key that matches both regex and special_secret_keys should be
        // found at least once (dedup is tested at the integration level).
        let re = leak_pattern();
        let input = "API_KEY=sk-test\n";
        let keys: Vec<String> = re
            .captures_iter(input)
            .map(|cap| cap[1].to_string())
            .collect();
        assert_eq!(keys, vec!["API_KEY"]);
    }
}
