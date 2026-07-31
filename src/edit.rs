//! Building, previewing, and applying a set of file edits.
//!
//! Edits are computed as full old/new file contents so they can be diffed and
//! confirmed before anything is written to disk.
//!
//! The flow is: pick **targets** (which files to touch) → build a **ChangeSet**
//! (compute the new text for each) → show a **diff** → **apply** (write to disk).
//! Nothing touches the filesystem until `apply` is called.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Context, Result};
use similar::{ChangeTag, TextDiff};

use crate::color::{AnsiColor, ColorConfig};
use crate::display::{self, Output, TreeFile, TreeItem, TreeService};
use crate::model::{EnvFile, FileKind, Service};
use crate::parser;

/// A single target file for an operation.
#[derive(Debug, Clone)]
pub struct Target {
    pub service: String,
    pub file: EnvFile,
}

/// Collect targets across `services`, optionally filtered by service names and
/// file kinds. Empty filters mean "all".
pub fn collect_targets(
    services: &[Service],
    service_filter: &[String],
    kind_filter: &[FileKind],
) -> Vec<Target> {
    let mut targets = Vec::new();
    for service in services {
        // Skip services not named in the filter (unless the filter is empty).
        if !service_filter.is_empty() && !service_filter.iter().any(|s| s == &service.name) {
            continue;
        }
        for file in &service.files {
            // Skip file kinds not named in the filter (unless it's empty).
            if !kind_filter.is_empty() && !kind_filter.contains(&file.kind) {
                continue;
            }
            targets.push(Target {
                service: service.name.clone(),
                file: file.clone(),
            });
        }
    }
    targets
}

/// A computed edit to one file: what it says now versus what it will say.
#[derive(Debug, Clone)]
pub struct FileChange {
    pub service: String,
    pub display: String,
    pub path: PathBuf,
    /// The file kind, retained for callers that want to filter or inspect
    /// changes by type.
    #[allow(dead_code)]
    pub kind: FileKind,
    /// The key and value being written, retained for callers that want to
    /// report or inspect an individual change.
    #[allow(dead_code)]
    pub key: String,
    /// The value that will actually be written (already adjusted for example
    /// files, base64 rules, etc. by the caller-supplied value function).
    #[allow(dead_code)]
    pub value: String,
    pub old_content: String,
    pub new_content: String,
}

impl FileChange {
    /// Whether this change actually modifies the file.
    pub fn is_noop(&self) -> bool {
        self.old_content == self.new_content
    }
}

/// A collection of pending file changes.
#[derive(Debug, Clone, Default)]
pub struct ChangeSet {
    pub changes: Vec<FileChange>,
}

impl ChangeSet {
    /// Build a change set that sets `key` in each target, computing the value
    /// per target via `value_for` (allowing per-file values such as empty
    /// example-file secrets or unique generated secrets).
    ///
    /// `value_for` is a closure (an inline function). Passing it in lets the
    /// caller decide the value per file — e.g. empty for `.env.example`, or a
    /// different secret for each target.
    pub fn build<F>(targets: &[Target], key: &str, mut value_for: F) -> Result<ChangeSet>
    where
        F: FnMut(&Target) -> String,
    {
        let mut changes = Vec::new();
        for target in targets {
            // Read what's there now (empty string if the file doesn't exist).
            let old_content = read_or_empty(&target.file.path)?;
            let value = value_for(target);
            // Compute the new text without writing anything yet.
            let new_content = parser::set_value(&old_content, target.file.kind, key, &value);
            changes.push(FileChange {
                service: target.service.clone(),
                display: target.file.display.clone(),
                path: target.file.path.clone(),
                kind: target.file.kind,
                key: key.to_string(),
                value,
                old_content,
                new_content,
            });
        }
        Ok(ChangeSet { changes })
    }

    /// Build a change set that reorders each target's keys to follow `order`,
    /// using the target's own file kind to pick the reorder strategy. Targets
    /// whose content cannot be read are skipped silently; targets already in
    /// the target order become no-op changes (dropped by `effective()`).
    pub fn reorder(targets: &[Target], order: &[String]) -> Result<ChangeSet> {
        let mut changes = Vec::new();
        for target in targets {
            let old_content = match read_or_empty(&target.file.path) {
                Ok(content) => content,
                Err(_) => continue,
            };
            let new_content = parser::reorder(&old_content, target.file.kind, order);
            changes.push(FileChange {
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
        Ok(ChangeSet { changes })
    }

    /// Changes that actually modify a file.
    pub fn effective(&self) -> impl Iterator<Item = &FileChange> {
        self.changes.iter().filter(|c| !c.is_noop())
    }

    /// Whether there is anything to write.
    pub fn is_empty(&self) -> bool {
        self.effective().next().is_none()
    }
    /// Render a hierarchical, colorized diff of all effective changes with
    /// tree-style vertical lines.
    ///
    /// Groups changes by service, then shows each file with its full relative
    /// path and `+`/`-` diff lines with color.
    ///
    /// Tree lines are colored to match the text they lead to:
    /// - File-level branches: file color (cyan)
    /// - Diff-level branches: the diff line's color (added/removed)
    ///
    /// ```text
    /// service_name/
    /// ├── docker/.env
    /// │   ├── - OLD_LINE
    /// │   └── + NEW_LINE
    /// └── configmap.yml
    ///     └── - OLD_LINE
    /// ```
    pub fn render_diff(&self, colors: &ColorConfig, use_color: bool) -> String {
        // Group changes by service name.
        let mut by_service: BTreeMap<String, Vec<&FileChange>> = BTreeMap::new();
        for change in self.effective() {
            by_service
                .entry(change.service.clone())
                .or_default()
                .push(change);
        }

        let mut total_diffs = 0usize;

        let services: Vec<TreeService> = by_service
            .into_iter()
            .map(|(service_name, changes)| {
                let tree_files: Vec<TreeFile> = changes
                    .iter()
                    .map(|change| {
                        let diff = TextDiff::from_lines(&change.old_content, &change.new_content);
                        let diff_lines: Vec<(AnsiColor, String)> = diff
                            .iter_all_changes()
                            .filter_map(|op| match op.tag() {
                                ChangeTag::Equal => None,
                                ChangeTag::Delete => Some((
                                    colors.removed,
                                    format!("- {}", op.value().trim_end_matches('\n')),
                                )),
                                ChangeTag::Insert => Some((
                                    colors.added,
                                    format!("+ {}", op.value().trim_end_matches('\n')),
                                )),
                            })
                            .collect();

                        total_diffs += diff_lines.len();

                        let items: Vec<TreeItem> = diff_lines
                            .into_iter()
                            .map(|(color, label)| TreeItem { label, color })
                            .collect();

                        TreeFile {
                            name: change.display.clone(),
                            count: items.len(),
                            items,
                        }
                    })
                    .collect();

                let service_count: usize = tree_files.iter().map(|f| f.count).sum();
                TreeService {
                    name: service_name,
                    count: service_count,
                    files: tree_files,
                }
            })
            .collect();

        let mut out = Output::String(String::new());
        display::render_tree(&services, colors, use_color, true, &mut out);

        if total_diffs > 0 {
            display::render_summary(
                &format!("{} potential change(s) found.", total_diffs),
                colors,
                use_color,
                &mut out,
            );
        }

        match out {
            Output::String(s) => s,
            _ => unreachable!(),
        }
    }

    /// Write all effective changes to disk, creating files and parent dirs as
    /// needed. Returns how many files were written.
    pub fn apply(&self) -> Result<usize> {
        let mut written = 0;
        for change in self.effective() {
            // Make sure the containing folder exists before writing.
            if let Some(parent) = change.path.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("creating {}", parent.display()))?;
            }
            std::fs::write(&change.path, &change.new_content)
                .with_context(|| format!("writing {}", change.path.display()))?;
            written += 1;
        }
        Ok(written)
    }
}

/// Read a file's text, treating "file not found" as an empty string so new
/// files can be created seamlessly. Other I/O errors are still reported.
pub fn read_or_empty(path: &std::path::Path) -> Result<String> {
    match std::fs::read_to_string(path) {
        Ok(c) => Ok(c),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn target(dir: &std::path::Path, name: &str, kind: FileKind) -> Target {
        Target {
            service: "svc".into(),
            file: EnvFile {
                kind,
                path: dir.join(name),
                display: name.into(),
            },
        }
    }

    #[test]
    fn builds_and_applies_same_value_across_files() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join(".env"), "KEY=old\n").unwrap();
        fs::write(tmp.path().join("configmap.yml"), "data:\n  KEY: old\n").unwrap();

        let targets = vec![
            target(tmp.path(), ".env", FileKind::Dotenv),
            target(tmp.path(), "configmap.yml", FileKind::ConfigMap),
        ];
        let cs = ChangeSet::build(&targets, "KEY", |_| "shared".to_string()).unwrap();
        assert_eq!(cs.effective().count(), 2);
        cs.apply().unwrap();

        assert_eq!(
            fs::read_to_string(tmp.path().join(".env")).unwrap(),
            "KEY=shared\n"
        );
        assert_eq!(
            fs::read_to_string(tmp.path().join("configmap.yml")).unwrap(),
            "data:\n  KEY: shared\n"
        );
    }

    #[test]
    fn example_files_can_receive_empty_value() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join(".env"), "TOKEN=\n").unwrap();
        fs::write(tmp.path().join(".env.example"), "TOKEN=\n").unwrap();
        let targets = vec![
            target(tmp.path(), ".env", FileKind::Dotenv),
            target(tmp.path(), ".env.example", FileKind::DotenvExample),
        ];
        let cs = ChangeSet::build(&targets, "TOKEN", |t| {
            if t.file.kind.is_example() {
                String::new()
            } else {
                "secret123".to_string()
            }
        })
        .unwrap();
        cs.apply().unwrap();
        assert_eq!(
            fs::read_to_string(tmp.path().join(".env")).unwrap(),
            "TOKEN=secret123\n"
        );
        assert_eq!(
            fs::read_to_string(tmp.path().join(".env.example")).unwrap(),
            "TOKEN=\n"
        );
    }

    #[test]
    fn creates_missing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let targets = vec![target(tmp.path(), ".env", FileKind::Dotenv)];
        let cs = ChangeSet::build(&targets, "NEW", |_| "v".to_string()).unwrap();
        cs.apply().unwrap();
        assert_eq!(
            fs::read_to_string(tmp.path().join(".env")).unwrap(),
            "NEW=v\n"
        );
    }
}
