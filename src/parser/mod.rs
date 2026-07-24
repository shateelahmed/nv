//! File parsing and formatting-preserving editing.
//!
//! Reading is used to build the search index. Writing is deliberately
//! line-oriented so that comments, ordering, and surrounding formatting stay
//! byte-identical outside the single value being changed.
//!
//! This `mod.rs` is the parser module's front door: it picks the right
//! sub-parser (dotenv vs YAML) based on the file kind and exposes two simple
//! operations to the rest of the program: `parse` (read) and `set_value`
//! (edit). The actual logic lives in `dotenv.rs` and `yaml.rs`.

// These make `src/parser/dotenv.rs` and `src/parser/yaml.rs` part of this module.
pub mod dotenv;
pub mod yaml;

use std::path::Path;

use crate::model::FileKind;

/// Detect the [`FileKind`] of a file from its name.
///
/// Returns `None` for files `nv` does not manage. The `?` operator returns
/// early with `None` if `file_name()` or `to_str()` yields nothing.
pub fn detect_kind(path: &Path) -> Option<FileKind> {
    let name = path.file_name()?.to_str()?;

    if name == ".env" {
        return Some(FileKind::Dotenv);
    }
    if name == ".env.example" || name.starts_with(".env.example") {
        return Some(FileKind::DotenvExample);
    }
    // `.env.<something>` that is not `.example` is still treated as a dotenv.
    if name == ".env.local" || (name.starts_with(".env.") && !name.contains("example")) {
        return Some(FileKind::Dotenv);
    }

    let is_yaml = name.ends_with(".yml") || name.ends_with(".yaml");
    if is_yaml {
        if name.starts_with("configmap") {
            return Some(FileKind::ConfigMap);
        }
        if name.starts_with("secret") {
            return Some(FileKind::Secret);
        }
    }
    None
}

/// A parsed key/value pair read from a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedPair {
    pub key: String,
    pub value: String,
}

/// Parse the key/value pairs from file `content` of the given `kind`.
///
/// Delegates to the YAML or dotenv parser depending on the file kind.
pub fn parse(content: &str, kind: FileKind) -> Vec<ParsedPair> {
    if kind.is_yaml() {
        yaml::parse(content)
    } else {
        dotenv::parse(content)
    }
}

/// Return new file content with `key` set to `value`, creating the key if it
/// does not already exist. All other bytes are preserved.
///
/// Note this takes the current file text and returns the *new* text; it does
/// not touch the disk. Callers decide when/whether to write the result.
pub fn set_value(content: &str, kind: FileKind, key: &str, value: &str) -> String {
    if kind.is_yaml() {
        yaml::set_value(content, key, value)
    } else {
        dotenv::set_value(content, key, value)
    }
}
