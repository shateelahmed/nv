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

/// Auto-discover env files in a service directory by filename pattern.
///
/// Reads the directory and keeps any file whose name matches a known pattern
/// (via `detect_kind`), e.g. `.env` or `configmap.yml`.
fn auto_discover_files(dir: &Path) -> Result<Vec<EnvFile>> {
    let mut files = Vec::new();
    if !dir.is_dir() {
        return Ok(files); // nothing to scan
    }
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .with_context(|| format!("reading service dir {}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path())) // ignore unreadable entries
        .filter(|p| p.is_file())
        .collect();
    entries.sort();

    for path in entries {
        if let Some(kind) = detect_kind(&path) {
            let display = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            files.push(EnvFile {
                kind,
                path,
                display,
            });
        }
    }
    Ok(files)
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
}
