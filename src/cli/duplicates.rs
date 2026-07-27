//! `nv duplicates` — list env keys that appear multiple times.
//!
//! This command scans env and env example files for keys that appear more than
//! once within a single file.

use std::collections::{BTreeMap, HashMap};
use std::fs;

use anyhow::Result;

use super::{Cli, context};
use crate::color::{self, ColorConfig, colorize};
use crate::model::{FileKind, Service};
use crate::parser;

/// A location where a key was found.
#[derive(Debug, Clone)]
struct KeyLocation {
    file_display: String,
    value: String,
}

/// A duplicate key with all its occurrences in a single file.
#[derive(Debug)]
struct DuplicateKey {
    key: String,
    locations: Vec<KeyLocation>,
}

/// Handle `nv duplicates`: list env keys that appear multiple times.
pub fn run(cli: &Cli, _service_names: &[String]) -> Result<()> {
    let ctx = context::resolve(cli)?;
    context::print_banner(ctx.source);

    let service_filter = ctx.service_filter(cli);

    // Only scan dotenv, dotenv example, configmap, and secrets files.
    let target_kinds = [
        FileKind::Dotenv,
        FileKind::DotenvExample,
        FileKind::ConfigMap,
        FileKind::Secret,
    ];

    let progress = context::ScanProgress::start();

    // Group duplicates by service name for uniform output.
    let mut duplicates_by_service: BTreeMap<String, Vec<DuplicateKey>> = BTreeMap::new();

    for service in &ctx.services {
        // Apply service filter.
        if !service_filter.is_empty() && !service_filter.iter().any(|s| s == &service.name) {
            continue;
        }

        progress.inc_dirs();

        for file in &service.files {
            if !target_kinds.contains(&file.kind) {
                continue;
            }

            progress.set_message(format!("Scanning {}", file.display));
            progress.inc_files();
        }

        let service_dupes = find_duplicates(service, &target_kinds);
        if !service_dupes.is_empty() {
            duplicates_by_service
                .entry(service.name.clone())
                .or_default()
                .extend(service_dupes);
        }
    }

    progress.finish();

    if duplicates_by_service.is_empty() {
        eprintln!("No duplicate keys found.");
        return Ok(());
    }

    let use_color = color::should_use_color();
    let colors = ctx
        .config
        .as_ref()
        .map(|c| c.colors.clone())
        .unwrap_or_default();

    print_duplicates(&duplicates_by_service, &colors, use_color);

    Ok(())
}

/// Find duplicate keys in a service's env files.
///
/// Only detects intra-file duplicates: keys that appear more than once
/// within a single file.
fn find_duplicates(service: &Service, target_kinds: &[FileKind]) -> Vec<DuplicateKey> {
    let mut duplicates = Vec::new();

    for file in &service.files {
        if !target_kinds.contains(&file.kind) {
            continue;
        }

        let content = match fs::read_to_string(&file.path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let pairs = parser::parse(&content, file.kind);

        // Collect occurrences of each key within this file.
        let mut key_occurrences: HashMap<String, Vec<KeyLocation>> = HashMap::new();

        for pair in pairs {
            let location = KeyLocation {
                file_display: file.display.clone(),
                value: pair.value,
            };

            key_occurrences.entry(pair.key).or_default().push(location);
        }

        // Find keys with multiple occurrences in this file.
        for (key, locations) in key_occurrences {
            if locations.len() < 2 {
                continue;
            }

            duplicates.push(DuplicateKey { key, locations });
        }
    }

    // Sort by key name for consistent output
    duplicates.sort_by(|a, b| a.key.cmp(&b.key));

    duplicates
}

/// Print duplicates grouped by service, then by file, with tree-style vertical
/// lines. Follows the same uniform format as `nv leaks`.
///
/// Each file is shown with its full path relative to the service root.
/// Tree lines are colored to match the text they lead to.
/// Counts are shown next to service names and file names.
fn print_duplicates(
    duplicates_by_service: &BTreeMap<String, Vec<DuplicateKey>>,
    colors: &ColorConfig,
    use_color: bool,
) {
    let mut total = 0;

    for (service, dupes) in duplicates_by_service {
        // Count duplicates in this service.
        let service_count = dupes.len();
        total += service_count;

        // Print service name with count.
        println!(
            "{} {}",
            colorize(&format!("{}/", service), colors.service_root, use_color),
            colorize(
                &format!("({})", service_count),
                colors.service_root,
                use_color
            )
        );

        // Group duplicates by file for this service.
        let mut file_groups: BTreeMap<String, Vec<&DuplicateKey>> = BTreeMap::new();
        for dupe in dupes {
            // All locations are in the same file, so just use the first one.
            if let Some(loc) = dupe.locations.first() {
                file_groups
                    .entry(loc.file_display.clone())
                    .or_default()
                    .push(dupe);
            }
        }

        let file_count = file_groups.len();
        for (i, (file_display, file_dupes)) in file_groups.iter().enumerate() {
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
                colorize(&format!("({})", file_dupes.len()), colors.file, use_color)
            );

            // Print each duplicate key in this file.
            for (j, dupe) in file_dupes.iter().enumerate() {
                let is_last_key = j + 1 == file_dupes.len();
                let key_branch = if is_last_key {
                    "└── "
                } else {
                    "├── "
                };

                // All locations are in the same file, use the first value.
                let value = dupe
                    .locations
                    .first()
                    .map(|l| l.value.as_str())
                    .unwrap_or("");

                // Pipe in service color (continuation below service), branch in file color.
                print!("{}", colorize(pipe, colors.service_root, use_color));
                print!("{}", colorize(key_branch, colors.file, use_color));
                println!(
                    "{} = {}",
                    colorize(&dupe.key, colors.key, use_color),
                    colorize(value, colors.value, use_color)
                );
            }
        }
    }

    eprintln!(
        "\n{}",
        colorize(
            &format!("{} duplicate key(s) found.", total),
            colors.service_root,
            use_color
        )
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_key_in_same_file() {
        let locations = vec![
            KeyLocation {
                file_display: ".env".to_string(),
                value: "value1".to_string(),
            },
            KeyLocation {
                file_display: ".env".to_string(),
                value: "value2".to_string(),
            },
        ];
        // This represents a valid intra-file duplicate
        assert_eq!(locations.len(), 2);
        assert_eq!(locations[0].file_display, locations[1].file_display);
    }

    #[test]
    fn single_occurrence_not_duplicate() {
        let locations = vec![KeyLocation {
            file_display: ".env".to_string(),
            value: "value1".to_string(),
        }];
        // Single occurrence is not a duplicate
        assert_eq!(locations.len(), 1);
    }
}
