//! `nv compare` — compare an env file against other files of the same kind.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;

use anyhow::{Result, bail};

use super::{Cli, context};
use crate::color::{self, AnsiColor, ColorConfig};
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
    /// Key in both but at a different position (only with --order). Positions
    /// are 1-based line indexes in each file.
    OutOfOrder {
        key: String,
        base_pos: usize,
        peer_pos: usize,
    },
    /// Comment attached to a key present in both files differs (only with
    /// --comments). One side may have no comment at all.
    CommentDiff {
        key: String,
        base_comment: String,
        peer_comment: String,
    },
    /// Comment present in base but absent in comparator (only with --comments).
    CommentMissing { comment: String },
    /// Comment absent in base but present in comparator (only with --comments).
    CommentExtra { comment: String },
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

/// Human-readable kind label for the "Available ... files:" header.
fn kind_label(kind: FileKind) -> &'static str {
    match kind {
        FileKind::Dotenv | FileKind::DotenvExample => "env",
        FileKind::ConfigMap => "configmap",
        FileKind::Secret => "secrets",
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

/// Compute the order diff between base pairs and peer pairs.
///
/// Only keys present in BOTH files participate. Walking the peer's common keys
/// in file order, a key is reported when its base position is smaller than the
/// largest base position seen so far — i.e. it appears "too early" relative to
/// the base file's ordering. Each key is reported at most once.
fn order_diffs(base: &[ParsedPair], peer: &[ParsedPair]) -> Vec<DiffItem> {
    // Base position (0-based) of the first occurrence of each key.
    let mut base_pos: BTreeMap<&str, usize> = BTreeMap::new();
    for (i, p) in base.iter().enumerate() {
        base_pos.entry(p.key.as_str()).or_insert(i);
    }
    if base_pos.is_empty() {
        return Vec::new();
    }

    let mut seen: HashSet<&str> = HashSet::new();
    let mut max_base = 0usize;
    let mut items: Vec<DiffItem> = Vec::new();
    for (peer_i, p) in peer.iter().enumerate() {
        let Some(&b) = base_pos.get(p.key.as_str()) else {
            continue; // key absent from the base file — not an order issue
        };
        if !seen.insert(p.key.as_str()) {
            continue; // duplicate key — only the first occurrence counts
        }
        if b < max_base {
            items.push(DiffItem::OutOfOrder {
                key: p.key.clone(),
                base_pos: b + 1,
                peer_pos: peer_i + 1,
            });
        }
        max_base = max_base.max(b);
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

/// Build the ANSI-colored label for an order-diff line (`- KEY (#N)`).
fn order_label(
    prefix: &str,
    key: &str,
    position: usize,
    prefix_color: AnsiColor,
    use_color: bool,
) -> String {
    if use_color {
        format!(
            "{} {} (#{})",
            color::colorize(prefix, prefix_color, use_color),
            key,
            position,
        )
    } else {
        format!("{} {} (#{})", prefix, key, position)
    }
}

/// Compute the `--comments` diff between base and peer files.
///
/// Two passes run together, per-key items first:
///
/// 1. **Per-key pass.** Only keys present in BOTH files participate. A key's
///    attached comment is compared (a key documented on one side only counts
///    as a difference) and reported as a `CommentDiff` pair, in sorted key
///    order.
/// 2. **Other-comments pass.** The comment lines consumed by pass 1 (the
///    attached lines of keys present in both files) are removed from each
///    file's comment pool, and the remaining pools are diffed as multisets.
///
/// Comments attached to keys present in only one file are never consumed by
/// the per-key pass, so they appear in the other-comments pass instead.
fn comment_diffs(
    base_pairs: &[ParsedPair],
    peer_pairs: &[ParsedPair],
    base_attached: &[parser::ParsedComment],
    peer_attached: &[parser::ParsedComment],
    base_comments: &[String],
    peer_comments: &[String],
) -> Vec<DiffItem> {
    let base_keys: HashSet<&str> = base_pairs.iter().map(|p| p.key.as_str()).collect();
    let peer_keys: HashSet<&str> = peer_pairs.iter().map(|p| p.key.as_str()).collect();

    let base_map: BTreeMap<&str, &str> = base_attached
        .iter()
        .map(|c| (c.key.as_str(), c.comment.as_str()))
        .collect();
    let peer_map: BTreeMap<&str, &str> = peer_attached
        .iter()
        .map(|c| (c.key.as_str(), c.comment.as_str()))
        .collect();

    // Per-key pass, in sorted key order so the output is deterministic.
    let mut both: Vec<&str> = base_keys.intersection(&peer_keys).copied().collect();
    both.sort_unstable();
    let mut items: Vec<DiffItem> = Vec::new();
    for key in &both {
        let base_comment = base_map.get(key).copied().unwrap_or("");
        let peer_comment = peer_map.get(key).copied().unwrap_or("");
        if base_comment != peer_comment {
            items.push(DiffItem::CommentDiff {
                key: (*key).to_string(),
                base_comment: base_comment.to_string(),
                peer_comment: peer_comment.to_string(),
            });
        }
    }

    // Consume the attached lines of keys present in both files, then diff the
    // leftover comments as a multiset so nothing is reported twice.
    let both_set: HashSet<&str> = both.into_iter().collect();
    let consumed_base: Vec<&str> = base_attached
        .iter()
        .filter(|c| both_set.contains(c.key.as_str()))
        .flat_map(|c| c.lines.iter().map(|s| s.as_str()))
        .collect();
    let consumed_peer: Vec<&str> = peer_attached
        .iter()
        .filter(|c| both_set.contains(c.key.as_str()))
        .flat_map(|c| c.lines.iter().map(|s| s.as_str()))
        .collect();

    let remaining_base = subtract_comments(&consumed_base, base_comments);
    let remaining_peer = subtract_comments(&consumed_peer, peer_comments);
    items.extend(multiset_diffs(&remaining_base, &remaining_peer));

    items
}

/// Remove the `consumed` lines (as a multiset) from `all`, preserving `all`'s
/// order. Consumed lines must be present in `all`; each occurrence removes one
/// matching entry.
fn subtract_comments(consumed: &[&str], all: &[String]) -> Vec<String> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for c in consumed {
        *counts.entry(c).or_insert(0) += 1;
    }
    let mut out = Vec::new();
    for item in all {
        match counts.get_mut(item.as_str()) {
            Some(n) if *n > 0 => *n -= 1,
            _ => out.push(item.clone()),
        }
    }
    out
}

/// Diff two comment pools as multisets, like the key-only diff: comments only
/// in the base pool become `CommentMissing` items (in base order), comments
/// only in the peer pool become `CommentExtra` items (in peer order). Comments
/// present in both cancel out. Repeated comments are counted, so a comment
/// appearing more often in one pool yields one item per excess occurrence.
fn multiset_diffs(base_comments: &[String], peer_comments: &[String]) -> Vec<DiffItem> {
    let mut base_counts: HashMap<&str, usize> = HashMap::new();
    for c in base_comments {
        *base_counts.entry(c).or_insert(0) += 1;
    }
    let mut peer_counts: HashMap<&str, usize> = HashMap::new();
    for c in peer_comments {
        *peer_counts.entry(c).or_insert(0) += 1;
    }

    let mut items: Vec<DiffItem> = Vec::new();

    // Missing comments, reported once per excess base occurrence in file order.
    let mut emitted: HashMap<&str, usize> = HashMap::new();
    for c in base_comments {
        let c = c.as_str();
        let missing = base_counts[c].saturating_sub(peer_counts.get(c).copied().unwrap_or(0));
        let seen = emitted.entry(c).or_insert(0);
        if *seen < missing {
            *seen += 1;
            items.push(DiffItem::CommentMissing {
                comment: c.to_string(),
            });
        }
    }

    // Extra comments, reported once per excess peer occurrence in file order.
    emitted.clear();
    for c in peer_comments {
        let c = c.as_str();
        let extra = peer_counts[c].saturating_sub(base_counts.get(c).copied().unwrap_or(0));
        let seen = emitted.entry(c).or_insert(0);
        if *seen < extra {
            *seen += 1;
            items.push(DiffItem::CommentExtra {
                comment: c.to_string(),
            });
        }
    }

    items
}

/// Build the ANSI-colored label for a per-key comment-diff line
/// (`- KEY # comment`).
///
/// The comment is shown after a `# ` separator in the `value` color; a side
/// without a comment renders as just `- KEY` / `+ KEY`.
fn comment_label(
    prefix: &str,
    key: &str,
    comment: &str,
    prefix_color: AnsiColor,
    colors: &ColorConfig,
    use_color: bool,
) -> String {
    if use_color {
        if comment.is_empty() {
            format!("{} {key}", color::colorize(prefix, prefix_color, use_color))
        } else {
            format!(
                "{} {key} # {}{}{}",
                color::colorize(prefix, prefix_color, use_color),
                colors.value.code(),
                comment,
                AnsiColor::Reset.code(),
            )
        }
    } else if comment.is_empty() {
        format!("{prefix} {key}")
    } else {
        format!("{prefix} {key} # {comment}")
    }
}

/// Build the ANSI-colored label for a free-comment diff line (`- # comment`).
fn free_comment_label(
    prefix: &str,
    comment: &str,
    prefix_color: AnsiColor,
    colors: &ColorConfig,
    use_color: bool,
) -> String {
    if use_color {
        format!(
            "{} # {}{}{}",
            color::colorize(prefix, prefix_color, use_color),
            colors.value.code(),
            comment,
            AnsiColor::Reset.code(),
        )
    } else {
        format!("{prefix} # {comment}")
    }
}

/// Build the base file's key order for `--reorder`: recognized keys in file
/// order, first occurrence wins.
fn base_key_order(pairs: &[ParsedPair]) -> Vec<String> {
    let mut seen: HashSet<&str> = HashSet::new();
    let mut order: Vec<String> = Vec::new();
    for p in pairs {
        if seen.insert(p.key.as_str()) {
            order.push(p.key.clone());
        }
    }
    order
}

/// Collect the peer targets for `--reorder`: same-kind files in the base
/// service, excluding the base file itself and `skip_files` matches.
fn reorder_targets(
    service: &crate::model::Service,
    base: &crate::model::EnvFile,
    skip_files: &HashSet<String>,
) -> Vec<crate::edit::Target> {
    let peers = peer_kinds(base.kind);
    let mut targets: Vec<crate::edit::Target> = Vec::new();
    for file in &service.files {
        if !peers.contains(&file.kind) {
            continue;
        }
        if file.display == base.display {
            continue;
        }
        // Peer files matching configured skip_files patterns are excluded.
        if context::file_is_skipped(file, &service.path, skip_files) {
            continue;
        }
        targets.push(crate::edit::Target {
            service: service.name.clone(),
            file: file.clone(),
        });
    }
    targets
}

/// Handle `nv compare`: compare a base file against peer files.
pub fn run(
    cli: &Cli,
    file_path: &str,
    compare_values: bool,
    compare_order: bool,
    compare_comments: bool,
    reorder: bool,
) -> Result<()> {
    // `--reorder` identifies the base service via `--service`; fail fast before
    // anything is read or written.
    if reorder && cli.services.is_empty() {
        bail!("--reorder requires --service to identify the base file's service.");
    }

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
            None,
            None,
            &ctx.config,
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

    // A base file listed under compare.skip_files cannot be compared. Show an
    // error with the reason and list available files of the same kind.
    let base_skip_files = context::build_skip_files(&ctx.config, &base_service.name, |cfg, svc| {
        cfg.compare_skip_files_for(svc)
    });
    if context::file_is_skipped(base_file, &base_service.path, &base_skip_files) {
        let tree = available_files_tree(
            &ctx.services,
            &service_filter,
            Some(base_kind),
            Some(file_path),
            &ctx.config,
            &ctx.colors(),
            color::should_use_color(),
        );
        let tree = if tree.is_empty() {
            tree
        } else {
            format!("Available {} files:\n{}", kind_label(base_kind), tree)
        };
        bail!(
            "file '{}' is excluded by compare.skip_files.\n{}",
            file_path,
            tree
        );
    }

    let base_content = fs::read_to_string(&base_file.path)
        .map_err(|e| anyhow::anyhow!("cannot read base file '{}': {}", base_file.display, e))?;
    let base_pairs = parse_pairs(&base_content, base_kind);

    // `--reorder` rewrites the base service's other same-kind files so their
    // keys follow the base file's order, then previews and applies via the
    // standard safe-write path.
    if reorder {
        let order = base_key_order(&base_pairs);
        let targets = reorder_targets(base_service, base_file, &base_skip_files);
        let changes = crate::edit::ChangeSet::reorder(&targets, &order)?;
        let use_color = color::should_use_color();
        let colors = ctx.colors();
        context::preview_and_apply(cli, &changes, &colors, use_color)?;
        return Ok(());
    }

    let base_comments = parser::parse_comments(&base_content);
    let base_attached = parser::parse_attached_comments(&base_content, base_kind);

    let peer_kinds = peer_kinds(base_kind);
    let mut comparisons: BTreeMap<String, BTreeMap<String, Vec<DiffItem>>> = BTreeMap::new();

    for (svc_idx, service) in ctx.services.iter().enumerate() {
        if !service_filter.is_empty() && !service_filter.iter().any(|n| n == &service.name) {
            continue;
        }

        let skip_files = context::build_skip_files(&ctx.config, &service.name, |cfg, svc| {
            cfg.compare_skip_files_for(svc)
        });

        for file in &service.files {
            if !peer_kinds.contains(&file.kind) {
                continue;
            }
            if svc_idx == base_svc_idx && file.display == base_file.display {
                continue;
            }

            // Skip peer files matching configured skip_files patterns.
            if context::file_is_skipped(file, &service.path, &skip_files) {
                continue;
            }

            let content = match fs::read_to_string(&file.path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let peer_pairs = parse_pairs(&content, file.kind);
            let diffs = if compare_comments {
                let peer_comments = parser::parse_comments(&content);
                let peer_attached = parser::parse_attached_comments(&content, file.kind);
                comment_diffs(
                    &base_pairs,
                    &peer_pairs,
                    &base_attached,
                    &peer_attached,
                    &base_comments,
                    &peer_comments,
                )
            } else {
                let mut d = diff_pairs(&base_pairs, &peer_pairs, compare_values);
                if compare_order {
                    d.extend(order_diffs(&base_pairs, &peer_pairs));
                }
                d
            };

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
/// Used when the requested base file path is not found or is excluded by
/// `compare.skip_files`, so the user can see valid paths at a glance. When
/// `service_filter` is non-empty, only those services' files are included.
/// When `kind_filter` is set, only files of that kind are listed. When
/// `exclude_path` is set, files with that display path are omitted (the
/// requested file is not "available"). Files matching a service's merged
/// `compare.skip_files` are omitted too, since they cannot be compared.
fn available_files_tree(
    services: &[crate::model::Service],
    service_filter: &[&str],
    kind_filter: Option<FileKind>,
    exclude_path: Option<&str>,
    config: &Option<crate::config::Config>,
    colors: &ColorConfig,
    use_color: bool,
) -> String {
    let trees: Vec<TreeService> = services
        .iter()
        .filter(|s| service_filter.is_empty() || service_filter.iter().any(|n| n == &s.name))
        .filter_map(|s| {
            let skip_files = context::build_skip_files(config, &s.name, |cfg, svc| {
                cfg.compare_skip_files_for(svc)
            });
            let files: Vec<TreeFile> = s
                .files
                .iter()
                .filter(|f| kind_filter.is_none_or(|k| f.kind == k))
                .filter(|f| exclude_path.is_none_or(|p| f.display != p))
                .filter(|f| !context::file_is_skipped(f, &s.path, &skip_files))
                .map(|f| TreeFile {
                    name: f.display.clone(),
                    count: 0,
                    items: Vec::new(),
                })
                .collect();
            if files.is_empty() {
                None
            } else {
                Some(TreeService {
                    name: s.name.clone(),
                    count: files.len(),
                    files,
                })
            }
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
                            DiffItem::OutOfOrder {
                                key,
                                base_pos,
                                peer_pos,
                            } => {
                                // Base side first (`-`), peer side (`+`), mirroring
                                // the value-diff pair format.
                                items.push(TreeItem {
                                    label: order_label(
                                        "-",
                                        key,
                                        *base_pos,
                                        colors.removed,
                                        use_color,
                                    ),
                                    color: colors.removed,
                                });
                                items.push(TreeItem {
                                    label: order_label(
                                        "+",
                                        key,
                                        *peer_pos,
                                        colors.added,
                                        use_color,
                                    ),
                                    color: colors.added,
                                });
                            }
                            DiffItem::CommentDiff {
                                key,
                                base_comment,
                                peer_comment,
                            } => {
                                items.push(TreeItem {
                                    label: comment_label(
                                        "-",
                                        key,
                                        base_comment,
                                        colors.removed,
                                        colors,
                                        use_color,
                                    ),
                                    color: colors.removed,
                                });
                                items.push(TreeItem {
                                    label: comment_label(
                                        "+",
                                        key,
                                        peer_comment,
                                        colors.added,
                                        colors,
                                        use_color,
                                    ),
                                    color: colors.added,
                                });
                            }
                            DiffItem::CommentMissing { comment } => {
                                items.push(TreeItem {
                                    label: free_comment_label(
                                        "-",
                                        comment,
                                        colors.removed,
                                        colors,
                                        use_color,
                                    ),
                                    color: colors.removed,
                                });
                            }
                            DiffItem::CommentExtra { comment } => {
                                items.push(TreeItem {
                                    label: free_comment_label(
                                        "+",
                                        comment,
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
    use std::collections::HashSet;
    use std::path::PathBuf;

    use crate::model::{EnvFile, FileKind};

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

    fn pair(key: &str, value: &str) -> ParsedPair {
        ParsedPair {
            key: key.into(),
            value: value.into(),
        }
    }

    #[test]
    fn test_order_diffs_same_order_is_empty() {
        let base = vec![pair("A", "1"), pair("B", "2"), pair("C", "3")];
        let peer = vec![pair("A", "1"), pair("B", "2"), pair("C", "3")];
        assert!(order_diffs(&base, &peer).is_empty());
    }

    #[test]
    fn test_order_diffs_swapped_pair() {
        // A B C D vs A C B D: B regresses (base #2, peer #3).
        let base = vec![
            pair("A", "1"),
            pair("B", "2"),
            pair("C", "3"),
            pair("D", "4"),
        ];
        let peer = vec![
            pair("A", "1"),
            pair("C", "3"),
            pair("B", "2"),
            pair("D", "4"),
        ];
        let diffs = order_diffs(&base, &peer);
        assert_eq!(diffs.len(), 1);
        match &diffs[0] {
            DiffItem::OutOfOrder {
                key,
                base_pos,
                peer_pos,
            } => {
                assert_eq!(key, "B");
                assert_eq!(*base_pos, 2);
                assert_eq!(*peer_pos, 3);
            }
            other => panic!("expected OutOfOrder, got {other:?}"),
        }
    }

    #[test]
    fn test_order_diffs_reversed() {
        // Fully reversed: every key after the first regresses.
        let base = vec![pair("A", "1"), pair("B", "2"), pair("C", "3")];
        let peer = vec![pair("C", "3"), pair("B", "2"), pair("A", "1")];
        let diffs = order_diffs(&base, &peer);
        let keys: Vec<&str> = diffs
            .iter()
            .map(|d| match d {
                DiffItem::OutOfOrder { key, .. } => key.as_str(),
                other => panic!("expected OutOfOrder, got {other:?}"),
            })
            .collect();
        assert_eq!(keys, vec!["B", "A"]);
    }

    #[test]
    fn test_order_diffs_ignores_missing_and_extra_keys() {
        // X is not in the base, Z is not in the peer — neither affects order.
        let base = vec![pair("A", "1"), pair("B", "2"), pair("Z", "9")];
        let peer = vec![pair("B", "2"), pair("X", "8"), pair("A", "1")];
        let diffs = order_diffs(&base, &peer);
        let keys: Vec<&str> = diffs
            .iter()
            .map(|d| match d {
                DiffItem::OutOfOrder { key, .. } => key.as_str(),
                other => panic!("expected OutOfOrder, got {other:?}"),
            })
            .collect();
        // B precedes A in the peer but not in the base → only A is reported.
        assert_eq!(keys, vec!["A"]);
    }

    #[test]
    fn test_order_diffs_duplicate_keys_use_first_occurrence() {
        let base = vec![pair("A", "1"), pair("B", "2"), pair("C", "3")];
        // B appears twice in the peer; the duplicate must not be re-reported.
        let peer = vec![
            pair("C", "3"),
            pair("B", "2"),
            pair("B", "4"),
            pair("A", "1"),
        ];
        let diffs = order_diffs(&base, &peer);
        let keys: Vec<&str> = diffs
            .iter()
            .map(|d| match d {
                DiffItem::OutOfOrder { key, .. } => key.as_str(),
                other => panic!("expected OutOfOrder, got {other:?}"),
            })
            .collect();
        assert_eq!(keys, vec!["B", "A"]);
    }

    #[test]
    fn test_order_label_no_color() {
        let label = order_label("-", "MY_KEY", 3, ColorConfig::default().removed, false);
        assert_eq!(label, "- MY_KEY (#3)");
    }

    #[test]
    fn test_order_label_with_color() {
        let label = order_label("+", "MY_KEY", 5, ColorConfig::default().added, true);
        assert!(label.starts_with("\x1b["));
        assert!(label.contains("+"));
        assert!(label.contains("MY_KEY (#5)"));
        assert!(label.contains("\x1b[0m"));
    }

    fn attached(key: &str, comment: &str, lines: &[&str]) -> parser::ParsedComment {
        parser::ParsedComment {
            key: key.into(),
            comment: comment.into(),
            lines: lines.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn comments(texts: &[&str]) -> Vec<String> {
        texts.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_comment_diffs_identical_attached_is_empty() {
        let base = vec![pair("A", "1"), pair("B", "2")];
        let peer = vec![pair("A", "1"), pair("B", "2")];
        let base_attached = vec![
            attached("A", "doc", &["doc"]),
            attached("B", "inline", &["inline"]),
        ];
        let peer_attached = base_attached.clone();
        let base_comments = comments(&["doc", "inline"]);
        let peer_comments = comments(&["doc", "inline"]);
        assert!(
            comment_diffs(
                &base,
                &peer,
                &base_attached,
                &peer_attached,
                &base_comments,
                &peer_comments,
            )
            .is_empty()
        );
    }

    #[test]
    fn test_comment_diffs_changed_attached_comment() {
        let base = vec![pair("A", "1")];
        let peer = vec![pair("A", "1")];
        let base_attached = vec![attached("A", "db doc", &["db doc"])];
        let peer_attached = vec![attached("A", "prod", &["prod"])];
        let base_comments = comments(&["db doc"]);
        let peer_comments = comments(&["prod"]);
        let diffs = comment_diffs(
            &base,
            &peer,
            &base_attached,
            &peer_attached,
            &base_comments,
            &peer_comments,
        );
        assert_eq!(diffs.len(), 1);
        match &diffs[0] {
            DiffItem::CommentDiff {
                key,
                base_comment,
                peer_comment,
            } => {
                assert_eq!(key, "A");
                assert_eq!(base_comment, "db doc");
                assert_eq!(peer_comment, "prod");
            }
            other => panic!("expected CommentDiff, got {other:?}"),
        }
    }

    #[test]
    fn test_comment_diffs_comment_added_on_one_side() {
        // Peer documents A, base does not → CommentDiff with an empty base side.
        let base = vec![pair("A", "1")];
        let peer = vec![pair("A", "1")];
        let base_attached = Vec::new();
        let peer_attached = vec![attached("A", "prod", &["prod"])];
        let base_comments = Vec::new();
        let peer_comments = comments(&["prod"]);
        let diffs = comment_diffs(
            &base,
            &peer,
            &base_attached,
            &peer_attached,
            &base_comments,
            &peer_comments,
        );
        assert_eq!(diffs.len(), 1);
        match &diffs[0] {
            DiffItem::CommentDiff {
                key,
                base_comment,
                peer_comment,
            } => {
                assert_eq!(key, "A");
                assert_eq!(base_comment, "");
                assert_eq!(peer_comment, "prod");
            }
            other => panic!("expected CommentDiff, got {other:?}"),
        }
    }

    #[test]
    fn test_comment_diffs_consumed_not_double_reported() {
        // The differing attached comment is shown per-key only; it must not
        // also appear as a `- #` / `+ #` free-comment item.
        let base = vec![pair("A", "1")];
        let peer = vec![pair("A", "1")];
        let base_attached = vec![attached("A", "db doc", &["db doc"])];
        let peer_attached = vec![attached("A", "prod", &["prod"])];
        let base_comments = comments(&["db doc"]);
        let peer_comments = comments(&["prod"]);
        let diffs = comment_diffs(
            &base,
            &peer,
            &base_attached,
            &peer_attached,
            &base_comments,
            &peer_comments,
        );
        assert_eq!(diffs.len(), 1);
        assert!(matches!(diffs[0], DiffItem::CommentDiff { .. }));
    }

    #[test]
    fn test_comment_diffs_duplicate_attached_lines_consumed() {
        // `# retry` above the key plus `# retry` inline both attach; both are
        // consumed, leaving no free-comment items.
        let base = vec![pair("A", "1")];
        let peer = vec![pair("A", "1")];
        let base_attached = vec![attached("A", "retry retry", &["retry", "retry"])];
        let peer_attached = Vec::new();
        let base_comments = comments(&["retry", "retry"]);
        let peer_comments = Vec::new();
        let diffs = comment_diffs(
            &base,
            &peer,
            &base_attached,
            &peer_attached,
            &base_comments,
            &peer_comments,
        );
        assert_eq!(diffs.len(), 1);
        assert!(matches!(diffs[0], DiffItem::CommentDiff { .. }));
    }

    #[test]
    fn test_comment_diffs_key_missing_in_peer_comment_is_free() {
        // B's comment cannot be compared per-key (B not in peer), so it flows
        // into the other-comments pass and is reported as a free comment.
        let base = vec![pair("A", "1"), pair("B", "2")];
        let peer = vec![pair("A", "1")];
        let base_attached = vec![attached("B", "only base", &["only base"])];
        let peer_attached = Vec::new();
        let base_comments = comments(&["only base"]);
        let peer_comments = Vec::new();
        let diffs = comment_diffs(
            &base,
            &peer,
            &base_attached,
            &peer_attached,
            &base_comments,
            &peer_comments,
        );
        assert_eq!(diffs.len(), 1);
        match &diffs[0] {
            DiffItem::CommentMissing { comment } => assert_eq!(comment, "only base"),
            other => panic!("expected CommentMissing, got {other:?}"),
        }
    }

    #[test]
    fn test_comment_diffs_free_comment_multiset() {
        // No attached comments; the free comment is diffed as a multiset.
        let base = vec![pair("A", "1")];
        let peer = vec![pair("A", "1")];
        let base_comments = comments(&["free note"]);
        let peer_comments = Vec::new();
        let diffs = comment_diffs(&base, &peer, &[], &[], &base_comments, &peer_comments);
        assert_eq!(diffs.len(), 1);
        match &diffs[0] {
            DiffItem::CommentMissing { comment } => assert_eq!(comment, "free note"),
            other => panic!("expected CommentMissing, got {other:?}"),
        }
    }

    #[test]
    fn test_comment_diffs_per_key_first_then_other_comments() {
        // The per-key pair is reported before the free-comment items.
        let base = vec![pair("A", "1")];
        let peer = vec![pair("A", "1")];
        let base_attached = vec![attached("A", "db doc", &["db doc"])];
        let peer_attached = Vec::new();
        let base_comments = comments(&["db doc", "free note"]);
        let peer_comments = Vec::new();
        let diffs = comment_diffs(
            &base,
            &peer,
            &base_attached,
            &peer_attached,
            &base_comments,
            &peer_comments,
        );
        assert_eq!(diffs.len(), 2);
        assert!(matches!(diffs[0], DiffItem::CommentDiff { .. }));
        match &diffs[1] {
            DiffItem::CommentMissing { comment } => assert_eq!(comment, "free note"),
            other => panic!("expected CommentMissing, got {other:?}"),
        }
    }

    #[test]
    fn test_comment_diffs_per_key_sorted_by_key() {
        let base = vec![pair("B", "1"), pair("A", "2")];
        let peer = vec![pair("B", "1"), pair("A", "2")];
        let base_attached = vec![
            attached("B", "b doc", &["b doc"]),
            attached("A", "a doc", &["a doc"]),
        ];
        let peer_attached = Vec::new();
        let base_comments = comments(&["b doc", "a doc"]);
        let peer_comments = Vec::new();
        let diffs = comment_diffs(
            &base,
            &peer,
            &base_attached,
            &peer_attached,
            &base_comments,
            &peer_comments,
        );
        // Keys differ from both sides (peer has no comments), reported in
        // sorted key order regardless of file order.
        let keys: Vec<&str> = diffs
            .iter()
            .map(|d| match d {
                DiffItem::CommentDiff { key, .. } => key.as_str(),
                other => panic!("expected CommentDiff, got {other:?}"),
            })
            .collect();
        assert_eq!(keys, vec!["A", "B"]);
    }

    #[test]
    fn test_comment_diffs_duplicates_counted() {
        // No attached keys; the multiset counting still applies to free
        // comments: two `retry` in base vs one in peer → one missing item.
        let base_comments = comments(&["retry", "retry", "other"]);
        let peer_comments = comments(&["retry", "other"]);
        let diffs = comment_diffs(&[], &[], &[], &[], &base_comments, &peer_comments);
        assert_eq!(diffs.len(), 1);
        match &diffs[0] {
            DiffItem::CommentMissing { comment } => assert_eq!(comment, "retry"),
            other => panic!("expected CommentMissing, got {other:?}"),
        }
    }

    #[test]
    fn test_comment_diffs_order_preserved() {
        let base_comments = comments(&["c1", "c2"]);
        let peer_comments = Vec::new();
        let diffs = comment_diffs(&[], &[], &[], &[], &base_comments, &peer_comments);
        let comments: Vec<&str> = diffs
            .iter()
            .map(|d| match d {
                DiffItem::CommentMissing { comment } => comment.as_str(),
                other => panic!("expected CommentMissing, got {other:?}"),
            })
            .collect();
        assert_eq!(comments, vec!["c1", "c2"]);
    }

    #[test]
    fn test_comment_diffs_no_comments_is_empty() {
        assert!(comment_diffs(&[], &[], &[], &[], &[], &[]).is_empty());
    }

    #[test]
    fn test_comment_label_no_color() {
        let colors = ColorConfig::default();
        let label = comment_label("-", "MY_KEY", "my note", colors.removed, &colors, false);
        assert_eq!(label, "- MY_KEY # my note");
    }

    #[test]
    fn test_comment_label_empty_renders_bare_key() {
        let colors = ColorConfig::default();
        let label = comment_label("+", "MY_KEY", "", colors.added, &colors, false);
        assert_eq!(label, "+ MY_KEY");
    }

    #[test]
    fn test_comment_label_with_color() {
        let colors = ColorConfig::default();
        let label = comment_label("+", "MY_KEY", "my note", colors.added, &colors, true);
        assert!(label.starts_with("\x1b["));
        assert!(label.contains("+"));
        assert!(label.contains("MY_KEY # "));
        assert!(label.contains("my note"));
        assert!(label.ends_with("\x1b[0m"));
    }

    #[test]
    fn test_free_comment_label_no_color() {
        let colors = ColorConfig::default();
        let label = free_comment_label(
            "-",
            "REDIS_ENTERPRISE_HOST=redis-enterprise",
            colors.removed,
            &colors,
            false,
        );
        assert_eq!(label, "- # REDIS_ENTERPRISE_HOST=redis-enterprise");
    }

    #[test]
    fn test_free_comment_label_with_color() {
        let colors = ColorConfig::default();
        let label = free_comment_label("+", "my doc", colors.added, &colors, true);
        assert!(label.starts_with("\x1b["));
        assert!(label.contains("+"));
        assert!(label.contains("# "));
        assert!(label.contains("my doc"));
        assert!(label.ends_with("\x1b[0m"));
    }

    fn skip_set(patterns: &[&str]) -> HashSet<String> {
        patterns.iter().map(|p| p.to_string()).collect()
    }

    #[test]
    fn test_matches_skip_pattern_exact_relative_path() {
        let skip = skip_set(&["docker/.env"]);
        assert!(context::matches_skip_pattern("docker/.env", ".env", &skip));
        assert!(!context::matches_skip_pattern(
            "docker/.env.example",
            ".env.example",
            &skip
        ));
    }

    #[test]
    fn test_matches_skip_pattern_file_name() {
        let skip = skip_set(&["custom.env"]);
        // A bare name matches any sub-path ending in that file name.
        assert!(context::matches_skip_pattern(
            "docker/custom.env",
            "custom.env",
            &skip
        ));
        assert!(!context::matches_skip_pattern(
            "docker/other.env",
            "other.env",
            &skip
        ));
    }

    #[test]
    fn test_matches_skip_pattern_glob_double_star() {
        let skip = skip_set(&["docker/**/*.env*"]);
        assert!(context::matches_skip_pattern(
            "docker/app/.env.example",
            ".env.example",
            &skip
        ));
        assert!(context::matches_skip_pattern("docker/.env", ".env", &skip));
        assert!(!context::matches_skip_pattern(
            "billing/.env",
            ".env",
            &skip
        ));
    }

    #[test]
    fn test_matches_skip_pattern_single_glob() {
        let skip = skip_set(&["*.test.env"]);
        assert!(context::matches_skip_pattern(
            "auth.test.env",
            "auth.test.env",
            &skip
        ));
        // The bare-name match also covers nested paths with the same file name.
        assert!(context::matches_skip_pattern(
            "docker/auth.test.env",
            "auth.test.env",
            &skip
        ));
        // But not a different file name.
        assert!(!context::matches_skip_pattern(
            "docker/app.env",
            "app.env",
            &skip
        ));
    }

    #[test]
    fn test_build_skip_files_empty_without_config() {
        let skip = context::build_skip_files(&None, "auth", |_, _| Vec::new());
        assert!(skip.is_empty());
    }

    #[test]
    fn test_available_files_tree_omits_skipped_files() {
        let config: crate::config::Config = serde_yaml::from_str(
            r#"
services_root: .
commands:
  compare:
    skip_files:
      - docker/.env
"#,
        )
        .unwrap();

        let root = PathBuf::from("/tmp/auth");
        let service = crate::model::Service {
            name: "auth".into(),
            path: root.clone(),
            files: vec![
                EnvFile {
                    kind: FileKind::Dotenv,
                    path: root.join(".env"),
                    display: ".env".into(),
                },
                EnvFile {
                    kind: FileKind::Dotenv,
                    path: root.join("docker/.env"),
                    display: "docker/.env".into(),
                },
            ],
        };

        let tree = available_files_tree(
            &[service],
            &[],
            None,
            None,
            &Some(config),
            &ColorConfig::default(),
            false,
        );
        assert!(tree.contains("── .env"), "expected .env listed:\n{tree}");
        assert!(
            !tree.contains("docker/.env"),
            "skipped file must not be listed:\n{tree}"
        );
    }

    #[test]
    fn test_available_files_tree_kind_filter_respects_skip_files() {
        let config: crate::config::Config = serde_yaml::from_str(
            r#"
services_root: .
commands:
  compare:
    skip_files:
      - deploy/secrets.yml
"#,
        )
        .unwrap();

        let root = PathBuf::from("/tmp/billing");
        let service = crate::model::Service {
            name: "billing".into(),
            path: root.clone(),
            files: vec![
                EnvFile {
                    kind: FileKind::ConfigMap,
                    path: root.join("deploy/configmap.yml"),
                    display: "deploy/configmap.yml".into(),
                },
                EnvFile {
                    kind: FileKind::Secret,
                    path: root.join("deploy/secrets.yml"),
                    display: "deploy/secrets.yml".into(),
                },
            ],
        };

        let tree = available_files_tree(
            &[service],
            &[],
            Some(FileKind::ConfigMap),
            None,
            &Some(config),
            &ColorConfig::default(),
            false,
        );
        assert!(
            tree.contains("deploy/configmap.yml"),
            "expected configmap listed:\n{tree}"
        );
        assert!(
            !tree.contains("secrets.yml"),
            "skipped secrets file must not be listed:\n{tree}"
        );
    }

    #[test]
    fn test_base_key_order_dedupes_first_occurrence_wins() {
        let pairs = vec![
            pair("A", "1"),
            pair("B", "2"),
            pair("A", "3"),
            pair("C", "4"),
            pair("B", "5"),
        ];
        assert_eq!(base_key_order(&pairs), vec!["A", "B", "C"]);
    }

    #[test]
    fn test_base_key_order_empty_base() {
        assert!(base_key_order(&[]).is_empty());
    }

    #[test]
    fn test_reorder_targets_same_kind_peers_in_service() {
        let root = PathBuf::from("/tmp/auth");
        let service = crate::model::Service {
            name: "auth".into(),
            path: root.clone(),
            files: vec![
                EnvFile {
                    kind: FileKind::Dotenv,
                    path: root.join(".env"),
                    display: ".env".into(),
                },
                EnvFile {
                    kind: FileKind::Dotenv,
                    path: root.join(".env.prod"),
                    display: ".env.prod".into(),
                },
                EnvFile {
                    kind: FileKind::DotenvExample,
                    path: root.join(".env.example"),
                    display: ".env.example".into(),
                },
                EnvFile {
                    kind: FileKind::ConfigMap,
                    path: root.join("configmap.yml"),
                    display: "configmap.yml".into(),
                },
            ],
        };
        let base = EnvFile {
            kind: FileKind::Dotenv,
            path: root.join(".env"),
            display: ".env".into(),
        };
        let targets = reorder_targets(&service, &base, &HashSet::new());
        let displays: Vec<&str> = targets.iter().map(|t| t.file.display.as_str()).collect();
        // Same-kind peers only, base file excluded, configmap untouched.
        assert_eq!(displays, vec![".env.prod", ".env.example"]);
    }

    #[test]
    fn test_reorder_targets_excludes_skip_files() {
        let root = PathBuf::from("/tmp/auth");
        let service = crate::model::Service {
            name: "auth".into(),
            path: root.clone(),
            files: vec![
                EnvFile {
                    kind: FileKind::Dotenv,
                    path: root.join(".env"),
                    display: ".env".into(),
                },
                EnvFile {
                    kind: FileKind::Dotenv,
                    path: root.join("docker/.env"),
                    display: "docker/.env".into(),
                },
            ],
        };
        let base = EnvFile {
            kind: FileKind::Dotenv,
            path: root.join(".env"),
            display: ".env".into(),
        };
        let skip = skip_set(&["docker/.env"]);
        let targets = reorder_targets(&service, &base, &skip);
        assert!(targets.is_empty(), "skipped peer must be excluded");
    }
}
