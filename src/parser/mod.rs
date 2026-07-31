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

    if name == ".env" || name.starts_with(".env.") {
        // `.env` and `.env.<suffix>` are dotenv files.
        if name.ends_with('~')
            || name.ends_with(".swp")
            || name.ends_with(".swo")
            || name.ends_with(".bak")
            || name.ends_with(".tmp")
        {
            // Swap/backup artifacts are
            // excluded so that editor temporaries don't pollute the file list.
            return None;
        }
        if name.contains(".example") {
            return Some(FileKind::DotenvExample);
        }
        return Some(FileKind::Dotenv);
    }

    let is_yaml = name.ends_with(".yml")
        || name.ends_with(".yaml")
        || name.ends_with(".yml.example")
        || name.ends_with(".yaml.example");
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

/// A key and the comment attached to it in a file.
///
/// The comment combines the prose `#` comment lines directly above the key
/// with an optional inline `# comment` on the key's own line. `comment` holds
/// the lines joined with single spaces (so a block or an inline rendering of
/// the same text compares equal); `lines` holds each normalized line
/// individually so `nv compare` can remove exactly the consumed lines from the
/// file's "other comments" pool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedComment {
    pub key: String,
    pub comment: String,
    pub lines: Vec<String>,
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

/// Parse every comment in `content` into comparable, normalized text.
///
/// A comment is either a full line whose trimmed start is `#` or an inline
/// `# comment` trailing a value line. Each is normalized via [`comment_text`]
/// / [`inline_comment_text`] (leading `#` markers and whitespace stripped) and
/// collected in file order. The extraction is identical for dotenv and YAML
/// files, so it is implemented here rather than per sub-parser.
pub fn parse_comments(content: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with('#') {
            out.push(comment_text(line));
        } else if let Some(inline) = inline_comment_text(line) {
            out.push(inline);
        }
    }
    out
}

/// Parse the comment attached to each key in file `content` of the given `kind`.
///
/// Only keys that have a comment produce an entry; keys without any comment
/// are absent. Commented-out assignment lines (`# KEY=value` / `# KEY: value`)
/// never attach to a key — they are compared among the "other comments"
/// instead — and they break any comment block above a key. Delegates to the
/// YAML or dotenv parser depending on the file kind.
pub fn parse_attached_comments(content: &str, kind: FileKind) -> Vec<ParsedComment> {
    if kind.is_yaml() {
        yaml::parse_comments(content)
    } else {
        dotenv::parse_comments(content)
    }
}

/// Normalize a full-line comment into comparable text: strip indentation, all
/// leading `#` markers, and surrounding whitespace. `"  # DB config "` becomes
/// `"DB config"`.
pub(crate) fn comment_text(line: &str) -> String {
    line.trim_start().trim_start_matches('#').trim().to_string()
}

/// Extract the trailing `# comment` text from an assignment/mapping line.
///
/// Returns `None` when there is no inline comment. The `#` must be preceded by
/// whitespace (space or tab) and must not sit inside single or double quotes,
/// matching how YAML inline comments are delimited. Full-line comments are
/// handled separately by [`parse_comments`] before this is consulted.
pub(crate) fn inline_comment_text(line: &str) -> Option<String> {
    let bytes = line.as_bytes();
    let mut in_single = false;
    let mut in_double = false;
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'\'' if !in_double => in_single = !in_single,
            b'"' if !in_single => in_double = !in_double,
            b'#' if !in_single
                && !in_double
                && (i == 0 || bytes[i - 1] == b' ' || bytes[i - 1] == b'\t') =>
            {
                let text = line[i + 1..].trim();
                return if text.is_empty() {
                    None
                } else {
                    Some(text.to_string())
                };
            }
            _ => {}
        }
    }
    None
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

/// Return new file content with `key` removed. All other bytes are preserved.
///
/// For dotenv files the assignment line is deleted. For YAML files the mapping
/// line is deleted. If the key is not present, content is returned unchanged.
pub fn remove_key(content: &str, kind: FileKind, key: &str) -> String {
    if kind.is_yaml() {
        yaml::remove_key(content, key)
    } else {
        dotenv::remove_key(content, key)
    }
}

/// Detect the line ending style of a text file.
///
/// Returns `"\r\n"` if the content contains CRLF, `"\n"` otherwise.
/// This avoids duplicating the `contains("\r\n")` check across sub-parsers.
pub fn detect_newline(content: &str) -> &str {
    if content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn kind(name: &str) -> Option<FileKind> {
        detect_kind(&PathBuf::from(name))
    }

    #[test]
    fn dotenv_exact() {
        assert_eq!(kind(".env"), Some(FileKind::Dotenv));
    }

    #[test]
    fn dotenv_local() {
        assert_eq!(kind(".env.local"), Some(FileKind::Dotenv));
    }

    #[test]
    fn dotenv_staging() {
        assert_eq!(kind(".env.staging"), Some(FileKind::Dotenv));
    }

    #[test]
    fn dotenv_foo_bar() {
        assert_eq!(kind(".env.foo.bar"), Some(FileKind::Dotenv));
    }

    #[test]
    fn dotenv_trailing_dot() {
        assert_eq!(kind(".env."), Some(FileKind::Dotenv));
    }

    #[test]
    fn dotenv_example_exact() {
        assert_eq!(kind(".env.example"), Some(FileKind::DotenvExample));
    }

    #[test]
    fn dotenv_testing_example() {
        assert_eq!(kind(".env.testing.example"), Some(FileKind::DotenvExample));
    }

    #[test]
    fn dotenv_staging_example() {
        assert_eq!(kind(".env.staging.example"), Some(FileKind::DotenvExample));
    }

    #[test]
    fn swap_files_excluded() {
        assert_eq!(kind(".env.swp"), None);
        assert_eq!(kind(".env.swo"), None);
    }

    #[test]
    fn backup_files_excluded() {
        assert_eq!(kind(".env~"), None);
        assert_eq!(kind(".env.bak"), None);
        assert_eq!(kind(".env.tmp"), None);
    }

    #[test]
    fn unrelated_files_ignored() {
        assert_eq!(kind("notes.txt"), None);
        assert_eq!(kind("Dockerfile"), None);
    }

    #[test]
    fn configmap_yaml() {
        assert_eq!(kind("configmap.yml"), Some(FileKind::ConfigMap));
        assert_eq!(kind("configmap-api.yaml"), Some(FileKind::ConfigMap));
    }

    #[test]
    fn secret_yaml() {
        assert_eq!(kind("secrets.yml"), Some(FileKind::Secret));
        assert_eq!(kind("secrets-db.yaml"), Some(FileKind::Secret));
    }

    #[test]
    fn secret_yaml_example() {
        assert_eq!(kind("secrets.yml.example"), Some(FileKind::Secret));
        assert_eq!(kind("secrets-db.yaml.example"), Some(FileKind::Secret));
    }

    #[test]
    fn configmap_yaml_example() {
        assert_eq!(kind("configmap.yml.example"), Some(FileKind::ConfigMap));
        assert_eq!(
            kind("configmap-api.yaml.example"),
            Some(FileKind::ConfigMap)
        );
    }

    #[test]
    fn comments_full_line_collected_in_order() {
        let comments = parse_comments("# first\n# second\nFOO=bar\n");
        assert_eq!(comments, vec!["first", "second"]);
    }

    #[test]
    fn comments_commented_out_key_is_collected() {
        // A commented-out assignment is still a comment.
        let comments = parse_comments("# REDIS_ENTERPRISE_HOST=redis-enterprise\nFOO=bar\n");
        assert_eq!(comments, vec!["REDIS_ENTERPRISE_HOST=redis-enterprise"]);
    }

    #[test]
    fn comments_inline_on_value_line() {
        let comments = parse_comments("FOO=bar # the foo\n");
        assert_eq!(comments, vec!["the foo"]);
    }

    #[test]
    fn comments_header_border_normalized() {
        let comments = parse_comments("#################### REDIS SETTINGS ####################\n");
        assert_eq!(comments, vec!["REDIS SETTINGS ####################"]);
    }

    #[test]
    fn comments_blank_lines_skipped() {
        let comments = parse_comments("\n\n# a\n\nFOO=bar\n\n");
        assert_eq!(comments, vec!["a"]);
    }

    #[test]
    fn comments_quoted_hash_ignored() {
        // The `#` inside quotes is not a comment; the trailing one is.
        let comments = parse_comments("FOO=\"a # b\" # real\n");
        assert_eq!(comments, vec!["real"]);
    }

    #[test]
    fn comments_hash_without_whitespace_not_inline() {
        let comments = parse_comments("FOO=abc#def\n");
        assert!(comments.is_empty());
    }

    #[test]
    fn comments_yaml_mapping_inline() {
        let comments = parse_comments("DATABASE_URL: postgres://x # prod\n");
        assert_eq!(comments, vec!["prod"]);
    }

    #[test]
    fn comments_yaml_k8s_comments_all_collected() {
        let content =
            "# Auth env\nkind: ConfigMap\ndata:\n  # DB config\n  DATABASE_URL: x # prod\n";
        let comments = parse_comments(content);
        assert_eq!(comments, vec!["Auth env", "DB config", "prod"]);
    }

    #[test]
    fn comments_duplicates_preserved() {
        let comments = parse_comments("# retry\nFOO=bar # retry\n");
        assert_eq!(comments, vec!["retry", "retry"]);
    }
}
