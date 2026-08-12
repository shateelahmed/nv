//! `nv remove` — remove environment variable keys from files.

use anyhow::{Result, bail};

use super::{Cli, context};
use crate::color;
use crate::edit::{self, ChangeSet};
use crate::model::FileKind;

/// Handle `nv remove <KEY>...`: remove the specified keys from files scoped by
/// file type (`-e`, `-c`, `-x`) and optionally by microservice (`-s`).
pub fn run(
    cli: &Cli,
    keys: &[String],
    env: bool,
    configmap: bool,
    secrets: bool,
    all: bool,
) -> Result<()> {
    if keys.is_empty() {
        bail!("no keys specified; provide at least one key to remove");
    }

    // Validate mutual exclusivity: -a must not appear with -e/-c/-x/-s.
    if all && (env || configmap || secrets || !cli.services.is_empty()) {
        bail!("-a cannot be combined with -e, -c, -x, or -s");
    }

    // When -s is present without -e/-c/-x, default to all three file types.
    // If no flags at all are provided, that's an error.
    if !all && !env && !configmap && !secrets && cli.services.is_empty() {
        bail!(
            "specify at least one of -e (env), -c (configmap), -x (secrets), -a (all), or -s <NAME>"
        );
    }

    let ctx = context::resolve(cli)?;
    context::print_banner(&ctx);

    // Build the kind filter from the file type flags.
    // When -s is present without -e/-c/-x, target all file types.
    let use_all_types = all || (!env && !configmap && !secrets);
    let kind_filter: Vec<FileKind> = if use_all_types {
        vec![
            FileKind::Dotenv,
            FileKind::DotenvExample,
            FileKind::ConfigMap,
            FileKind::Secret,
        ]
    } else {
        let mut kinds = Vec::new();
        if env {
            kinds.push(FileKind::Dotenv);
            kinds.push(FileKind::DotenvExample);
        }
        if configmap {
            kinds.push(FileKind::ConfigMap);
        }
        if secrets {
            kinds.push(FileKind::Secret);
        }
        kinds
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

    #[test]
    fn rejects_no_flags_at_all() {
        let cli = Cli::try_parse_from(["nv", "remove", "KEY"]).unwrap();
        let result = run(&cli, &["KEY".into()], false, false, false, false);
        assert!(result.is_err());
    }

    #[test]
    fn service_flag_without_file_flags_defaults_to_all() {
        // -s without -e/-c/-x should not error (defaults to all file types).
        let cli = Cli::try_parse_from(["nv", "remove", "KEY", "-s", "auth"]).unwrap();
        let result = run(&cli, &["KEY".into()], false, false, false, false);
        // Will fail because no services exist in test env, but not a flag error.
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("no matching files"),
            "expected 'no matching files' error, got: {err}"
        );
    }

    #[test]
    fn rejects_all_with_file_flags() {
        let cli = Cli::try_parse_from(["nv", "remove", "KEY", "-a", "-e"]).unwrap();
        let result = run(&cli, &["KEY".into()], true, false, false, true);
        assert!(result.is_err());
    }

    #[test]
    fn rejects_all_with_service() {
        let cli = Cli::try_parse_from(["nv", "remove", "KEY", "-a", "-s", "auth"]).unwrap();
        let result = run(&cli, &["KEY".into()], false, false, false, true);
        assert!(result.is_err());
    }
}
