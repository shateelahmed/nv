//! `nv ls` — list all unique env keys for a service and environment.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use anyhow::{Result, bail};
use regex::Regex;

use super::{Cli, context};
use crate::model::FileKind;

/// True when a file's display path contains a path segment equal to `env`.
fn in_environment(display: &str, env: &str) -> bool {
    display.split('/').any(|segment| segment == env)
}

/// Extract ENV directive names from a Dockerfile's content.
///
/// Returns the first token after `ENV` on each directive line.
/// Continuation lines (starting with whitespace followed by `\`) are ignored.
/// `ARG` directives are excluded because they are build-time only and do not
/// persist as environment variables at runtime.
fn dockerfile_keys(content: &str) -> Vec<String> {
    let re =
        Regex::new(r"(?m)^\s*ENV\s+([A-Za-z_][A-Za-z0-9_]*)").expect("hardcoded regex is valid");
    re.captures_iter(content)
        .map(|cap| cap[1].to_string())
        .collect()
}

/// Extract ARG directive names from a Dockerfile's content.
///
/// Returns the first token after `ARG` on each directive line.
fn dockerfile_arg_keys(content: &str) -> Vec<String> {
    let re =
        Regex::new(r"(?m)^\s*ARG\s+([A-Za-z_][A-Za-z0-9_]*)").expect("hardcoded regex is valid");
    re.captures_iter(content)
        .map(|cap| cap[1].to_string())
        .collect()
}

/// Collect keys from a parsed file, adding each to `keys` and the file's
/// display path to `files`.
fn collect_keys(
    content: &str,
    kind: FileKind,
    display: &str,
    keys: &mut BTreeSet<String>,
    files: &mut BTreeSet<String>,
) {
    for pair in crate::parser::parse(content, kind) {
        let k = pair.key.trim().to_string();
        if !k.is_empty() {
            keys.insert(k);
            files.insert(display.to_string());
        }
    }
}

/// Handle `nv ls -s SERVICE -e ENVIRONMENT [--flat] [--secrets|--configmap]`.
pub fn run(
    cli: &Cli,
    environment: &str,
    flat: bool,
    secrets_only: bool,
    configmap_only: bool,
) -> Result<()> {
    let ctx = context::resolve(cli)?;
    context::print_banner(&ctx);

    if cli.services.len() != 1 {
        bail!("ls requires exactly one --service to select a single service.");
    }
    let service_name = &cli.services[0];

    let service = ctx
        .services
        .iter()
        .find(|s| &s.name == service_name)
        .ok_or_else(|| anyhow::anyhow!("service '{service_name}' not found"))?;

    let mut keys = BTreeSet::new();
    let mut files = BTreeSet::new();

    // Configmaps and secrets scoped to the specified environment.
    for file in &service.files {
        if !matches!(file.kind, FileKind::ConfigMap | FileKind::Secret) {
            continue;
        }
        if secrets_only && file.kind != FileKind::Secret {
            continue;
        }
        if configmap_only && file.kind != FileKind::ConfigMap {
            continue;
        }
        if !in_environment(&file.display, environment) {
            continue;
        }
        let content = match fs::read_to_string(&file.path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        collect_keys(&content, file.kind, &file.display, &mut keys, &mut files);
    }

    // .env files — only when the user specifies the local environment.
    if environment == "local" && !secrets_only && !configmap_only {
        for file in &service.files {
            if file.kind != FileKind::Dotenv {
                continue;
            }
            let content = match fs::read_to_string(&file.path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            collect_keys(&content, file.kind, &file.display, &mut keys, &mut files);
        }
    }

    // Dockerfiles (common to all environments) — excluded when filtering by
    // secrets/configmap since Dockerfiles are neither.
    if !secrets_only && !configmap_only {
        for (abs_path, display) in dockerfiles(&service.path) {
            let content = match fs::read_to_string(&abs_path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            // ENV keys are always included.
            for k in dockerfile_keys(&content) {
                if !k.is_empty() {
                    keys.insert(k);
                    files.insert(display.clone());
                }
            }
            // ARG keys are included only if already present in another source
            // (configmap, secrets, .env, or a Dockerfile ENV).
            for k in dockerfile_arg_keys(&content) {
                if !k.is_empty() && keys.contains(&k) {
                    files.insert(display.clone());
                }
            }
        }
    }

    if keys.is_empty() {
        eprintln!("No keys found.");
        return Ok(());
    }

    let file_count = files.len();
    let key_count = keys.len();

    // Print files section.
    println!("Files ({file_count}):");
    for f in &files {
        println!("  {f}");
    }

    // Print keys section.
    println!("\nKeys ({key_count}):");
    if flat {
        let line = keys
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        println!("  {line}");
    } else {
        for k in &keys {
            println!("  {k}");
        }
    }

    Ok(())
}

/// Glob for `Dockerfile*` files under the service root.
///
/// Returns `(absolute_path, display_path)` tuples where `display_path` is the
/// path relative to the service root (e.g. `docker/app/Dockerfile`).
fn dockerfiles(service_path: &Path) -> Vec<(std::path::PathBuf, String)> {
    let mut out = Vec::new();
    collect_dockerfiles(service_path, service_path, &mut out);
    out
}

/// Recursively walk `dir` looking for files whose name starts with `Dockerfile`.
fn collect_dockerfiles(base: &Path, dir: &Path, out: &mut Vec<(std::path::PathBuf, String)>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten() {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        if path.is_dir() {
            collect_dockerfiles(base, &path, out);
        } else {
            let fname = entry.file_name().to_string_lossy().into_owned();
            if fname.starts_with("Dockerfile") {
                let display = path
                    .strip_prefix(base)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .into_owned();
                out.push((path, display));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dockerfile_keys_basic() {
        let content = "\
ARG PHP_VERSION=8.1.8-fpm
ARG HTTP_PROXY
ARG BUILD_MODE=\"prod\"
ENV TIMEZONE=\"Asia/Dhaka\"
ENV APP_DEBUG=true
";
        let mut keys = dockerfile_keys(content);
        keys.sort();
        // Only ENV directives are collected; ARG directives are excluded.
        assert_eq!(keys, vec!["APP_DEBUG", "TIMEZONE"]);
    }

    #[test]
    fn dockerfile_arg_keys_basic() {
        let content = "\
ARG PHP_VERSION=8.1.8-fpm
ARG HTTP_PROXY
ARG GID=1000
ENV TIMEZONE=\"Asia/Dhaka\"
";
        let mut keys = dockerfile_arg_keys(content);
        keys.sort();
        assert_eq!(keys, vec!["GID", "HTTP_PROXY", "PHP_VERSION"]);
    }

    #[test]
    fn dockerfile_keys_multiline_env() {
        let content = "\
ENV APP_DEBUG=true \\
    APP_NAME=myapp
ARG GID=1000
";
        // Only the first token on each directive line is captured.
        // ARG GID is excluded from ENV extraction.
        let mut keys = dockerfile_keys(content);
        keys.sort();
        assert_eq!(keys, vec!["APP_DEBUG"]);
    }

    #[test]
    fn dockerfile_keys_empty() {
        let content = "# just a comment\n";
        assert!(dockerfile_keys(content).is_empty());
    }

    #[test]
    fn in_environment_matches() {
        assert!(in_environment(
            "deploy/dev/kubernetes/configmap-app.yaml",
            "dev"
        ));
        assert!(in_environment("deploy/prod/kubernetes/app.yaml", "prod"));
        assert!(!in_environment("deploy/dev/kubernetes/app.yaml", "prod"));
        // No deploy folder at all.
        assert!(!in_environment("docker/app/.env", "dev"));
    }

    #[test]
    fn in_environment_segments() {
        // Matches any segment, not just the first.
        assert!(in_environment("some/path/dev/other/configmap.yaml", "dev"));
    }

    #[test]
    fn collect_keys_adds_to_sets() {
        let content = "APP_DEBUG=true\nAPP_NAME=myapp\n";
        let mut keys = BTreeSet::new();
        let mut files = BTreeSet::new();
        collect_keys(
            content,
            FileKind::Dotenv,
            "docker/app/.env",
            &mut keys,
            &mut files,
        );
        assert!(keys.contains("APP_DEBUG"));
        assert!(keys.contains("APP_NAME"));
        assert!(files.contains("docker/app/.env"));
    }

    #[test]
    fn collect_keys_skips_empty() {
        let content = "EMPTY=\n";
        let mut keys = BTreeSet::new();
        let mut files = BTreeSet::new();
        collect_keys(content, FileKind::Dotenv, "test.env", &mut keys, &mut files);
        // EMPTY= is parsed but the key "EMPTY" is non-empty, so it IS collected.
        assert!(keys.contains("EMPTY"));
    }
}
