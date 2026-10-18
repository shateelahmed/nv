//! `nv remove` — remove environment variable keys from files.

use anyhow::{Result, bail};

use super::{Cli, context};
use crate::color;
use crate::edit::{self, ChangeSet};
use crate::model::FileKind;

/// Handle `nv remove <KEY>...`: remove the specified keys from files scoped by
/// the global filters (`-s/--service`, `-f/--file`, `--all`).
///
/// With no `-f/--file` and no `--all`, every file kind is targeted — the same
/// "empty means all" rule the other commands follow.
pub fn run(cli: &Cli, keys: &[String]) -> Result<()> {
    if keys.is_empty() {
        bail!("no keys specified; provide at least one key to remove");
    }

    let ctx = context::resolve(cli)?;
    context::print_banner(&ctx);

    // Empty kind filter means all kinds (global -f/--all semantics).
    let kind_filter = ctx.kind_filter(cli)?;
    let kind_filter: Vec<FileKind> = if kind_filter.is_empty() {
        vec![
            FileKind::Dotenv,
            FileKind::DotenvExample,
            FileKind::ConfigMap,
            FileKind::Secret,
        ]
    } else {
        kind_filter
    };

    // Build the service filter: -s if provided, empty means all.
    let service_filter = ctx.service_filter(cli);

    let targets = edit::collect_targets(&ctx.services, &service_filter, &kind_filter);

    if targets.is_empty() {
        bail!("no matching files found for the specified flags");
    }

    // Build a change set: for each target, try to remove every specified key.
    let mut changes = ChangeSet::default();
    for target in &targets {
        let old_content = edit::read_or_empty(&target.file.path)?;
        let mut new_content = old_content.clone();

        for key in keys {
            new_content = crate::parser::remove_key(&new_content, target.file.kind, key);
        }

        if new_content != old_content {
            changes.changes.push(edit::FileChange {
                service: target.service.clone(),
                display: target.file.display.clone(),
                path: target.file.path.clone(),
                kind: target.file.kind,
                key: keys.join(", "),
                value: String::new(),
                old_content,
                new_content,
            });
        }
    }

    if changes.is_empty() {
        eprintln!("Nothing to remove.");
        return Ok(());
    }

    let use_color = color::should_use_color();
    let colors = ctx.colors();
    context::preview_and_apply(cli, &changes, &colors, use_color)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use std::path::Path;

    /// Build a `--no-config --root <empty dir>` argv so discovery finds no
    /// services, making the run deterministic (always "no matching files").
    fn empty_root_args(dir: &Path) -> Vec<String> {
        vec![
            "nv".into(),
            "--no-config".into(),
            "--root".into(),
            dir.to_string_lossy().into_owned(),
            "remove".into(),
        ]
    }

    fn empty_dir() -> tempfile::TempDir {
        tempfile::tempdir().expect("create temp dir")
    }

    #[test]
    fn remove_with_no_filters_targets_everything() {
        // No -f/--file and no --all: every file kind is targeted.
        let dir = empty_dir();
        let mut argv = empty_root_args(dir.path());
        argv.push("KEY".into());
        let cli = Cli::try_parse_from(&argv).unwrap();
        let result = run(&cli, &["KEY".into()]);
        // Fails because no services exist in the empty root, not a flag error.
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("no matching files"),
            "expected 'no matching files' error, got: {err}"
        );
    }

    #[test]
    fn file_kind_flag_selects_kinds() {
        // Global -f replaces the old -e/-c/-x subcommand flags.
        let dir = empty_dir();
        let mut argv = empty_root_args(dir.path());
        argv.push("-f".into());
        argv.push("configmap".into());
        argv.push("KEY".into());
        let cli = Cli::try_parse_from(&argv).unwrap();
        let result = run(&cli, &["KEY".into()]);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("no matching files"),
            "expected 'no matching files' error, got: {err}"
        );
    }

    #[test]
    fn all_short_flag_is_global_alias() {
        // -a is now the short alias for the global --all (target everything).
        let dir = empty_dir();
        let mut argv = empty_root_args(dir.path());
        argv.push("-a".into());
        argv.push("KEY".into());
        let cli = Cli::try_parse_from(&argv).unwrap();
        assert!(cli.all);
        let result = run(&cli, &["KEY".into()]);
        assert!(result.is_err());
    }

    #[test]
    fn rejects_unknown_legacy_scope_flags() {
        // The old subcommand-scoped -e/-c/-x/-a are gone.
        assert!(Cli::try_parse_from(["nv", "remove", "KEY", "-e"]).is_err());
        assert!(Cli::try_parse_from(["nv", "remove", "KEY", "-c"]).is_err());
        assert!(Cli::try_parse_from(["nv", "remove", "KEY", "-x"]).is_err());
    }
}
