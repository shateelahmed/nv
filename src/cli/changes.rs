//! `nv changes` — list a service's configmap/secrets key changes between two
//! branches of its git repository.
//!
//! This module contains the command handler and the pure change-computation
//! core: branch diffs, cross-kind "moved" annotations, and markdown rendering.
//! The computation functions take parsed branch contents, so they are
//! unit-testable without touching git or the filesystem.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;

use anyhow::{Context, Result, bail};

use super::{Cli, context};
use crate::color::{self, AnsiColor, ColorConfig};
use crate::display::{self, Output, TreeFile, TreeItem, TreeService};
use crate::git;
use crate::model::{FileKind, Service};
use crate::parser::{self, ParsedPair};

/// A file and its content on both branches, the input to [`build_report`].
#[derive(Debug, Clone)]
struct FileInput {
    kind: FileKind,
    display: String,
    from_content: String,
    to_content: String,
}

/// How a key changed between the branches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChangeKind {
    /// Present on `--from`, absent on `--to`.
    Created,
    /// Present on both branches with a different value (configmap only).
    Updated,
    /// Present on `--to`, absent on `--from`.
    Deleted,
}

/// A single finalized change ready for rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Item {
    kind: FileKind,
    change: ChangeKind,
    key: String,
    /// Value shown to the user: the `--from` value for configmap items, or the
    /// plain-text value for a secret moved out of a configmap. `None` when
    /// redacted or not shown.
    value: Option<String>,
    /// The `--to` (old) value, for configmap Updated items.
    old_value: Option<String>,
    /// Annotation suffix, e.g. `(from secret)`; empty when none.
    note: String,
}

/// The finalized changes for one file.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FileReport {
    display: String,
    kind: FileKind,
    items: Vec<Item>,
}

/// Compute the created/updated/deleted items between two branches of one file.
///
/// Values are redacted for secrets (kept `None`); secrets never produce
/// Updated items. Items are grouped created → updated → deleted, each in
/// sorted key order, for deterministic output.
fn branch_diff(from: &[ParsedPair], to: &[ParsedPair], kind: FileKind) -> Vec<Item> {
    let from_map: BTreeMap<&str, &str> = from
        .iter()
        .map(|p| (p.key.as_str(), p.value.as_str()))
        .collect();
    let to_map: BTreeMap<&str, &str> = to
        .iter()
        .map(|p| (p.key.as_str(), p.value.as_str()))
        .collect();

    let mut items = Vec::new();
    for (key, value) in &from_map {
        if !to_map.contains_key(key) {
            items.push(Item {
                kind,
                change: ChangeKind::Created,
                key: (*key).to_string(),
                value: if kind == FileKind::ConfigMap {
                    Some((*value).to_string())
                } else {
                    None
                },
                old_value: None,
                note: String::new(),
            });
        }
    }
    if kind == FileKind::ConfigMap {
        for (key, value) in &from_map {
            if let Some(old) = to_map.get(key)
                && old != value
            {
                items.push(Item {
                    kind,
                    change: ChangeKind::Updated,
                    key: (*key).to_string(),
                    value: Some((*value).to_string()),
                    old_value: Some((*old).to_string()),
                    note: String::new(),
                });
            }
        }
    }
    for key in to_map.keys() {
        if !from_map.contains_key(key) {
            items.push(Item {
                kind,
                change: ChangeKind::Deleted,
                key: (*key).to_string(),
                value: None,
                old_value: None,
                note: String::new(),
            });
        }
    }
    items
}

/// The environment grouping key for a file: its parent directory relative to
/// the service root (e.g. `deploy/dev/kubernetes`). Configmap and secrets files
/// of one environment share a folder, so grouping by it keeps move detection
/// from crossing environments.
fn env_group(display: &str) -> String {
    match display.rsplit_once('/') {
        Some((parent, _)) => parent.to_string(),
        None => String::new(),
    }
}

/// Per-environment bookkeeping used for move detection and suppression.
#[derive(Default)]
struct GroupMeta {
    /// Keys Deleted from this group's secrets files.
    secrets_deleted: HashSet<String>,
    /// Keys Deleted from this group's configmap files.
    configmap_deleted: HashSet<String>,
    /// The `--to` configmap values by key for this group, first file wins.
    configmap_to_values: HashMap<String, String>,
    /// Keys present in this group's secrets files on the `--from` branch.
    secrets_from_keys: HashSet<String>,
    /// Keys present in this group's configmap files on the `--from` branch,
    /// with their values (first file wins).
    configmap_from_values: HashMap<String, String>,
}

/// Build the finalized, annotated report for a set of files.
///
/// Pass 1 diffs each file between the branches. Pass 2 gathers per-environment
/// key sets (grouped by the file's parent folder) so moves are detected only
/// within one environment. Pass 3 suppresses a configmap Deleted key that still
/// lives in the same environment's secrets on the `--from` branch — that is a
/// move, reported on the secrets side instead. Pass 4 applies the cross-kind
/// annotations: a configmap Created/Updated key annotated `(from secret)` when
/// the same key was Deleted from the environment's secrets files, and a secrets
/// Created key annotated `(from configmap)` (showing the plain-text value it
/// had in the `--to` branch's configmap) when the same key was Deleted from
/// the environment's configmap files.
fn build_report(inputs: &[FileInput]) -> Vec<FileReport> {
    // Pass 1: parse + diff each file between the branches.
    let mut files: Vec<FileReport> = inputs
        .iter()
        .map(|input| {
            let from_pairs = parser::parse(&input.from_content, input.kind);
            let to_pairs = parser::parse(&input.to_content, input.kind);
            FileReport {
                display: input.display.clone(),
                kind: input.kind,
                items: branch_diff(&from_pairs, &to_pairs, input.kind),
            }
        })
        .collect();

    // Pass 2: per-environment sets.
    let mut groups: BTreeMap<String, GroupMeta> = BTreeMap::new();
    for input in inputs {
        let meta = groups.entry(env_group(&input.display)).or_default();
        let from_pairs = parser::parse(&input.from_content, input.kind);
        let to_pairs = parser::parse(&input.to_content, input.kind);
        match input.kind {
            FileKind::Secret => {
                meta.secrets_from_keys
                    .extend(from_pairs.iter().map(|p| p.key.clone()));
            }
            FileKind::ConfigMap => {
                for pair in from_pairs {
                    meta.configmap_from_values
                        .entry(pair.key)
                        .or_insert(pair.value);
                }
                for pair in to_pairs {
                    meta.configmap_to_values
                        .entry(pair.key)
                        .or_insert(pair.value);
                }
            }
            _ => {}
        }
    }
    for file in &files {
        let meta = groups
            .get_mut(&env_group(&file.display))
            .expect("group exists");
        for item in &file.items {
            if item.change == ChangeKind::Deleted {
                match file.kind {
                    FileKind::Secret => {
                        meta.secrets_deleted.insert(item.key.clone());
                    }
                    FileKind::ConfigMap => {
                        meta.configmap_deleted.insert(item.key.clone());
                    }
                    _ => {}
                }
            }
        }
    }

    // Pass 3: suppress configmap Deleted keys that still exist in the same
    // group's secrets on the `--from` branch (a move, not a removal).
    for file in &mut files {
        if file.kind != FileKind::ConfigMap {
            continue;
        }
        let Some(secrets_from) = groups
            .get(&env_group(&file.display))
            .map(|meta| &meta.secrets_from_keys)
        else {
            continue;
        };
        file.items.retain(|item| {
            !(item.change == ChangeKind::Deleted && secrets_from.contains(&item.key))
        });
    }

    // Pass 4: cross-kind annotations within each environment. A key deleted
    // from secrets but still present in the configmap (unchanged value) is a
    // move into the configmap: it is reported as a configmap Updated item
    // annotated `(from secret)` even though its value did not change.
    for file in &mut files {
        let Some(meta) = groups.get(&env_group(&file.display)) else {
            continue;
        };
        for item in &mut file.items {
            match (file.kind, item.change) {
                (FileKind::ConfigMap, ChangeKind::Created | ChangeKind::Updated) => {
                    if meta.secrets_deleted.contains(&item.key) {
                        item.note = "(from secret)".to_string();
                    }
                }
                (FileKind::Secret, ChangeKind::Created)
                    if meta.configmap_deleted.contains(&item.key) =>
                {
                    if let Some(value) = meta.configmap_to_values.get(&item.key)
                        && has_real_value(value)
                    {
                        item.note = "(from configmap)".to_string();
                        item.value = Some(value.clone());
                    }
                }
                _ => {}
            }
        }
        if file.kind == FileKind::ConfigMap {
            for key in &meta.secrets_deleted {
                let Some(value) = meta.configmap_from_values.get(key) else {
                    continue;
                };
                if !file.items.iter().any(|i| &i.key == key) {
                    file.items.push(Item {
                        kind: FileKind::ConfigMap,
                        change: ChangeKind::Updated,
                        key: key.clone(),
                        value: Some(value.clone()),
                        old_value: None,
                        note: "(from secret)".to_string(),
                    });
                }
            }
        }
    }
    // Items must stay grouped Created → Updated → Deleted, each sorted by key,
    // for deterministic output.
    for file in &mut files {
        file.items.sort_by(|a, b| {
            change_rank(&a.change)
                .cmp(&change_rank(&b.change))
                .then_with(|| a.key.cmp(&b.key))
        });
    }
    files
}

/// Ordering of change kinds for display: Created, then Updated, then Deleted.
fn change_rank(change: &ChangeKind) -> u8 {
    match change {
        ChangeKind::Created => 0,
        ChangeKind::Updated => 1,
        ChangeKind::Deleted => 2,
    }
}

/// The human-readable section label for a file kind in the markdown report.
fn kind_label(kind: FileKind) -> &'static str {
    match kind {
        FileKind::ConfigMap => "configmap",
        FileKind::Secret => "secrets",
        _ => "other",
    }
}

/// Replace embedded newlines so multiline values stay on one output line.
fn escape_newlines(value: &str) -> String {
    value.replace('\n', "\\n")
}

/// True when a configmap value is a real value rather than empty or a redacted
/// placeholder (`xxxx…`), so a moved secrets key can show it. Placeholders are
/// never surfaced as if they were real secret values.
fn has_real_value(value: &str) -> bool {
    let trimmed = value.trim();
    !trimmed.is_empty() && !trimmed.chars().all(|c| c == 'x' || c == 'X')
}

/// The annotation suffix appended to an item label, e.g. ` (from secret)`.
fn note_suffix(note: &str) -> String {
    if note.is_empty() {
        String::new()
    } else {
        format!(" {note}")
    }
}

/// The value portion of a created/updated label (with annotations), or an
/// empty string when the value is redacted.
fn value_part(item: &Item) -> String {
    let Some(value) = &item.value else {
        return String::new();
    };
    format!("{}{}", escape_newlines(value), note_suffix(&item.note))
}

/// Render one item as a markdown bullet (without the leading `- `).
///
/// Configmap bullets always carry the key's value, even when it is empty, so
/// every configmap line reads `KEY: value`. Secret bullets render bare unless
/// a key moved out of a configmap with a real value to show.
fn markdown_label(item: &Item) -> String {
    match item.change {
        ChangeKind::Created | ChangeKind::Updated => match item.kind {
            // Configmap bullets always carry the key's value, even when it is
            // empty (rendered as `KEY:`), so every configmap line reads
            // `KEY: value`.
            FileKind::ConfigMap => {
                let value = item
                    .value
                    .as_deref()
                    .map(escape_newlines)
                    .unwrap_or_default();
                let body = if value.is_empty() {
                    item.note.clone()
                } else if item.note.is_empty() {
                    value
                } else {
                    format!("{value} {}", item.note)
                };
                if body.is_empty() {
                    format!("{}:", item.key)
                } else {
                    format!("{}: {body}", item.key)
                }
            }
            _ => {
                let part = value_part(item);
                if part.is_empty() {
                    item.key.clone()
                } else {
                    format!("{}: {part}", item.key)
                }
            }
        },
        ChangeKind::Deleted => item.key.clone(),
    }
}

/// Folder names that are structural containers rather than environment names,
/// so `deploy/dev/kubernetes` yields the environment `dev`.
const ENV_WRAPPER_DIRS: &[&str] = &[
    "deploy",
    "kubernetes",
    "k8s",
    "manifests",
    "config",
    "configs",
    "configmap",
    "configmaps",
    "secret",
    "secrets",
    "env",
    "environments",
    "environment",
];

/// Extract a short environment name from a file's parent path, e.g. `dev` from
/// `deploy/dev/kubernetes`: the first path segment that is not a container
/// folder. `None` when there is no parent or every segment is a container —
/// the caller falls back to the branch name.
fn env_name(group: &str) -> Option<String> {
    group
        .split('/')
        .find(|seg| !seg.is_empty() && !ENV_WRAPPER_DIRS.contains(seg))
        .map(str::to_string)
}

/// The `<strong>` subsections rendered for a file kind, in order. Created and
/// Updated are merged into a single "Updated" section; secrets use "Updated"
/// for their created items (secrets never produce true value updates).
fn kind_sections(kind: FileKind) -> &'static [(ChangeKind, &'static str)] {
    match kind {
        FileKind::ConfigMap => &[
            (ChangeKind::Created, "Updated"),
            (ChangeKind::Updated, "Updated"),
            (ChangeKind::Deleted, "Deleted"),
        ],
        _ => &[
            (ChangeKind::Created, "Updated"),
            (ChangeKind::Deleted, "Deleted"),
        ],
    }
}

/// The `(No changes)` marker rendered in green next to an empty label.
fn no_changes_marker() -> String {
    color::colorize("(No changes)", AnsiColor::Green, true).to_string()
}

/// Render one `<strong>` subsection: the label, then the `(No changes)` marker
/// in green when the section is empty, then the item bullets.
fn render_subsection(label: &str, items: &[&Item]) -> String {
    let mut out = format!("<strong>{label}</strong>");
    if items.is_empty() {
        out.push(' ');
        out.push_str(&no_changes_marker());
    }
    out.push_str("\n\n");
    for item in items {
        out.push_str(&format!("- {}\n", markdown_label(item)));
    }
    out.trim_end().to_string()
}

/// Render one environment's block of the markdown report: `## <env>`, then a
/// `### configmap` / `### secrets` section per kind with a `---` rule and
/// `<strong>Updated</strong>` / `<strong>Deleted</strong>` subsections. Every
/// label is rendered even when empty, marked `(No changes)` in green: an
/// environment with no changes renders just its header, a kind with no changes
/// renders just its `###` label, and an empty subsection still shows its
/// `<strong>` label.
fn render_env_section(env: &str, files: &[&FileReport]) -> String {
    if files.iter().map(|f| f.items.len()).sum::<usize>() == 0 {
        return format!("## {env} {}", no_changes_marker());
    }
    let mut blocks: Vec<String> = Vec::new();
    for kind in [FileKind::ConfigMap, FileKind::Secret] {
        let items: Vec<&Item> = files
            .iter()
            .filter(|f| f.kind == kind)
            .flat_map(|f| &f.items)
            .collect();
        if items.is_empty() {
            blocks.push(format!("### {} {}", kind_label(kind), no_changes_marker()));
            continue;
        }
        let mut subsections: Vec<(&str, Vec<&Item>)> = Vec::new();
        for (is_change, label) in kind_sections(kind) {
            let section: Vec<&Item> = items
                .iter()
                .filter(|i| i.change == *is_change)
                .copied()
                .collect();
            match subsections.iter_mut().find(|(l, _)| *l == *label) {
                Some((_, existing)) => existing.extend(section),
                None => subsections.push((label, section)),
            }
        }
        for (_, section) in &mut subsections {
            section.sort_by(|a, b| a.key.cmp(&b.key));
        }
        let mut out = format!("### {}\n---\n", kind_label(kind));
        for (label, section) in subsections {
            out.push_str(&render_subsection(label, &section));
            out.push_str("\n\n");
        }
        blocks.push(out.trim_end().to_string());
    }
    let body = blocks.join("\n\n");
    format!("## {env}\n\n{body}")
}

/// Render the markdown report: a `# Comparing branches: <from> → <to>` header,
/// then one `## <env>` section per environment (sorted by name, e.g. `dev`,
/// `prod`, `qa1`), including environments with no changes. Files outside any
/// environment folder fall back to the `--from` branch name as their section
/// title.
fn render_markdown(files: &[FileReport], from_branch: &str, to_branch: &str) -> String {
    let mut envs: BTreeMap<String, Vec<&FileReport>> = BTreeMap::new();
    for file in files {
        let title = env_name(&env_group(&file.display)).unwrap_or_else(|| from_branch.to_string());
        envs.entry(title).or_default().push(file);
    }
    let mut sections: Vec<String> = Vec::new();
    for (env, env_files) in envs {
        sections.push(render_env_section(&env, &env_files));
    }
    let mut out = format!("# Comparing branches: {from_branch} → {to_branch}\n\n");
    out.push_str(&sections.join("\n\n"));
    while out.ends_with('\n') {
        out.pop();
    }
    out.push('\n');
    out
}

/// The escaped old value for an Updated item's `- KEY = old` line (empty when
/// there is none).
fn old_value_part(item: &Item) -> String {
    item.old_value
        .as_deref()
        .map(escape_newlines)
        .unwrap_or_default()
}

/// Build one terminal tree item label: `+ KEY = value (note)` (or `- KEY` /
/// `- KEY = old`). The value portion, when present, is shown after the `=`
/// separator and colored with `colors.value`.
fn tree_label(
    prefix: &str,
    key: &str,
    value: &str,
    prefix_color: AnsiColor,
    colors: &ColorConfig,
    use_color: bool,
) -> String {
    let head = if use_color {
        format!("{} {key}", color::colorize(prefix, prefix_color, use_color))
    } else {
        format!("{prefix} {key}")
    };
    if value.is_empty() {
        head
    } else if use_color {
        format!(
            "{head} = {}{}{}",
            colors.value.code(),
            value,
            AnsiColor::Reset.code(),
        )
    } else {
        format!("{head} = {value}")
    }
}

/// The terminal tree items for one file's report, in the standard `+`/`-`
/// diff conventions: Created as `+ KEY = value`, Deleted as `- KEY`, and
/// Updated as a `- KEY = old` / `+ KEY = new` pair.
fn file_tree_items(file: &FileReport, colors: &ColorConfig, use_color: bool) -> Vec<TreeItem> {
    let mut items = Vec::new();
    for item in &file.items {
        match item.change {
            ChangeKind::Deleted => {
                items.push(TreeItem {
                    label: tree_label("-", &item.key, "", colors.removed, colors, use_color),
                    color: colors.removed,
                });
            }
            ChangeKind::Created => {
                items.push(TreeItem {
                    label: tree_label(
                        "+",
                        &item.key,
                        &value_part(item),
                        colors.added,
                        colors,
                        use_color,
                    ),
                    color: colors.added,
                });
            }
            ChangeKind::Updated => {
                items.push(TreeItem {
                    label: tree_label(
                        "-",
                        &item.key,
                        &old_value_part(item),
                        colors.removed,
                        colors,
                        use_color,
                    ),
                    color: colors.removed,
                });
                items.push(TreeItem {
                    label: tree_label(
                        "+",
                        &item.key,
                        &value_part(item),
                        colors.added,
                        colors,
                        use_color,
                    ),
                    color: colors.added,
                });
            }
        }
    }
    items
}

/// Render a report as the standard service/file/tree terminal output.
fn render_report(service_name: &str, files: &[FileReport], colors: &ColorConfig, use_color: bool) {
    let tree_files: Vec<TreeFile> = files
        .iter()
        .map(|file| TreeFile {
            name: file.display.clone(),
            count: file.items.len(),
            items: file_tree_items(file, colors, use_color),
        })
        .collect();
    let count: usize = tree_files.iter().map(|f| f.count).sum();
    let services = vec![TreeService {
        name: service_name.to_string(),
        count,
        files: tree_files,
    }];
    let mut out = Output::Stdout;
    display::render_tree(&services, colors, use_color, true, &mut out);
}

/// Resolve the service named by exactly one `--service` flag, erroring before
/// any git work otherwise.
fn selected_service<'a>(cli: &Cli, services: &'a [Service]) -> Result<&'a Service> {
    if cli.all || cli.services.len() != 1 {
        bail!("changes requires exactly one --service to select a single service.");
    }
    let name = &cli.services[0];
    services
        .iter()
        .find(|s| &s.name == name)
        .ok_or_else(|| anyhow::anyhow!("service '{name}' not found"))
}

/// True when a file's display path (relative to the service root) contains a
/// path segment equal to the environment name, e.g. `deploy/dev/kubernetes/...`
/// for `dev`.
fn in_environment(display: &str, env: &str) -> bool {
    display.split('/').any(|segment| segment == env)
}

/// Handle `nv changes --service NAME --from BRANCH [--to BRANCH]
/// [--environment ENV]`.
pub fn run(cli: &Cli, from: &str, to: Option<&str>, environment: Option<&str>) -> Result<()> {
    let ctx = context::resolve(cli)?;
    context::print_banner(ctx.source);

    let service = selected_service(cli, &ctx.services)?;

    // Effective baseline: explicit --to, else the service's configured master
    // branch, else `master`.
    let to_branch = match to {
        Some(b) => b.to_string(),
        None => ctx
            .config
            .as_ref()
            .and_then(|cfg| cfg.changes_master_branch_for(&service.name))
            .map(str::to_string)
            .unwrap_or_else(|| "master".to_string()),
    };

    // Effective environment filter: --environment, else the service's
    // configured `commands.changes.environment`, else all environments.
    let env_filter = match environment {
        Some(env) => Some(env.to_string()),
        None => ctx
            .config
            .as_ref()
            .and_then(|cfg| cfg.changes_environment_for(&service.name))
            .map(str::to_string),
    };

    // Validate the repository and both branches up front, with clear errors,
    // before scanning anything.
    let repo = git::repo_root(&service.path).context("service is not inside a git repository")?;
    let repo = repo.canonicalize().unwrap_or(repo);
    if !git::branch_exists(&repo, from)? {
        bail!(
            "branch '{from}' not found in the repository for service '{}'.",
            service.name
        );
    }
    if !git::branch_exists(&repo, &to_branch)? {
        bail!(
            "branch '{to_branch}' not found in the repository for service '{}'.",
            service.name
        );
    }

    let skip_files = context::build_skip_files(&ctx.config, &service.name, |cfg, svc| {
        cfg.changes_skip_files_for(svc)
    });

    // Collect each configmap/secrets file's content at both branches. A file
    // missing on a branch is treated as empty.
    let mut inputs: Vec<FileInput> = Vec::new();
    for file in &service.files {
        if !matches!(file.kind, FileKind::ConfigMap | FileKind::Secret) {
            continue;
        }
        if context::file_is_skipped(file, &service.path, &skip_files) {
            continue;
        }
        if let Some(env) = &env_filter
            && !in_environment(&file.display, env)
        {
            continue;
        }
        let file_path = file
            .path
            .canonicalize()
            .unwrap_or_else(|_| file.path.clone());
        let rel =
            git::rel_to_repo(&repo, &file_path).context("file is outside the git repository")?;
        let from_content = git::read_file_at(&repo, from, &rel)?.unwrap_or_default();
        let to_content = git::read_file_at(&repo, &to_branch, &rel)?.unwrap_or_default();
        inputs.push(FileInput {
            kind: file.kind,
            display: file.display.clone(),
            from_content,
            to_content,
        });
    }
    if let Some(env) = &env_filter
        && inputs.is_empty()
    {
        bail!(
            "environment '{env}' has no configmap/secrets files for service '{}'.",
            service.name
        );
    }

    let reports = build_report(&inputs);
    let total: usize = reports.iter().map(|r| r.items.len()).sum();
    if total == 0 {
        eprintln!("No changes found.");
        return Ok(());
    }

    let use_color = color::should_use_color();
    let colors = ctx.colors();
    render_report(&service.name, &reports, &colors, use_color);
    eprintln!(
        "\n{} change(s) found between '{from}' and '{to_branch}'.",
        total
    );

    if cli.dry_run {
        eprintln!("Dry run: no markdown file written.");
        return Ok(());
    }

    // Offer a markdown report, defaulting the file name and writing into the
    // service's own root folder.
    if !context::confirm("Generate a markdown list of changes?", false)? {
        return Ok(());
    }

    let default_name = format!("{}-env-changes.md", service.name);
    let file_name: String = dialoguer::Input::new()
        .with_prompt("File name")
        .default(default_name)
        .interact()?;
    write_markdown_file(&service.path, &file_name, from, &to_branch, &reports)?;
    Ok(())
}

/// Write the markdown report as `file_name` inside `service_root` and report
/// the written path. Split out of [`run`] so the write itself is unit-testable
/// without going through the interactive prompts.
fn write_markdown_file(
    service_root: &std::path::Path,
    file_name: &str,
    from: &str,
    to: &str,
    reports: &[FileReport],
) -> Result<std::path::PathBuf> {
    let path = service_root.join(file_name);
    fs::write(&path, render_markdown(reports, from, to))?;
    eprintln!("Wrote {}.", path.display());
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(key: &str, value: &str) -> ParsedPair {
        ParsedPair {
            key: key.into(),
            value: value.into(),
        }
    }

    #[test]
    fn branch_diff_configmap_created_updated_deleted() {
        let from = vec![pair("B", "2"), pair("A", "1"), pair("C", "3")];
        let to = vec![pair("B", "2"), pair("A", "9"), pair("D", "4")];
        let items = branch_diff(&from, &to, FileKind::ConfigMap);
        let kinds: Vec<ChangeKind> = items.iter().map(|i| i.change).collect();
        assert_eq!(
            kinds,
            vec![
                ChangeKind::Created,
                ChangeKind::Updated,
                ChangeKind::Deleted,
            ]
        );
        let created = items
            .iter()
            .find(|i| i.change == ChangeKind::Created)
            .unwrap();
        assert_eq!(created.key, "C");
        assert_eq!(created.value.as_deref(), Some("3"));
        let updated = items
            .iter()
            .find(|i| i.change == ChangeKind::Updated)
            .unwrap();
        assert_eq!(updated.key, "A");
        assert_eq!(updated.old_value.as_deref(), Some("9"));
        assert_eq!(updated.value.as_deref(), Some("1"));
        let deleted = items
            .iter()
            .find(|i| i.change == ChangeKind::Deleted)
            .unwrap();
        assert_eq!(deleted.key, "D");
        assert_eq!(deleted.value, None);
    }

    #[test]
    fn branch_diff_secrets_redacts_values_and_no_updated() {
        // A differs between the branches but must not produce an Updated item.
        let from = vec![pair("A", "v1"), pair("B", "x")];
        let to = vec![pair("A", "v2"), pair("C", "y")];
        let items = branch_diff(&from, &to, FileKind::Secret);
        assert!(
            !items.iter().any(|i| i.change == ChangeKind::Updated),
            "secrets must not produce Updated items"
        );
        let created = items
            .iter()
            .find(|i| i.change == ChangeKind::Created)
            .unwrap();
        assert_eq!(created.key, "B");
        assert_eq!(created.value, None, "secret values are redacted");
        let deleted = items
            .iter()
            .find(|i| i.change == ChangeKind::Deleted)
            .unwrap();
        assert_eq!(deleted.key, "C");
    }

    fn input(kind: FileKind, display: &str, from: &str, to: &str) -> FileInput {
        FileInput {
            kind,
            display: display.into(),
            from_content: from.into(),
            to_content: to.into(),
        }
    }

    #[test]
    fn build_report_annotates_moves_between_kinds() {
        let inputs = vec![
            input(
                FileKind::ConfigMap,
                "configmap.yml",
                "moved_from_secrets: newval\nupdated_cm: new\ncreated_cm: value\n",
                "moved_from_configmap: plainvalue\nupdated_cm: old\ngone_cm: x\n",
            ),
            input(
                FileKind::Secret,
                "secrets.yml",
                "moved_from_configmap: secretval\ncreated_s: s\n",
                "moved_from_secrets: old\ngone_s: g\n",
            ),
        ];
        let files = build_report(&inputs);

        let cm = &files[0];
        let moved_to_cm = cm
            .items
            .iter()
            .find(|i| i.key == "moved_from_secrets" && i.change == ChangeKind::Created)
            .unwrap();
        assert_eq!(moved_to_cm.note, "(from secret)");
        assert_eq!(moved_to_cm.value.as_deref(), Some("newval"));

        let plain = cm.items.iter().find(|i| i.key == "created_cm").unwrap();
        assert_eq!(plain.note, "", "unrelated keys get no annotation");

        let sec = &files[1];
        let moved_to_secrets = sec
            .items
            .iter()
            .find(|i| i.key == "moved_from_configmap" && i.change == ChangeKind::Created)
            .unwrap();
        assert_eq!(moved_to_secrets.note, "(from configmap)");
        assert_eq!(
            moved_to_secrets.value.as_deref(),
            Some("plainvalue"),
            "plain text value comes from the --to branch configmap"
        );

        let redacted = sec.items.iter().find(|i| i.key == "created_s").unwrap();
        assert_eq!(redacted.value, None);
        assert_eq!(redacted.note, "");
    }

    #[test]
    fn build_report_skips_placeholder_configmap_moves() {
        // A secrets Created key that moved out of a configmap whose baseline
        // value is a redacted placeholder (`xxxx…`) must not show the value or
        // the `(from configmap)` annotation, since there is no real value to
        // surface. A real baseline value still annotates as before.
        let inputs = vec![
            input(
                FileKind::ConfigMap,
                "configmap.yml",
                "other_cm: new\n",
                "PLACEHOLDER_MOVED: xxxxxxxxxxxxxxxxx\nREAL_MOVED: realvalue\nother_cm: old\n",
            ),
            input(
                FileKind::Secret,
                "secrets.yml",
                "PLACEHOLDER_MOVED: placeholder-secret\nREAL_MOVED: real-secret\n",
                "other_s: g\n",
            ),
        ];
        let files = build_report(&inputs);
        let sec = &files[1];

        let placeholder = sec
            .items
            .iter()
            .find(|i| i.key == "PLACEHOLDER_MOVED" && i.change == ChangeKind::Created)
            .unwrap();
        assert_eq!(
            placeholder.note, "",
            "placeholder has no real value to show"
        );
        assert_eq!(placeholder.value, None);

        let real = sec
            .items
            .iter()
            .find(|i| i.key == "REAL_MOVED" && i.change == ChangeKind::Created)
            .unwrap();
        assert_eq!(real.note, "(from configmap)");
        assert_eq!(real.value.as_deref(), Some("realvalue"));
    }

    #[test]
    fn build_report_lists_unchanged_value_move_from_secrets() {
        // MOVED_IN is deleted from secrets and already present in the configmap
        // with the same value on both branches: it is reported as a configmap
        // Updated item annotated `(from secret)` even though its value did not
        // change.
        let inputs = vec![
            input(
                FileKind::ConfigMap,
                "configmap.yml",
                "MOVED_IN: same\nKEPT: same\n",
                "MOVED_IN: same\nKEPT: same\n",
            ),
            input(
                FileKind::Secret,
                "secrets.yml",
                "KEPT: s\n",
                "MOVED_IN: old\nKEPT: s\n",
            ),
        ];
        let files = build_report(&inputs);
        let cm = &files[0];
        let moved = cm.items.iter().find(|i| i.key == "MOVED_IN").unwrap();
        assert_eq!(moved.change, ChangeKind::Updated);
        assert_eq!(moved.note, "(from secret)");
        assert_eq!(moved.value.as_deref(), Some("same"));
        assert_eq!(moved.old_value, None);
        assert!(
            !cm.items.iter().any(|i| i.key == "KEPT"),
            "an unchanged, never-moved key stays omitted"
        );
    }

    #[test]
    fn markdown_matches_spec_layout() {
        let files = vec![
            FileReport {
                display: "configmap.yml".into(),
                kind: FileKind::ConfigMap,
                items: vec![
                    Item {
                        kind: FileKind::ConfigMap,
                        change: ChangeKind::Created,
                        key: "key".into(),
                        value: Some("value".into()),
                        old_value: None,
                        note: String::new(),
                    },
                    Item {
                        kind: FileKind::ConfigMap,
                        change: ChangeKind::Created,
                        key: "key2".into(),
                        value: Some("value2".into()),
                        old_value: None,
                        note: "(from secret)".into(),
                    },
                    Item {
                        kind: FileKind::ConfigMap,
                        change: ChangeKind::Updated,
                        key: "key".into(),
                        value: Some("value".into()),
                        old_value: Some("oldvalue".into()),
                        note: String::new(),
                    },
                    Item {
                        kind: FileKind::ConfigMap,
                        change: ChangeKind::Deleted,
                        key: "Key".into(),
                        value: None,
                        old_value: None,
                        note: String::new(),
                    },
                ],
            },
            FileReport {
                display: "secrets.yml".into(),
                kind: FileKind::Secret,
                items: vec![
                    Item {
                        kind: FileKind::Secret,
                        change: ChangeKind::Created,
                        key: "key".into(),
                        value: None,
                        old_value: None,
                        note: String::new(),
                    },
                    Item {
                        kind: FileKind::Secret,
                        change: ChangeKind::Created,
                        key: "key2".into(),
                        value: Some("value2".into()),
                        old_value: None,
                        note: "(from configmap)".into(),
                    },
                    Item {
                        kind: FileKind::Secret,
                        change: ChangeKind::Deleted,
                        key: "Key".into(),
                        value: None,
                        old_value: None,
                        note: String::new(),
                    },
                ],
            },
        ];
        let expected = "\
# Comparing branches: dev → master

## dev

### configmap
---
<strong>Updated</strong>

- key: value
- key: value
- key2: value2 (from secret)

<strong>Deleted</strong>

- Key

### secrets
---
<strong>Updated</strong>

- key
- key2: value2 (from configmap)

<strong>Deleted</strong>

- Key
";
        assert_eq!(render_markdown(&files, "dev", "master"), expected);
    }

    #[test]
    fn markdown_marks_empty_labels_no_changes() {
        let files = vec![FileReport {
            display: "secrets.yml".into(),
            kind: FileKind::Secret,
            items: vec![Item {
                kind: FileKind::Secret,
                change: ChangeKind::Deleted,
                key: "OLD".into(),
                value: None,
                old_value: None,
                note: String::new(),
            }],
        }];
        // Empty subsections, empty kinds, and the configmap kind each render
        // their label followed by `(No changes)` in green.
        let expected = "\
# Comparing branches: feature/x → master

## feature/x

### configmap \x1b[32m(No changes)\x1b[0m

### secrets
---
<strong>Updated</strong> \x1b[32m(No changes)\x1b[0m

<strong>Deleted</strong>

- OLD
";
        assert_eq!(render_markdown(&files, "feature/x", "master"), expected);
    }

    #[test]
    fn markdown_environment_with_no_changes() {
        // A configmap/secrets file with no diff still contributes its
        // environment, marked `(No changes)`.
        let files = vec![FileReport {
            display: "deploy/uat1/kubernetes/configmap.yml".into(),
            kind: FileKind::ConfigMap,
            items: vec![],
        }];
        let expected = "\
# Comparing branches: feature/x → master

## uat1 \x1b[32m(No changes)\x1b[0m
";
        assert_eq!(render_markdown(&files, "feature/x", "master"), expected);
    }

    fn item(kind: FileKind, change: ChangeKind, key: &str) -> Item {
        Item {
            kind,
            change,
            key: key.into(),
            value: None,
            old_value: None,
            note: String::new(),
        }
    }

    #[test]
    fn markdown_segments_by_environment() {
        let files = vec![
            FileReport {
                display: "deploy/qa1/kubernetes/configmap.yml".into(),
                kind: FileKind::ConfigMap,
                items: vec![item(FileKind::ConfigMap, ChangeKind::Deleted, "QA1_GONE")],
            },
            FileReport {
                display: "deploy/dev/kubernetes/configmap.yml".into(),
                kind: FileKind::ConfigMap,
                items: vec![Item {
                    kind: FileKind::ConfigMap,
                    change: ChangeKind::Created,
                    key: "DEV_NEW".into(),
                    value: Some("newvalue".into()),
                    old_value: None,
                    note: String::new(),
                }],
            },
            FileReport {
                display: "deploy/dev/kubernetes/secrets.yml".into(),
                kind: FileKind::Secret,
                items: vec![item(FileKind::Secret, ChangeKind::Deleted, "DEV_OLD")],
            },
        ];
        let md = render_markdown(&files, "feature/x", "master");
        // Environments are sorted by name; dev comes before qa1, and each
        // environment gets its own `## <env>` block with kind subsections.
        let expected = "\
# Comparing branches: feature/x → master

## dev

### configmap
---
<strong>Updated</strong>

- DEV_NEW: newvalue

<strong>Deleted</strong> \x1b[32m(No changes)\x1b[0m

### secrets
---
<strong>Updated</strong> \x1b[32m(No changes)\x1b[0m

<strong>Deleted</strong>

- DEV_OLD

## qa1

### configmap
---
<strong>Updated</strong> \x1b[32m(No changes)\x1b[0m

<strong>Deleted</strong>

- QA1_GONE

### secrets \x1b[32m(No changes)\x1b[0m
";
        assert_eq!(md, expected);
    }

    #[test]
    fn markdown_environment_falls_back_to_branch_for_root_files() {
        let files = vec![FileReport {
            display: "configmap.yml".into(),
            kind: FileKind::ConfigMap,
            items: vec![item(FileKind::ConfigMap, ChangeKind::Created, "NEW")],
        }];
        let md = render_markdown(&files, "feature/x", "master");
        assert!(
            md.starts_with("# Comparing branches: feature/x → master\n\n## feature/x\n"),
            "got:\n{md}"
        );
    }

    #[test]
    fn write_markdown_file_lands_in_service_root() {
        let tmp = tempfile::tempdir().unwrap();
        let service_root = tmp.path().join("auth");
        fs::create_dir_all(&service_root).unwrap();
        let reports = vec![FileReport {
            display: "configmap.yml".into(),
            kind: FileKind::ConfigMap,
            items: vec![Item {
                kind: FileKind::ConfigMap,
                change: ChangeKind::Created,
                key: "NEW".into(),
                value: Some("value".into()),
                old_value: None,
                note: String::new(),
            }],
        }];
        let written = write_markdown_file(
            &service_root,
            "auth-env-changes.md",
            "dev",
            "master",
            &reports,
        )
        .unwrap();
        assert_eq!(written, service_root.join("auth-env-changes.md"));
        let content = fs::read_to_string(&written).unwrap();
        assert!(content.starts_with("# Comparing branches: dev → master\n\n## dev\n"));
        assert!(content.contains("- NEW: value"));
    }

    #[test]
    fn write_markdown_file_custom_name() {
        let tmp = tempfile::tempdir().unwrap();
        let service_root = tmp.path().join("auth");
        fs::create_dir_all(&service_root).unwrap();
        let reports = vec![FileReport {
            display: "deploy/dev/kubernetes/configmap.yml".into(),
            kind: FileKind::ConfigMap,
            items: vec![Item {
                kind: FileKind::ConfigMap,
                change: ChangeKind::Created,
                key: "NEW".into(),
                value: Some("value".into()),
                old_value: None,
                note: String::new(),
            }],
        }];
        let written =
            write_markdown_file(&service_root, "custom.md", "master", "main", &reports).unwrap();
        assert_eq!(written, service_root.join("custom.md"));
        assert!(written.exists());
        assert_eq!(
            fs::read_to_string(&written).unwrap(),
            "# Comparing branches: master → main\n\n## dev\n\n### configmap\n---\n<strong>Updated</strong>\n\n- NEW: value\n\n<strong>Deleted</strong> \x1b[32m(No changes)\x1b[0m\n\n### secrets \x1b[32m(No changes)\x1b[0m\n"
        );
    }

    #[test]
    fn markdown_escapes_multiline_values() {
        let files = vec![FileReport {
            display: "configmap.yml".into(),
            kind: FileKind::ConfigMap,
            items: vec![Item {
                kind: FileKind::ConfigMap,
                change: ChangeKind::Created,
                key: "MULTI".into(),
                value: Some("line1\nline2".into()),
                old_value: None,
                note: String::new(),
            }],
        }];
        let md = render_markdown(&files, "dev", "master");
        assert!(md.contains("- MULTI: line1\\nline2"));
    }

    #[test]
    fn markdown_configmap_empty_value_still_renders_value() {
        // A configmap key whose `--from` value is empty (`''`) must still read
        // `KEY: value`, not a bare key. Secrets keep rendering bare unless a
        // real configmap value was moved in.
        let files = vec![
            FileReport {
                display: "configmap.yml".into(),
                kind: FileKind::ConfigMap,
                items: vec![
                    Item {
                        kind: FileKind::ConfigMap,
                        change: ChangeKind::Created,
                        key: "EMPTY".into(),
                        value: Some(String::new()),
                        old_value: None,
                        note: String::new(),
                    },
                    Item {
                        kind: FileKind::ConfigMap,
                        change: ChangeKind::Updated,
                        key: "FROM_SECRET".into(),
                        value: Some(String::new()),
                        old_value: Some("old".into()),
                        note: "(from secret)".into(),
                    },
                ],
            },
            FileReport {
                display: "secrets.yml".into(),
                kind: FileKind::Secret,
                items: vec![Item {
                    kind: FileKind::Secret,
                    change: ChangeKind::Created,
                    key: "SECRET_ONLY".into(),
                    value: None,
                    old_value: None,
                    note: String::new(),
                }],
            },
        ];
        let md = render_markdown(&files, "dev", "master");
        assert!(
            md.contains("- EMPTY:"),
            "empty configmap value renders:\n{md}"
        );
        assert!(
            md.contains("- FROM_SECRET: (from secret)"),
            "annotated empty configmap value renders:\n{md}"
        );
        assert!(md.contains("- SECRET_ONLY"), "secret stays bare:\n{md}");
        assert!(
            !md.contains("- SECRET_ONLY:"),
            "secret must not gain a value:\n{md}"
        );
    }
}
