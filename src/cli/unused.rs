//! `nv unused` — list env keys that are not referenced in the codebase.
//!
//! This command scans env, env example, configmap, and secret files for keys,
//! then searches the entire project source tree for exact, case-sensitive
//! occurrences. Keys that are not found anywhere are reported as unused.
//!
//! Optimisations (memory + speed):
//! - Single Aho-Corasick automaton built once for all unique keys.
//! - Keys are stored as owned strings only once; the search works with
//!   indices (`usize`) and a mutable set of remaining indices.
//! - No `HashSet::difference().cloned()` on every directory/file.
//! - Extension-based binary filter before opening files.
//! - Streaming line-by-line reading; early exit when all keys are found.
//! - Word-boundary check performed only on the rare automaton matches.

use std::collections::{BTreeMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use aho_corasick::{AhoCorasick, MatchKind};
use anyhow::Result;
use glob::Pattern;

use super::{Cli, context};
use crate::color;
use crate::config;
use crate::edit::{ChangeSet, FileChange};
use crate::model::{EnvFile, FileKind, Service};
use crate::parser;

/// Default directories to skip when searching for key usage.
const DEFAULT_SKIP_DIRS: &[&str] = &[".git", "target", "vendor", "node_modules", "logs"];
const DEFAULT_SKIP_FILES: &[&str] = &[];

/// Maximum recursion depth for directory traversal.
const MAX_DEPTH: usize = 20;

/// Number of bytes to read for binary detection (fallback only).
const BINARY_CHECK_BYTES: usize = 512;

/// Extensions that are almost always text; we skip the null-byte check for them.
const TEXT_EXTENSIONS: &[&str] = &[
    "rs",
    "go",
    "py",
    "js",
    "ts",
    "tsx",
    "jsx",
    "java",
    "kt",
    "c",
    "h",
    "cpp",
    "hpp",
    "cs",
    "rb",
    "php",
    "swift",
    "scala",
    "clj",
    "ex",
    "exs",
    "erl",
    "hs",
    "ml",
    "mli",
    "toml",
    "yaml",
    "yml",
    "json",
    "jsonc",
    "xml",
    "html",
    "htm",
    "css",
    "scss",
    "md",
    "markdown",
    "txt",
    "text",
    "cfg",
    "conf",
    "ini",
    "env",
    "sh",
    "bash",
    "zsh",
    "fish",
    "ps1",
    "bat",
    "cmd",
    "dockerfile",
    "makefile",
    "cmake",
    "sql",
    "graphql",
    "proto",
    "thrift",
    "avsc",
    "tf",
    "hcl",
    "nix",
];

/// A key found in an env file, along with its source location.
#[derive(Debug, Clone)]
struct KeyLocation {
    service: String,
    file: EnvFile,
    key: String,
}

/// Collect all keys from env/example/configmap/secret files.
fn collect_all_keys(
    services: &[Service],
    service_filter: &[String],
    config: &Option<config::Config>,
    global_skip_files: &HashSet<String>,
) -> Vec<KeyLocation> {
    let mut keys = Vec::new();
    for service in services {
        if !service_filter.is_empty() && !service_filter.iter().any(|s| s == &service.name) {
            continue;
        }

        // Build merged skip_files for this service
        let mut merged_skip_files = global_skip_files.clone();
        if let Some(cfg) = config
            && let Some(service_config) = cfg.services.get(&service.name)
            && let Some(ref cmd) = service_config.commands
            && let Some(ref unused_config) = cmd.unused
        {
            for file_name in &unused_config.skip_files {
                merged_skip_files.insert(file_name.clone());
            }
        }

        for file in &service.files {
            if !matches!(
                file.kind,
                FileKind::Dotenv | FileKind::DotenvExample | FileKind::ConfigMap | FileKind::Secret
            ) {
                continue;
            }

            // Skip files matching skip_files patterns
            let relative = file.path.strip_prefix(&service.path).unwrap_or(&file.path);
            let relative_str = relative.to_str().unwrap_or("");
            let name = file.path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if matches_skip_pattern(relative_str, name, &merged_skip_files) {
                continue;
            }

            let content = std::fs::read_to_string(&file.path).unwrap_or_default();
            let pairs = parser::parse(&content, file.kind);
            for pair in pairs {
                if !pair.value.is_empty() {
                    keys.push(KeyLocation {
                        service: service.name.clone(),
                        file: file.clone(),
                        key: pair.key,
                    });
                }
            }
        }
    }
    keys
}

/// Build the set of directories to skip when searching.
fn build_skip_dirs(config_skip_dirs: &[String]) -> HashSet<String> {
    let mut skip: HashSet<String> = DEFAULT_SKIP_DIRS.iter().map(|s| s.to_string()).collect();
    for dir in config_skip_dirs {
        skip.insert(dir.clone());
    }
    skip
}

/// Build the set of files to skip when searching.
fn build_skip_files(config_skip_files: &[String]) -> HashSet<String> {
    let mut skip: HashSet<String> = DEFAULT_SKIP_FILES.iter().map(|s| s.to_string()).collect();
    for file in config_skip_files {
        skip.insert(file.clone());
    }
    skip
}

/// Check if a file path matches any skip_files pattern.
/// Supports glob patterns: `*` matches any characters except `/`,
/// `**` matches any characters including `/` (recursive).
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

/// Cheap extension / name-based text filter. Falls back to a null-byte check
/// only for unknown files.
fn is_probably_text(path: &Path) -> bool {
    // 1. Special filenames that have no (or unusual) extensions
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        let lower = name.to_ascii_lowercase();
        const SPECIAL_TEXT: &[&str] = &[
            "dockerfile",
            "containerfile",
            "makefile",
            "gnumakefile",
            "cmakelists.txt",
            "rakefile",
            "gemfile",
            "procfile",
            "vagrantfile",
            "jenkinsfile",
            "brewfile",
        ];
        if SPECIAL_TEXT
            .iter()
            .any(|&s| lower == s || lower.starts_with(&(s.to_owned() + ".")))
        {
            return true;
        }
    }

    // 2. Known text extensions
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        let lower = ext.to_ascii_lowercase();
        if TEXT_EXTENSIONS.iter().any(|&t| t == lower) {
            return true;
        }

        // Known binary extensions – skip immediately
        const BINARY_EXT: &[&str] = &[
            "o", "a", "so", "dylib", "dll", "exe", "bin", "class", "jar", "war", "png", "jpg",
            "jpeg", "gif", "webp", "ico", "bmp", "tiff", "pdf", "zip", "tar", "gz", "bz2", "xz",
            "7z", "rar", "woff", "woff2", "ttf", "otf", "eot", "mp3", "mp4", "wav", "ogg", "webm",
            "avi", "mov", "db", "sqlite", "sqlite3",
        ];
        if BINARY_EXT.iter().any(|&b| b == lower) {
            return false;
        }
    }

    // 3. Fallback: peek at the first few bytes
    File::open(path)
        .ok()
        .and_then(|file| {
            let mut buf = [0u8; BINARY_CHECK_BYTES];
            let mut reader = BufReader::new(file);
            let n = std::io::Read::read(&mut reader, &mut buf).ok()?;
            Some(!buf[..n].contains(&0))
        })
        .unwrap_or(false)
}

/// Returns true when the match at `start..end` is a whole-word occurrence
/// of the key (surrounded by non-alphanumeric / non-underscore characters
/// or by the line boundaries).
fn is_word_boundary(line: &str, start: usize, end: usize) -> bool {
    let bytes = line.as_bytes();
    let before_ok =
        start == 0 || (!bytes[start - 1].is_ascii_alphanumeric() && bytes[start - 1] != b'_');
    let after_ok =
        end >= bytes.len() || (!bytes[end].is_ascii_alphanumeric() && bytes[end] != b'_');
    before_ok && after_ok
}

/// Scan a single file with the pre-built automaton.
/// Removes every matched key index from `remaining`.
fn scan_file(path: &Path, ac: &AhoCorasick, remaining: &mut HashSet<usize>) {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return,
    };
    let reader = BufReader::new(file);

    for line_result in reader.lines() {
        let line = match line_result {
            Ok(l) => l,
            Err(_) => continue,
        };

        for mat in ac.find_iter(&line) {
            let pat = mat.pattern().as_usize();
            if remaining.contains(&pat) && is_word_boundary(&line, mat.start(), mat.end()) {
                remaining.remove(&pat);
                if remaining.is_empty() {
                    return;
                }
            }
        }
    }
}

/// Check if a file is an env-like file that should be excluded from search.
fn is_env_like_file(path: &Path) -> bool {
    parser::detect_kind(path).is_some()
}

/// Static context shared across recursive `search_dir` calls.
struct SearchCtx<'a> {
    service_root: &'a Path,
    ac: &'a AhoCorasick,
    skip_dirs: &'a HashSet<String>,
    skip_files: &'a HashSet<String>,
    total_keys: usize,
    progress: &'a context::ScanProgress,
}

/// Recursively search the directory tree, removing found keys from `remaining`.
fn search_dir(dir: &Path, remaining: &mut HashSet<usize>, depth: usize, ctx: &SearchCtx<'_>) {
    if depth > MAX_DEPTH || remaining.is_empty() {
        return;
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    ctx.progress.inc_dirs();

    for entry in entries.flatten() {
        if remaining.is_empty() {
            return;
        }

        let path = entry.path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();

        if path.is_dir() {
            if name.starts_with('.') || ctx.skip_dirs.contains(name) {
                continue;
            }
            search_dir(&path, remaining, depth + 1, ctx);
        } else if path.is_file() && is_probably_text(&path) {
            // Check if the file should be skipped by matching against its relative path from service root
            let relative_path = path.strip_prefix(ctx.service_root).unwrap_or(&path);
            let relative_str = relative_path.to_str().unwrap_or("");
            if matches_skip_pattern(relative_str, name, ctx.skip_files) {
                continue;
            }

            // Skip env-like files (dotenv, dotenv_example, configmap, secret)
            if is_env_like_file(&path) {
                continue;
            }

            let display = path.to_str().unwrap_or("?");
            let remaining_count = ctx.total_keys - (ctx.total_keys - remaining.len());
            ctx.progress.set_message(format!(
                "{} key(s) remaining | {}",
                remaining_count, display
            ));
            scan_file(&path, ctx.ac, remaining);
            ctx.progress.inc_files();
        }
    }
}

/// Find a target file by service name and file path.
fn find_target<'a>(
    services: &'a [Service],
    service_name: &str,
    file_path: &str,
) -> Result<&'a EnvFile> {
    let service = services
        .iter()
        .find(|s| s.name == service_name)
        .ok_or_else(|| anyhow::anyhow!("service '{service_name}' not found"))?;

    service
        .files
        .iter()
        .find(|f| f.display == file_path)
        .ok_or_else(|| anyhow::anyhow!("file '{file_path}' not found in service '{service_name}'"))
}

/// Build a ChangeSet that removes unused keys from their files.
fn build_clean_changes(unused_keys: &[KeyLocation], services: &[Service]) -> Result<ChangeSet> {
    let mut by_file: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    for loc in unused_keys {
        by_file
            .entry((loc.service.clone(), loc.file.display.clone()))
            .or_default()
            .push(loc.key.clone());
    }

    let mut changes = Vec::new();
    for ((service_name, file_path), keys) in by_file {
        let target_file = find_target(services, &service_name, &file_path)?;
        let old_content = std::fs::read_to_string(&target_file.path).unwrap_or_default();

        let mut new_content = old_content.clone();
        for key in &keys {
            new_content = parser::remove_key(&new_content, target_file.kind, key);
        }

        if old_content != new_content {
            changes.push(FileChange {
                service: service_name,
                display: target_file.display.clone(),
                path: target_file.path.clone(),
                kind: target_file.kind,
                key: String::new(),
                value: String::new(),
                old_content,
                new_content,
            });
        }
    }

    Ok(ChangeSet { changes })
}

/// Handle `nv unused`: list env keys not referenced in the codebase.
pub fn run(cli: &Cli, service_names: &[String], clean: bool) -> Result<()> {
    let ctx = context::resolve(cli)?;
    context::print_banner(ctx.source);

    let skip_dirs = build_skip_dirs(
        &ctx.config
            .as_ref()
            .and_then(|c| c.commands.as_ref())
            .and_then(|cmd| cmd.unused.as_ref())
            .map(|u| u.skip_dirs.clone())
            .unwrap_or_default(),
    );

    let global_skip_files = build_skip_files(
        &ctx.config
            .as_ref()
            .and_then(|c| c.commands.as_ref())
            .and_then(|cmd| cmd.unused.as_ref())
            .map(|u| u.skip_files.clone())
            .unwrap_or_default(),
    );

    let all_keys = collect_all_keys(
        &ctx.services,
        service_names,
        &ctx.config,
        &global_skip_files,
    );

    if all_keys.is_empty() {
        eprintln!("No keys found to check.");
        return Ok(());
    }

    // Deduplicate while preserving a stable index → string mapping.
    let mut unique_keys: Vec<String> = Vec::new();
    let mut key_to_idx: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for loc in &all_keys {
        if let std::collections::hash_map::Entry::Vacant(e) = key_to_idx.entry(loc.key.clone()) {
            let idx = unique_keys.len();
            unique_keys.push(loc.key.clone());
            e.insert(idx);
        }
    }

    // Build the automaton once.
    let ac = AhoCorasick::builder()
        .match_kind(MatchKind::LeftmostLongest)
        .build(unique_keys.iter().map(|s| s.as_str()))
        .expect("aho-corasick build failed");

    // All keys start as "remaining".
    let mut remaining: HashSet<usize> = (0..unique_keys.len()).collect();

    // Set up progress reporting.
    let progress = context::ScanProgress::start_with_keys(unique_keys.len());

    // Determine which service directories to search:
    // - If `-s` flag is provided, only search within those service directories
    // - Otherwise, search all discovered service directories
    let search_dirs: Vec<&Path> = if !service_names.is_empty() {
        ctx.services
            .iter()
            .filter(|s| service_names.iter().any(|sn| sn == &s.name))
            .map(|s| s.path.as_path())
            .collect()
    } else {
        ctx.services.iter().map(|s| s.path.as_path()).collect()
    };

    // Search each service directory.
    let total_keys = unique_keys.len();
    for dir in search_dirs {
        // Merge global skip_dirs/skip_files with service-specific ones
        let mut merged_skip_dirs = skip_dirs.clone();
        let mut merged_skip_files = global_skip_files.clone();

        // Find service-specific skip_dirs/skip_files for this directory
        if let Some(ref config) = ctx.config {
            let svc_name = ctx
                .services
                .iter()
                .find(|s| s.path.as_path() == dir)
                .map(|s| s.name.as_str());
            if let Some(name) = svc_name
                && let Some(service_config) = config.services.get(name)
                && let Some(ref cmd) = service_config.commands
                && let Some(ref unused_config) = cmd.unused
            {
                for dir_name in &unused_config.skip_dirs {
                    merged_skip_dirs.insert(dir_name.clone());
                }
                for file_name in &unused_config.skip_files {
                    merged_skip_files.insert(file_name.clone());
                }
            }
        }

        let ctx = SearchCtx {
            service_root: dir,
            ac: &ac,
            skip_dirs: &merged_skip_dirs,
            skip_files: &merged_skip_files,
            total_keys,
            progress: &progress,
        };
        search_dir(dir, &mut remaining, 0, &ctx);
        if remaining.is_empty() {
            break;
        }
    }
    progress.finish();

    // Anything still in `remaining` was never found.
    let unused_keys: Vec<KeyLocation> = all_keys
        .into_iter()
        .filter(|loc| {
            key_to_idx
                .get(&loc.key)
                .map(|idx| remaining.contains(idx))
                .unwrap_or(true)
        })
        .collect();

    if unused_keys.is_empty() {
        eprintln!("No unused keys found.");
        return Ok(());
    }

    if clean {
        let changes = build_clean_changes(&unused_keys, &ctx.services)?;

        let use_color = color::should_use_color();
        let colors = ctx
            .config
            .as_ref()
            .map(|c| c.colors.clone())
            .unwrap_or_default();
        context::preview_and_apply(cli, &changes, &colors, use_color)
    } else {
        let use_color = color::should_use_color();
        let colors = ctx
            .config
            .as_ref()
            .map(|c| c.colors.clone())
            .unwrap_or_default();
        print_unused_keys(&unused_keys, &colors, use_color);
        Ok(())
    }
}

/// Print unused keys in the same hierarchical format as other commands.
fn print_unused_keys(keys: &[KeyLocation], colors: &crate::color::ColorConfig, use_color: bool) {
    let mut by_service: BTreeMap<String, BTreeMap<String, Vec<&str>>> = BTreeMap::new();
    for loc in keys {
        by_service
            .entry(loc.service.clone())
            .or_default()
            .entry(loc.file.display.clone())
            .or_default()
            .push(&loc.key);
    }

    let mut total = 0;
    for (service, files) in &by_service {
        let service_count: usize = files.values().map(|k| k.len()).sum();
        total += service_count;

        eprintln!(
            "{} {}",
            color::colorize(&format!("{}/", service), colors.service_root, use_color),
            color::colorize(
                &format!("({})", service_count),
                colors.service_root,
                use_color
            )
        );

        let file_count = files.len();
        for (i, (file, file_keys)) in files.iter().enumerate() {
            let is_last_file = i + 1 == file_count;
            let branch = if is_last_file {
                "└── "
            } else {
                "├── "
            };
            let pipe = if is_last_file { "    " } else { "│   " };

            eprintln!(
                "{}{} {}",
                color::colorize(branch, colors.service_root, use_color),
                color::colorize(file, colors.file, use_color),
                color::colorize(&format!("({})", file_keys.len()), colors.file, use_color)
            );

            for (j, key) in file_keys.iter().enumerate() {
                let is_last_key = j + 1 == file_keys.len();
                let key_branch = if is_last_key {
                    "└── "
                } else {
                    "├── "
                };
                eprintln!(
                    "{}{}{}",
                    color::colorize(pipe, colors.service_root, use_color),
                    color::colorize(key_branch, colors.file, use_color),
                    color::colorize(key, colors.key, use_color)
                );
            }
        }
    }

    eprintln!(
        "\n{}",
        color::colorize(
            &format!("{} unused key(s) found.", total),
            colors.service_root,
            use_color
        )
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_boundary_exact_match() {
        assert!(is_word_boundary("DB_HOST=localhost", 0, 7));
        assert!(!is_word_boundary("DB_HOST=localhost", 0, 2)); // "DB"
        assert!(!is_word_boundary("DB_HOST=localhost", 3, 7)); // "HOST"
    }

    #[test]
    fn word_boundary_with_special_chars() {
        assert!(is_word_boundary("export API_KEY=xxx", 7, 14));
        assert!(is_word_boundary("API_KEY: value", 0, 7));
        assert!(is_word_boundary("# API_KEY comment", 2, 9));
    }

    #[test]
    fn word_boundary_underscore() {
        assert!(is_word_boundary("FOO_BAR=value", 0, 7));
        // FOO_BAR is a prefix of FOO_BAR_BAZ → not a whole word
        assert!(!is_word_boundary("FOO_BAR_BAZ=value", 0, 7));
    }

    #[test]
    fn word_boundary_at_edges() {
        assert!(is_word_boundary("DB_HOST=", 0, 7));
        assert!(is_word_boundary("=DB_HOST", 1, 8));
        assert!(is_word_boundary(" DB_HOST ", 1, 8));
    }

    #[test]
    fn build_skip_dirs_merges_defaults() {
        let config_skip = vec!["dist".to_string(), "build".to_string()];
        let skip = build_skip_dirs(&config_skip);
        assert!(skip.contains(".git"));
        assert!(skip.contains("target"));
        assert!(skip.contains("vendor"));
        assert!(skip.contains("node_modules"));
        assert!(skip.contains("logs"));
        assert!(skip.contains("dist"));
        assert!(skip.contains("build"));
    }

    #[test]
    fn build_skip_files_merges_defaults() {
        let config_skip = vec!["custom.js".to_string(), "test.ts".to_string()];
        let skip = build_skip_files(&config_skip);
        assert!(skip.contains("custom.js"));
        assert!(skip.contains("test.ts"));
        assert_eq!(skip.len(), 2);
    }

    #[test]
    fn is_probably_text_known_extensions() {
        assert!(is_probably_text(Path::new("src/main.rs")));
        assert!(is_probably_text(Path::new("config.toml")));
        assert!(!is_probably_text(Path::new("lib.so")));
        assert!(!is_probably_text(Path::new("image.png")));
    }
}
