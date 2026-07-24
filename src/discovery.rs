//! Resolve microservices and their env files from configuration or by scanning.
//!
//! "Discovery" answers two questions: which folders are services, and which
//! files inside each are env files. It works either from an explicit list in
//! `nv.yml` or, when none is given, by scanning the filesystem.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::config::{Config, ServiceFiles};
use crate::model::{EnvFile, FileKind, Service};
use crate::parser::detect_kind;

/// Discover all services described by `config`, resolving paths against `base`
/// (the directory containing `nv.yml`, or the working directory).
pub fn discover(config: &Config, base: &Path) -> Result<Vec<Service>> {
    let root = config.services_root_abs(base);
    // No explicit `services:` list means "treat every subfolder as a service".
    if config.services.is_empty() {
        discover_scanned(&root, &config.ignore)
    } else {
        discover_explicit(config, &root)
    }
}

/// Scan `root` treating every subfolder (minus `ignore`) as a service.
pub fn discover_scanned(root: &Path, ignore: &[String]) -> Result<Vec<Service>> {
    let mut services = Vec::new();
    let entries = std::fs::read_dir(root)
        .with_context(|| format!("reading services root {}", root.display()))?;

    // Collect candidate directories first so we can sort them for stable order.
    let mut dirs: Vec<PathBuf> = Vec::new();
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue; // only folders can be services
        }
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        // Skip hidden folders (like `.git`) and anything on the ignore list.
        if name.starts_with('.') || ignore.iter().any(|i| i == &name) {
            continue;
        }
        dirs.push(path);
    }
    dirs.sort();

    // Turn each directory into a Service with its discovered env files.
    for dir in dirs {
        let name = dir.file_name().unwrap().to_string_lossy().to_string();
        let files = auto_discover_files(&dir)?;
        services.push(Service {
            name,
            path: dir,
            files,
        });
    }
    Ok(services)
}

/// Build services from an explicit config list.
fn discover_explicit(config: &Config, root: &Path) -> Result<Vec<Service>> {
    let mut services = Vec::new();
    for svc in &config.services {
        let rel = svc.path.clone().unwrap_or_else(|| svc.name.clone());
        let dir = root.join(rel);
        let files = match &svc.files {
            Some(sel) => resolve_configured_files(&dir, sel),
            None => auto_discover_files(&dir)?,
        };
        services.push(Service {
            name: svc.name.clone(),
            path: dir,
            files,
        });
    }
    Ok(services)
}

/// Resolve explicitly configured file names for each kind.
fn resolve_configured_files(dir: &Path, sel: &ServiceFiles) -> Vec<EnvFile> {
    let mut files = Vec::new();
    let groups: [(FileKind, &Option<Vec<String>>); 4] = [
        (FileKind::Dotenv, &sel.dotenv),
        (FileKind::DotenvExample, &sel.dotenv_example),
        (FileKind::ConfigMap, &sel.configmap),
        (FileKind::Secret, &sel.secret),
    ];
    for (kind, names) in groups {
        if let Some(names) = names {
            for name in names {
                let path = dir.join(name);
                files.push(EnvFile {
                    kind,
                    path,
                    display: name.clone(),
                });
            }
        }
    }
    files
}

/// Directory names skipped while scanning for env files: version control,
/// editor state, and dependency/build output folders that never hold config we
/// want to edit (and can be huge).
const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    "vendor",
    ".venv",
    "__pycache__",
];

/// How deep to search within a service folder for nested env files.
const MAX_DEPTH: usize = 8;

/// Auto-discover env files in a service directory by filename pattern.
///
/// Env files are often nested (e.g. `src/.env`, `docker/app/.env`, or
/// `deploy/prod/kubernetes/configmap-*.yaml`), so this walks subdirectories
/// too. Each file's `display` is its path relative to the service folder, so
/// several `.env` files in different subfolders stay distinguishable.
fn auto_discover_files(dir: &Path) -> Result<Vec<EnvFile>> {
    let mut files = Vec::new();
    if !dir.is_dir() {
        return Ok(files); // nothing to scan
    }
    // `dir` is both the starting point and the base we make paths relative to.
    walk_for_env_files(dir, dir, 0, &mut files)?;
    files.sort_by(|a, b| a.display.cmp(&b.display));
    Ok(files)
}

/// Recursively collect env files under `dir`, computing `display` relative to
/// `base`. Hidden and well-known noise directories are skipped.
fn walk_for_env_files(
    base: &Path,
    dir: &Path,
    depth: usize,
    out: &mut Vec<EnvFile>,
) -> Result<()> {
    if depth > MAX_DEPTH {
        return Ok(());
    }
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .with_context(|| format!("reading dir {}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path())) // ignore unreadable entries
        .collect();
    entries.sort();

    for path in entries {
        if path.is_dir() {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            // Skip hidden folders (like `.git`, `.github`) and known noise.
            if name.starts_with('.') || SKIP_DIRS.contains(&name) {
                continue;
            }
            walk_for_env_files(base, &path, depth + 1, out)?;
        } else if path.is_file()
            && let Some(kind) = detect_kind(&path)
        {
            // Show the path relative to the service folder, e.g. `src/.env`.
            let display = path
                .strip_prefix(base)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string();
            files_push(out, kind, path, display);
        }
    }
    Ok(())
}

/// Small helper to append an [`EnvFile`] (keeps the loop above readable).
fn files_push(out: &mut Vec<EnvFile>, kind: FileKind, path: PathBuf, display: String) {
    out.push(EnvFile {
        kind,
        path,
        display,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn touch(dir: &Path, name: &str) {
        fs::write(dir.join(name), "").unwrap();
    }

    #[test]
    fn scans_subfolders_as_services() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let auth = root.join("auth");
        let billing = root.join("billing");
        fs::create_dir(&auth).unwrap();
        fs::create_dir(&billing).unwrap();
        touch(&auth, ".env");
        touch(&auth, ".env.example");
        touch(&billing, "configmap.yml");

        let services = discover_scanned(root, &[]).unwrap();
        assert_eq!(services.len(), 2);
        assert_eq!(services[0].name, "auth");
        assert_eq!(services[0].files.len(), 2);
        assert_eq!(services[1].name, "billing");
        assert_eq!(services[1].files[0].kind, FileKind::ConfigMap);
    }

    #[test]
    fn respects_ignore_list() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir(root.join("keep")).unwrap();
        fs::create_dir(root.join("skip")).unwrap();
        let services = discover_scanned(root, &["skip".to_string()]).unwrap();
        assert_eq!(services.len(), 1);
        assert_eq!(services[0].name, "keep");
    }

    #[test]
    fn finds_env_files_nested_in_subfolders() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let svc = root.join("api");
        // Env files live in nested folders, not at the service top level.
        fs::create_dir_all(svc.join("src")).unwrap();
        fs::create_dir_all(svc.join("docker/app")).unwrap();
        fs::create_dir_all(svc.join("deploy/prod/kubernetes")).unwrap();
        touch(&svc.join("src"), ".env");
        touch(&svc.join("docker/app"), ".env.example");
        touch(&svc.join("deploy/prod/kubernetes"), "configmap-api.yaml");

        let services = discover_scanned(root, &[]).unwrap();
        assert_eq!(services.len(), 1);
        let files = &services[0].files;
        assert_eq!(files.len(), 3);

        // `display` is the path relative to the service folder.
        let displays: Vec<&str> = files.iter().map(|f| f.display.as_str()).collect();
        assert!(displays.contains(&"src/.env"));
        assert!(displays.contains(&"docker/app/.env.example"));
        assert!(displays.contains(&"deploy/prod/kubernetes/configmap-api.yaml"));
    }

    #[test]
    fn distinguishes_same_named_files_in_different_subfolders() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let svc = root.join("api");
        fs::create_dir_all(svc.join("src")).unwrap();
        fs::create_dir_all(svc.join("docker")).unwrap();
        touch(&svc.join("src"), ".env");
        touch(&svc.join("docker"), ".env");

        let services = discover_scanned(root, &[]).unwrap();
        let displays: Vec<&str> = services[0].files.iter().map(|f| f.display.as_str()).collect();
        assert_eq!(displays, vec!["docker/.env", "src/.env"]); // sorted, distinct
    }

    #[test]
    fn skips_noise_and_hidden_directories() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let svc = root.join("api");
        // Real env file plus decoys inside skipped directories.
        fs::create_dir_all(svc.join("src")).unwrap();
        fs::create_dir_all(svc.join("node_modules/pkg")).unwrap();
        fs::create_dir_all(svc.join("vendor")).unwrap();
        fs::create_dir_all(svc.join(".git")).unwrap();
        touch(&svc.join("src"), ".env");
        touch(&svc.join("node_modules/pkg"), ".env");
        touch(&svc.join("vendor"), ".env");
        touch(&svc.join(".git"), ".env");

        let services = discover_scanned(root, &[]).unwrap();
        let displays: Vec<&str> = services[0].files.iter().map(|f| f.display.as_str()).collect();
        assert_eq!(displays, vec!["src/.env"]);
    }
}
