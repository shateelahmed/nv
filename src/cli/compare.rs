//! `nv compare` — compare an env file against other files of the same kind.

use std::collections::{BTreeMap, HashSet};
use std::fs;

use anyhow::{Result, bail};
use glob::Pattern;

use super::{Cli, context};
use crate::color::{self, AnsiColor, ColorConfig};
use crate::config;
use crate::display::{self, Output, TreeFile, TreeItem, TreeService};
use crate::model::FileKind;
use crate::parser;

/// A parsed key-value pair from a file.
#[derive(Debug, Clone)]
struct ParsedPair {
    key: String,
    value: String,
}

/// A single diff entry between the base file and a comparator file.
#[derive(Debug)]
enum DiffItem {
    /// Key present in base but absent in comparator.
    Missing { key: String, value: String },
    /// Key absent in base but present in comparator.
    Extra { key: String, value: String },
    /// Key in both but values differ (only with --values).
    Different {
        key: String,
        base_value: String,
        peer_value: String,
    },
}

/// Return the file kinds that a base file of `kind` should be compared against.
fn peer_kinds(kind: FileKind) -> Vec<FileKind> {
    match kind {
        FileKind::Dotenv | FileKind::DotenvExample => {
            vec![FileKind::Dotenv, FileKind::DotenvExample]
        }
        FileKind::ConfigMap => vec![FileKind::ConfigMap],
        FileKind::Secret => vec![FileKind::Secret],
    }
}

/// Parse a file's content into key-value pairs.
fn parse_pairs(content: &str, kind: FileKind) -> Vec<ParsedPair> {
    parser::parse(content, kind)
        .into_iter()
        .map(|p| ParsedPair {
            key: p.key,
            value: p.value,
        })
        .collect()
}

/// Build a map from key → value for quick lookup.
fn to_map(pairs: &[ParsedPair]) -> BTreeMap<&str, &str> {
    pairs
        .iter()
        .map(|p| (p.key.as_str(), p.value.as_str()))
        .collect()
}

/// Check if a file path matches any skip_files pattern.
///
/// Supports glob patterns: `*` matches any characters except `/`, `**` matches
/// any characters including `/` (recursive). Matches are tried against both the
/// relative path from the service root and the bare file name.
fn matches_skip_pattern(relative_str: &str, name: &str, skip_files: &HashSet<String>) -> bool {
    for pattern in skip_files {
        if let Ok(glob_pattern) = Pattern::new(pattern)
            && (glob_pattern.matches(relative_str) || glob_pattern.matches(name))
        {
            return true;
        }
    }
    false
}

/// Build the merged set of files to skip for a service — global
/// `commands.compare.skip_files` plus per-service entries.
fn build_skip_files(config: &Option<config::Config>, service: &str) -> HashSet<String> {
    let mut skip: HashSet<String> = HashSet::new();
    if let Some(cfg) = config {
        for file in cfg.compare_skip_files_for(service) {
            skip.insert(file.to_string());
        }
    }
    skip
}

/// Compute the diff between base pairs and peer pairs.
///
/// When `compare_values` is false, only key existence is compared.
fn diff_pairs(base: &[ParsedPair], peer: &[ParsedPair], compare_values: bool) -> Vec<DiffItem> {
    let base_map = to_map(base);
    let peer_map = to_map(peer);

    let mut items: Vec<DiffItem> = Vec::new();

    for (key, val) in &base_map {
        match peer_map.get(key) {
            None => {
                items.push(DiffItem::Missing {
                    key: key.to_string(),
                    value: val.to_string(),
                });
            }
            Some(peer_val) if compare_values && val != peer_val => {
                items.push(DiffItem::Different {
                    key: key.to_string(),
                    base_value: val.to_string(),
                    peer_value: peer_val.to_string(),
                });
            }
            _ => {}
        }
    }

    for (key, val) in &peer_map {
        if !base_map.contains_key(key) {
            items.push(DiffItem::Extra {
                key: key.to_string(),
                value: val.to_string(),
            });
        }
    }

    items
}

/// Build the ANSI-colored label for a diff line.
///
/// `prefix` is `-` or `+`, `color` is the prefix' color, `key` and `value` are
/// content. The value portion gets `colors.value` coloring.
fn diff_label(
    prefix: &str,
    key: &str,
    value: &str,
    prefix_color: AnsiColor,
    colors: &ColorConfig,
    use_color: bool,
) -> String {
    if use_color {
        format!(
            "{} {} = {}{}{}",
            color::colorize(prefix, prefix_color, use_color),
            key,
            colors.value.code(),
            value,
            AnsiColor::Reset.code(),
        )
    } else {
        format!("{} {} = {}", prefix, key, value)
    }
}

/// Handle `nv compare`: compare a base file against peer files.
pub fn run(cli: &Cli, file_path: &str, compare_values: bool) -> Result<()> {
    let ctx = context::resolve(cli)?;
    context::print_banner(ctx.source);

    let service_filter: Vec<&str> = if cli.all || cli.services.is_empty() {
        Vec::new()
    } else {
        cli.services.iter().map(|s| s.as_str()).collect()
    };

    let matching: Vec<(usize, &crate::model::Service)> = ctx
        .services
        .iter()
        .enumerate()
        .filter(|(_, s)| {
            if !service_filter.is_empty() && !service_filter.iter().any(|n| n == &s.name) {
                return false;
            }
            s.files.iter().any(|f| f.display == file_path)
        })
        .collect();

    if matching.is_empty() {
        let tree = available_files_tree(
            &ctx.services,
            &service_filter,
            &ctx.colors(),
            color::should_use_color(),
        );
        let tree = if tree.is_empty() {
            tree
        } else {
            format!("Available files:\n{}", tree)
        };
        bail!("file '{}' not found.\n{}", file_path, tree);
    }

    if matching.len() > 1 {
        let svc_names: Vec<&str> = matching.iter().map(|(_, s)| s.name.as_str()).collect();
        bail!(
            "file '{}' found in multiple services: {}. Use --service to disambiguate.",
            file_path,
            svc_names.join(", ")
        );
    }

    let (base_svc_idx, base_service) = matching[0];
    let base_file = base_service
        .files
        .iter()
        .find(|f| f.display == file_path)
        .expect("file just matched");
    let base_kind = base_file.kind;

    let base_content = fs::read_to_string(&base_file.path)
        .map_err(|e| anyhow::anyhow!("cannot read base file '{}': {}", base_file.display, e))?;
    let base_pairs = parse_pairs(&base_content, base_kind);

    let peer_kinds = peer_kinds(base_kind);
    let mut comparisons: BTreeMap<String, BTreeMap<String, Vec<DiffItem>>> = BTreeMap::new();

    for (svc_idx, service) in ctx.services.iter().enumerate() {
        if !service_filter.is_empty() && !service_filter.iter().any(|n| n == &service.name) {
            continue;
        }

        let skip_files = build_skip_files(&ctx.config, &service.name);

        for file in &service.files {
            if !peer_kinds.contains(&file.kind) {
                continue;
            }
            if svc_idx == base_svc_idx && file.display == base_file.display {
                continue;
            }

            // Skip peer files matching configured skip_files patterns.
            let relative = file.path.strip_prefix(&service.path).unwrap_or(&file.path);
            let relative_str = relative.to_str().unwrap_or("");
            let name = file.path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if matches_skip_pattern(relative_str, name, &skip_files) {
                continue;
            }

            let content = match fs::read_to_string(&file.path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let peer_pairs = parse_pairs(&content, file.kind);
            let diffs = diff_pairs(&base_pairs, &peer_pairs, compare_values);

            if diffs.is_empty() {
                continue;
            }

            comparisons
                .entry(service.name.clone())
                .or_default()
                .entry(file.display.clone())
                .or_insert(diffs);
        }
    }

    if comparisons.is_empty() {
        eprintln!("No comparisons found.");
        return Ok(());
    }

    let use_color = color::should_use_color();
    let colors = ctx.colors();

    print_comparisons(&comparisons, &colors, use_color);

    Ok(())
}

/// Build the available-files listing in the uniform tree format, as a string.
///
/// Used when the requested base file path is not found, so the user can see the
/// valid paths at a glance. When `service_filter` is non-empty, only those
/// services' files are included.
fn available_files_tree(
    services: &[crate::model::Service],
    service_filter: &[&str],
    colors: &ColorConfig,
    use_color: bool,
) -> String {
    let trees: Vec<TreeService> = services
        .iter()
        .filter(|s| {
            !s.files.is_empty()
                && (service_filter.is_empty() || service_filter.iter().any(|n| n == &s.name))
        })
        .map(|s| TreeService {
            name: s.name.clone(),
            count: s.files.len(),
            files: s
                .files
                .iter()
                .map(|f| TreeFile {
                    name: f.display.clone(),
                    count: 0,
                    items: Vec::new(),
                })
                .collect(),
        })
        .collect();

    let mut out = Output::String(String::new());
    display::render_tree(&trees, colors, use_color, false, &mut out);
    match out {
        Output::String(s) => s,
        _ => unreachable!("we always render into a string"),
    }
}

/// Print comparison results grouped by service, then by file.
fn print_comparisons(
    comparisons: &BTreeMap<String, BTreeMap<String, Vec<DiffItem>>>,
    colors: &ColorConfig,
    use_color: bool,
) {
    let mut total = 0usize;

    let services: Vec<TreeService> = comparisons
        .iter()
        .map(|(service_name, files)| {
            let service_count: usize = files.values().map(|d| d.len()).sum();
            total += service_count;

            let tree_files: Vec<TreeFile> = files
                .iter()
                .map(|(file_name, diffs)| {
                    let mut items: Vec<TreeItem> = Vec::with_capacity(diffs.len());

                    for diff in diffs {
                        match diff {
                            DiffItem::Missing { key, value } => {
                                items.push(TreeItem {
                                    label: diff_label(
                                        "-",
                                        key,
                                        value,
                                        colors.removed,
                                        colors,
                                        use_color,
                                    ),
                                    color: colors.removed,
                                });
                            }
                            DiffItem::Extra { key, value } => {
                                items.push(TreeItem {
                                    label: diff_label(
                                        "+",
                                        key,
                                        value,
                                        colors.added,
                                        colors,
                                        use_color,
                                    ),
                                    color: colors.added,
                                });
                            }
                            DiffItem::Different {
                                key,
                                base_value,
                                peer_value,
                            } => {
                                items.push(TreeItem {
                                    label: diff_label(
                                        "-",
                                        key,
                                        base_value,
                                        colors.removed,
                                        colors,
                                        use_color,
                                    ),
                                    color: colors.removed,
                                });
                                items.push(TreeItem {
                                    label: diff_label(
                                        "+",
                                        key,
                                        peer_value,
                                        colors.added,
                                        colors,
                                        use_color,
                                    ),
                                    color: colors.added,
                                });
                            }
                        }
                    }

                    TreeFile {
                        name: file_name.clone(),
                        count: items.len(),
                        items,
                    }
                })
                .collect();

            TreeService {
                name: service_name.clone(),
                count: service_count,
                files: tree_files,
            }
        })
        .collect();

    let mut out = Output::Stdout;
    display::render_tree(&services, colors, use_color, true, &mut out);

    eprintln!("\n{} difference(s) found.", total);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::FileKind;

    #[test]
    fn test_peer_kinds_dotenv() {
        let kinds = peer_kinds(FileKind::Dotenv);
        assert_eq!(kinds, vec![FileKind::Dotenv, FileKind::DotenvExample]);
    }

    #[test]
    fn test_peer_kinds_dotenv_example() {
        let kinds = peer_kinds(FileKind::DotenvExample);
        assert_eq!(kinds, vec![FileKind::Dotenv, FileKind::DotenvExample]);
    }

    #[test]
    fn test_peer_kinds_configmap() {
        let kinds = peer_kinds(FileKind::ConfigMap);
        assert_eq!(kinds, vec![FileKind::ConfigMap]);
    }

    #[test]
    fn test_peer_kinds_secret() {
        let kinds = peer_kinds(FileKind::Secret);
        assert_eq!(kinds, vec![FileKind::Secret]);
    }

    #[test]
    fn test_diff_keys_only() {
        let base = vec![
            ParsedPair {
                key: "A".into(),
                value: "1".into(),
            },
            ParsedPair {
                key: "B".into(),
                value: "2".into(),
            },
            ParsedPair {
                key: "C".into(),
                value: "3".into(),
            },
        ];
        let peer = vec![
            ParsedPair {
                key: "A".into(),
                value: "1".into(),
            },
            ParsedPair {
                key: "C".into(),
                value: "3".into(),
            },
            ParsedPair {
                key: "D".into(),
                value: "4".into(),
            },
        ];

        let diffs = diff_pairs(&base, &peer, false);
        assert_eq!(diffs.len(), 2);

        assert!(
            diffs
                .iter()
                .any(|d| matches!(d, DiffItem::Missing { key, .. } if key == "B"))
        );
        assert!(
            diffs
                .iter()
                .any(|d| matches!(d, DiffItem::Extra { key, .. } if key == "D"))
        );
        assert!(
            !diffs
                .iter()
                .any(|d| matches!(d, DiffItem::Different { .. }))
        );
    }

    #[test]
    fn test_diff_with_values() {
        let base = vec![
            ParsedPair {
                key: "A".into(),
                value: "1".into(),
            },
            ParsedPair {
                key: "B".into(),
                value: "2".into(),
            },
        ];
        let peer = vec![
            ParsedPair {
                key: "A".into(),
                value: "1".into(),
            },
            ParsedPair {
                key: "B".into(),
                value: "changed".into(),
            },
            ParsedPair {
                key: "C".into(),
                value: "3".into(),
            },
        ];

        let diffs = diff_pairs(&base, &peer, true);
        assert!(
            diffs
                .iter()
                .any(|d| matches!(d, DiffItem::Different { key, .. } if key == "B"))
        );
        assert!(
            !diffs
                .iter()
                .any(|d| matches!(d, DiffItem::Missing { key, .. } if key == "B"))
        );
        assert!(
            !diffs
                .iter()
                .any(|d| matches!(d, DiffItem::Extra { key, .. } if key == "B"))
        );
        assert!(
            diffs
                .iter()
                .any(|d| matches!(d, DiffItem::Extra { key, .. } if key == "C"))
        );
    }

    #[test]
    fn test_diff_empty_base() {
        let base: Vec<ParsedPair> = vec![];
        let peer = vec![ParsedPair {
            key: "A".into(),
            value: "1".into(),
        }];
        let diffs = diff_pairs(&base, &peer, false);
        assert_eq!(diffs.len(), 1);
        assert!(matches!(diffs[0], DiffItem::Extra { ref key, .. } if key == "A"));
    }

    #[test]
    fn test_diff_empty_peer() {
        let base = vec![ParsedPair {
            key: "A".into(),
            value: "1".into(),
        }];
        let peer: Vec<ParsedPair> = vec![];
        let diffs = diff_pairs(&base, &peer, false);
        assert_eq!(diffs.len(), 1);
        assert!(matches!(diffs[0], DiffItem::Missing { ref key, .. } if key == "A"));
    }

    #[test]
    fn test_diff_identical_files() {
        let base = vec![ParsedPair {
            key: "A".into(),
            value: "1".into(),
        }];
        let peer = vec![ParsedPair {
            key: "A".into(),
            value: "1".into(),
        }];
        let diffs = diff_pairs(&base, &peer, true);
        assert!(diffs.is_empty());
    }

    #[test]
    fn test_diff_label_no_color() {
        let colors = ColorConfig::default();
        let label = diff_label("-", "MY_KEY", "my_value", colors.removed, &colors, false);
        assert_eq!(label, "- MY_KEY = my_value");
    }

    #[test]
    fn test_diff_label_prefix_color() {
        let colors = ColorConfig::default();
        // When use_color is true, the label contains ANSI codes.
        let label = diff_label("+", "K", "v", colors.added, &colors, true);
        assert!(label.starts_with("\x1b["));
        assert!(label.contains("+"));
        assert!(label.contains("K = "));
        assert!(label.contains("v"));
        assert!(label.ends_with("\x1b[0m"));
    }

    fn skip_set(patterns: &[&str]) -> HashSet<String> {
        patterns.iter().map(|p| p.to_string()).collect()
    }

    #[test]
    fn test_matches_skip_pattern_exact_relative_path() {
        let skip = skip_set(&["docker/.env"]);
        assert!(matches_skip_pattern("docker/.env", ".env", &skip));
        assert!(!matches_skip_pattern(
            "docker/.env.example",
            ".env.example",
            &skip
        ));
    }

    #[test]
    fn test_matches_skip_pattern_file_name() {
        let skip = skip_set(&["custom.env"]);
        // A bare name matches any sub-path ending in that file name.
        assert!(matches_skip_pattern(
            "docker/custom.env",
            "custom.env",
            &skip
        ));
        assert!(!matches_skip_pattern(
            "docker/other.env",
            "other.env",
            &skip
        ));
    }

    #[test]
    fn test_matches_skip_pattern_glob_double_star() {
        let skip = skip_set(&["docker/**/*.env*"]);
        assert!(matches_skip_pattern(
            "docker/app/.env.example",
            ".env.example",
            &skip
        ));
        assert!(matches_skip_pattern("docker/.env", ".env", &skip));
        assert!(!matches_skip_pattern("billing/.env", ".env", &skip));
    }

    #[test]
    fn test_matches_skip_pattern_single_glob() {
        let skip = skip_set(&["*.test.env"]);
        assert!(matches_skip_pattern(
            "auth.test.env",
            "auth.test.env",
            &skip
        ));
        // The bare-name match also covers nested paths with the same file name.
        assert!(matches_skip_pattern(
            "docker/auth.test.env",
            "auth.test.env",
            &skip
        ));
        // But not a different file name.
        assert!(!matches_skip_pattern("docker/app.env", "app.env", &skip));
    }

    #[test]
    fn test_build_skip_files_empty_without_config() {
        let skip = build_skip_files(&None, "auth");
        assert!(skip.is_empty());
    }
}
