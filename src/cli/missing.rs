//! `nv missing` — find env keys used in code but missing from
//! configmap/secrets.
//!
//! Scans the project source tree for env-key references (PHP `env()` /
//! `getenv()`, JS `process.env.X` / `env()`), then reports keys that are not
//! defined in the service's configmap or secrets for a given environment. It
//! is the inverse of `nv unused`: unused finds defined-but-unreferenced keys,
//! missing finds referenced-but-undefined keys.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Result, bail};
use glob::Pattern;
use regex::Regex;

use super::{Cli, context};
use crate::color::{self, AnsiColor};
use crate::display::{self, Output, TreeFile, TreeItem, TreeService};
use crate::model::FileKind;

/// Default directories to skip when scanning for key references.
const DEFAULT_SKIP_DIRS: &[&str] = &[".git", "target", "vendor", "node_modules", "logs"];
const DEFAULT_SKIP_FILES: &[&str] = &[];

/// Maximum recursion depth for directory traversal.
const MAX_DEPTH: usize = 20;

/// A single env-key reference found in source code.
#[derive(Debug, Clone)]
struct KeyRef {
    key: String,
    file: String,
    line: usize,
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

/// True when a file's display path contains a path segment equal to `env`.
fn in_environment(display: &str, env: &str) -> bool {
    display.split('/').any(|segment| segment == env)
}

/// Extract the environment name from a file's display path, e.g. `dev` from
/// `deploy/dev/kubernetes/configmap-app.yaml`: the first path segment that is
/// not a structural container folder.
fn env_for(display: &str) -> Option<String> {
    display
        .split('/')
        .find(|seg| !seg.is_empty() && !ENV_WRAPPER_DIRS.contains(seg))
        .map(str::to_string)
}

/// Discover every distinct environment present across a service's
/// configmap/secrets file paths.
fn discovered_environments(service: &crate::model::Service) -> BTreeSet<String> {
    let mut envs = BTreeSet::new();
    for file in &service.files {
        if !matches!(file.kind, FileKind::ConfigMap | FileKind::Secret) {
            continue;
        }
        if let Some(env) = env_for(&file.display) {
            envs.insert(env);
        }
    }
    envs
}

/// Collect every key defined in a service's configmap/secrets scoped to
/// `environment`.
fn defined_keys(service: &crate::model::Service, environment: &str) -> BTreeSet<String> {
    let mut keys = BTreeSet::new();
    for file in &service.files {
        if !matches!(file.kind, FileKind::ConfigMap | FileKind::Secret) {
            continue;
        }
        if !in_environment(&file.display, environment) {
            continue;
        }
        let content = match std::fs::read_to_string(&file.path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        for pair in crate::parser::parse(&content, file.kind) {
            let k = pair.key.trim().to_string();
            if !k.is_empty() {
                keys.insert(k);
            }
        }
    }
    keys
}

/// Extract env-key references from PHP source.
///
/// Matches `env('KEY')`, `env("KEY")`, `getenv('KEY')`, `getenv("KEY")`.
/// Returns tuples of (key, line_number).
fn php_references(content: &str) -> Vec<(String, usize)> {
    // Quote-aware: matches a single- or double-quoted key. Built at runtime
    // because a raw string cannot contain a bare double quote.
    let re = Regex::new(r#"\b(?:env|getenv)\(\s*['"]([A-Za-z_][A-Za-z0-9_]*)['"]"#)
        .expect("hardcoded regex is valid");
    re.captures_iter(content)
        .map(|cap| {
            let key = cap[1].to_string();
            let line = content[..cap.get(0).unwrap().start()]
                .bytes()
                .filter(|&b| b == b'\n')
                .count()
                + 1;
            (key, line)
        })
        .collect()
}

/// Extract env-key references from JavaScript/TypeScript source.
///
/// Matches `process.env.KEY`, `process.env['KEY']`, `process.env["KEY"]`,
/// `env('KEY')`, and `env("KEY")`. Returns tuples of (key, line_number).
fn js_references(content: &str) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    let dot_re =
        Regex::new(r"\bprocess\.env\.([A-Za-z_][A-Za-z0-9_]*)").expect("hardcoded regex is valid");
    let bracket_re = Regex::new(r#"\bprocess\.env\s*\[\s*['"]([A-Za-z_][A-Za-z0-9_]*)['"]\s*\]"#)
        .expect("hardcoded regex is valid");
    let env_re = Regex::new(r#"\benv\(\s*['"]([A-Za-z_][A-Za-z0-9_]*)['"]"#)
        .expect("hardcoded regex is valid");

    for re in [&dot_re, &bracket_re, &env_re] {
        for cap in re.captures_iter(content) {
            let key = cap[1].to_string();
            let line = content[..cap.get(0).unwrap().start()]
                .bytes()
                .filter(|&b| b == b'\n')
                .count()
                + 1;
            out.push((key, line));
        }
    }
    out
}

/// Cheap check for whether a file should be scanned based on its extension.
fn is_scannable_source(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    matches!(ext.as_str(), "php" | "js" | "ts" | "tsx" | "jsx")
}

/// Check if a file path matches any skip_files pattern.
fn matches_skip_pattern(relative_str: &str, name: &str, skip_files: &BTreeSet<String>) -> bool {
    for pattern in skip_files {
        if let Ok(glob_pattern) = Pattern::new(pattern)
            && (glob_pattern.matches(relative_str) || glob_pattern.matches(name))
        {
            return true;
        }
    }
    false
}

/// Recursively walk `dir`, collecting env-key references from source files.
fn scan_dir(
    dir: &Path,
    service_root: &Path,
    skip_dirs: &BTreeSet<String>,
    skip_files: &BTreeSet<String>,
    depth: usize,
    refs: &mut Vec<KeyRef>,
) {
    if depth > MAX_DEPTH {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();

        if path.is_dir() {
            if name.starts_with('.') || skip_dirs.contains(name) {
                continue;
            }
            scan_dir(&path, service_root, skip_dirs, skip_files, depth + 1, refs);
        } else if path.is_file() && is_scannable_source(&path) {
            let relative = path.strip_prefix(service_root).unwrap_or(&path);
            let relative_str = relative.to_str().unwrap_or("");
            if matches_skip_pattern(relative_str, name, skip_files) {
                continue;
            }
            let content = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let display = path
                .strip_prefix(service_root)
                .unwrap_or(&path)
                .to_string_lossy()
                .into_owned();
            let is_php = name.ends_with(".php");
            for (key, line) in if is_php {
                php_references(&content)
            } else {
                js_references(&content)
            } {
                refs.push(KeyRef {
                    key,
                    file: display.clone(),
                    line,
                });
            }
        }
    }
}

/// Handle `nv missing [-e ENVIRONMENT] [-s SERVICE | --all]`.
pub fn run(cli: &Cli, environment: Option<&str>) -> Result<()> {
    let ctx = context::resolve(cli)?;
    context::print_banner(&ctx);

    if cli.all && !cli.services.is_empty() {
        bail!("--all and --service are mutually exclusive.");
    }
    if !cli.all && cli.services.is_empty() {
        bail!("missing requires at least one --service or --all.");
    }

    let selected: Vec<&crate::model::Service> = ctx
        .services
        .iter()
        .filter(|s| cli.all || cli.services.iter().any(|sn| sn == &s.name))
        .collect();

    let mut total_missing = 0;
    let mut tree_services: Vec<TreeService> = Vec::new();

    for service in &selected {
        let mut skip_dirs: BTreeSet<String> =
            DEFAULT_SKIP_DIRS.iter().map(|s| s.to_string()).collect();
        let mut skip_files: BTreeSet<String> =
            DEFAULT_SKIP_FILES.iter().map(|s| s.to_string()).collect();
        if let Some(cfg) = ctx
            .config
            .as_ref()
            .and_then(|c| c.services.get(&service.name))
            .and_then(|sc| sc.commands.as_ref())
            .and_then(|cmd| cmd.missing.as_ref())
        {
            for d in &cfg.skip_dirs {
                skip_dirs.insert(d.clone());
            }
            for f in &cfg.skip_files {
                skip_files.insert(f.clone());
            }
        }

        // One scan of the source tree per service; reuse refs across envs.
        let mut refs = Vec::new();
        scan_dir(
            &service.path,
            &service.path,
            &skip_dirs,
            &skip_files,
            0,
            &mut refs,
        );

        // Determine the environments to check: an explicit -e, else every
        // environment discovered in the service's configmap/secrets files.
        let envs: Vec<String> = match environment {
            Some(e) => vec![e.to_string()],
            None => {
                let discovered = discovered_environments(service);
                if discovered.is_empty() {
                    vec!["*".to_string()]
                } else {
                    discovered.into_iter().collect()
                }
            }
        };

        for env in &envs {
            // Collect defined keys once per service so we can share it across
            // envs by selecting per-env. We recompute for each env (cheap).
            let defined = defined_keys(service, env);

            // Only keys referenced in code but missing from configmap/secrets.
            let missing: Vec<&KeyRef> = refs.iter().filter(|r| !defined.contains(&r.key)).collect();

            if missing.is_empty() {
                continue;
            }

            // Group missing keys by file. A key referenced in multiple files
            // shows all locations.
            let mut by_file: BTreeMap<String, BTreeSet<(String, usize)>> = BTreeMap::new();
            for m in &missing {
                by_file
                    .entry(m.file.clone())
                    .or_default()
                    .insert((m.key.clone(), m.line));
            }

            let env_count = missing.len();
            total_missing += env_count;

            let tree_files: Vec<TreeFile> = by_file
                .into_iter()
                .map(|(file_name, refs)| {
                    let items: Vec<TreeItem> = refs
                        .into_iter()
                        .map(|(key, line)| TreeItem {
                            label: format!("+ {key:<30} {file_name}:{line}"),
                            color: AnsiColor::Green,
                        })
                        .collect();
                    let count = items.len();
                    TreeFile {
                        name: file_name,
                        count,
                        items,
                    }
                })
                .collect();

            // Label the node as `service/env` when scanning multiple envs so
            // the user can tell them apart. The tree renderer adds the
            // trailing `/`.
            let label = if environment.is_some() {
                service.name.clone()
            } else {
                format!("{}/{}", service.name, env)
            };

            tree_services.push(TreeService {
                name: label,
                count: env_count,
                files: tree_files,
            });
        }
    }

    if tree_services.is_empty() {
        eprintln!("No missing keys found.");
        return Ok(());
    }

    let colors = ctx.colors();
    let use_color = color::should_use_color();

    let mut out = Output::Stdout;
    display::render_tree(&tree_services, &colors, use_color, true, &mut out);

    eprintln!(
        "\n{}",
        color::colorize(
            &format!("{total_missing} missing key(s) found."),
            colors.service_root,
            use_color
        )
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn php_env_single_quotes() {
        let content = "$x = env('APP_DEBUG');";
        let refs = php_references(content);
        assert_eq!(refs, vec![("APP_DEBUG".to_string(), 1)]);
    }

    #[test]
    fn php_env_double_quotes_with_default() {
        let content = "$x = env(\"CACHE_TTL\", 60);";
        let refs = php_references(content);
        assert_eq!(refs, vec![("CACHE_TTL".to_string(), 1)]);
    }

    #[test]
    fn php_getenv() {
        let content = "$p = getenv('HTTP_PROXY');\n$n = getenv(\"NO_PROXY\");";
        let refs = php_references(content);
        assert_eq!(
            refs,
            vec![("HTTP_PROXY".to_string(), 1), ("NO_PROXY".to_string(), 2)]
        );
    }

    #[test]
    fn php_ignores_variable_keys() {
        // A variable key is not a literal string and is not detected.
        let content = "env($key);";
        let refs = php_references(content);
        assert!(refs.is_empty());
    }

    #[test]
    fn js_process_env_dot() {
        let content = "const api = process.env.API_KEY;";
        let refs = js_references(content);
        assert_eq!(refs, vec![("API_KEY".to_string(), 1)]);
    }

    #[test]
    fn js_process_env_brackets() {
        let content = "const h = process.env['DB_HOST'];\nconst n = process.env[\"NO_PROXY\"];";
        let refs = js_references(content);
        assert_eq!(
            refs,
            vec![("DB_HOST".to_string(), 1), ("NO_PROXY".to_string(), 2)]
        );
    }

    #[test]
    fn js_env_function() {
        let content = "const url = env('MIX_APP_URL');";
        let refs = js_references(content);
        assert_eq!(refs, vec![("MIX_APP_URL".to_string(), 1)]);
    }

    #[test]
    fn line_numbers_are_accurate() {
        let content = "line1\nline2\nenv('THIRD')\n";
        let refs = php_references(content);
        assert_eq!(refs, vec![("THIRD".to_string(), 3)]);
    }

    #[test]
    fn is_scannable_source_extensions() {
        assert!(is_scannable_source(Path::new("a.php")));
        assert!(is_scannable_source(Path::new("b.js")));
        assert!(is_scannable_source(Path::new("c.ts")));
        assert!(is_scannable_source(Path::new("d.tsx")));
        assert!(is_scannable_source(Path::new("e.jsx")));
        assert!(!is_scannable_source(Path::new("f.txt")));
        assert!(!is_scannable_source(Path::new("g.yaml")));
    }

    #[test]
    fn env_for_skips_structural_containers() {
        assert_eq!(
            env_for("deploy/dev/kubernetes/configmap-app.yaml"),
            Some("dev".to_string())
        );
        assert_eq!(
            env_for("deploy/prod/config/secrets-app.yaml"),
            Some("prod".to_string())
        );
        // The last segment is the file name, not an environment; env_for will
        // still surface it because it is the only non-container segment.
        assert!(env_for("kubernetes/configmaps/app.yaml").is_some());
    }

    #[test]
    fn in_environment_matches_path_segment() {
        assert!(in_environment(
            "deploy/dev/kubernetes/configmap.yaml",
            "dev"
        ));
        assert!(!in_environment(
            "deploy/prod/kubernetes/configmap.yaml",
            "dev"
        ));
    }
}
