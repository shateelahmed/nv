//! `configmap*.yml` / `secrets*.yml` parsing and indentation-aware editing.
//!
//! Two shapes are supported and auto-detected:
//! * **Kubernetes manifests** — env keys live under a top-level `data:` or
//!   `stringData:` block mapping.
//! * **Flat key-value YAML** — env keys are top-level scalars.
//!
//! Values are treated as raw strings; no base64 encoding/decoding is performed.
//!
//! Like the dotenv editor, we never round-trip the whole document through a
//! YAML library (that would drop comments and reorder keys). Instead we scan
//! the raw lines, using indentation to understand nesting, and rewrite only the
//! one line that holds the target value.

use std::borrow::Cow;

use super::{ParsedComment, ParsedPair};

/// Names of the top-level blocks that hold env keys in k8s manifests.
/// Order matters: when creating a brand-new key we prefer `stringData`.
const K8S_BLOCKS: [&str; 2] = ["stringData", "data"];

/// Count leading space characters (indentation) of a line. Indentation is how
/// YAML expresses nesting, so it is central to everything here.
fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// Parse a mapping line into `(indent, key, value)` if it is a `key: value`
/// entry. Ignores comments, list items, and blanks.
fn mapping_of(line: &str) -> Option<(usize, String, String)> {
    let indent = indent_of(line);
    let content = line[indent..].trim_end();
    if content.is_empty() || content.starts_with('#') || content.starts_with("- ") {
        return None;
    }
    let colon = find_key_colon(content)?;
    let key = content[..colon].trim();
    if key.is_empty() {
        return None;
    }
    let value = strip_inline_comment(content[colon + 1..].trim());
    Some((indent, key.to_string(), unquote(value).into_owned()))
}

/// Find the index of the colon that separates a mapping key from its value,
/// skipping colons inside quotes.
///
/// We track whether we are inside single or double quotes so that a colon in a
/// value like `url: "http://x"` is not mistaken for the key separator.
fn find_key_colon(content: &str) -> Option<usize> {
    let bytes = content.as_bytes();
    let mut in_single = false;
    let mut in_double = false;
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'\'' if !in_double => in_single = !in_single,
            b'"' if !in_single => in_double = !in_double,
            b':' if !in_single && !in_double
                // A key colon is followed by end-of-line or whitespace.
                && (i + 1 == bytes.len() || bytes[i + 1] == b' ' || bytes[i + 1] == b'\t') =>
            {
                return Some(i);
            }
            _ => {}
        }
    }
    None
}

/// Remove an unquoted trailing `# comment` from a scalar value.
fn strip_inline_comment(value: &str) -> &str {
    let bytes = value.as_bytes();
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
                return value[..i].trim_end();
            }
            _ => {}
        }
    }
    value
}

/// Strip a single layer of matching quotes from a scalar.
fn unquote(value: &str) -> Cow<'_, str> {
    let b = value.as_bytes();
    if b.len() >= 2 {
        let (f, l) = (b[0], b[b.len() - 1]);
        if (f == b'"' && l == b'"') || (f == b'\'' && l == b'\'') {
            return Cow::Owned(value[1..value.len() - 1].to_string());
        }
    }
    Cow::Borrowed(value)
}

/// Format a Rust string as a YAML scalar, quoting only when necessary.
///
/// YAML has many characters that change meaning at the start of a value, plus
/// words like `true`/`no` and bare numbers that would be read as booleans or
/// numbers. When any of those apply we wrap the value in double quotes so it is
/// always interpreted as a plain string.
fn yaml_scalar(value: &str) -> String {
    if value.is_empty() {
        return "\"\"".to_string();
    }
    let needs_quote = value.starts_with(|c: char| c.is_whitespace())
        || value.ends_with(|c: char| c.is_whitespace())
        || value.starts_with([
            '#', '&', '*', '!', '|', '>', '%', '@', '`', '"', '\'', '[', '{', '?', ',', '-',
        ])
        || value.contains(": ")
        || value.contains(" #")
        || value.ends_with(':')
        || is_reserved_word(value)
        || looks_numeric(value);
    if needs_quote {
        let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
        format!("\"{escaped}\"")
    } else {
        value.to_string()
    }
}

fn is_reserved_word(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "true" | "false" | "yes" | "no" | "on" | "off" | "null" | "~"
    )
}

fn looks_numeric(value: &str) -> bool {
    value.parse::<f64>().is_ok()
}

/// Whether the document uses the Kubernetes shape: returns the top-level indent
/// and the line index of the first present block among [`K8S_BLOCKS`].
///
/// We look for a top-level (indent 0) line like `data:` with no value after it,
/// which signals a nested block of keys follows.
fn find_k8s_block(lines: &[String], block: &str) -> Option<usize> {
    lines.iter().position(
        |line| matches!(mapping_of(line), Some((0, ref k, ref v)) if k == block && v.is_empty()),
    )
}

/// True if the document has at least one k8s block (`data:` or `stringData:`).
fn is_k8s(lines: &[String]) -> bool {
    K8S_BLOCKS
        .iter()
        .any(|b| find_k8s_block(lines, b).is_some())
}

/// Parse env keys from YAML `content`.
pub fn parse(content: &str) -> Vec<ParsedPair> {
    let lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();

    if is_k8s(&lines) {
        // Kubernetes shape: collect the children under each block.
        let mut out = Vec::new();
        for block in K8S_BLOCKS {
            if let Some(start) = find_k8s_block(&lines, block) {
                out.extend(collect_block_children(&lines, start));
            }
        }
        out
    } else {
        // Flat shape: every non-empty top-level `key: value` is an env key.
        lines
            .iter()
            .filter_map(|l| mapping_of(l))
            .filter(|(indent, _, value)| *indent == 0 && !value.is_empty())
            .map(|(_, key, value)| ParsedPair { key, value })
            .collect()
    }
}

/// Collect scalar children of a block starting at `block_line`.
///
/// Children are the following lines that are indented *more* than the block
/// header; the first line indented the same or less ends the block.
fn collect_block_children(lines: &[String], block_line: usize) -> Vec<ParsedPair> {
    let block_indent = indent_of(&lines[block_line]);
    let mut out = Vec::new();
    for line in &lines[block_line + 1..] {
        if line.trim().is_empty() {
            continue;
        }
        let indent = indent_of(line);
        if indent <= block_indent {
            break; // dedented back out of the block
        }
        if let Some((_, key, value)) = mapping_of(line) {
            out.push(ParsedPair { key, value });
        }
    }
    out
}

/// Parse the comment attached to each key in YAML `content`.
///
/// Works for both the flat and Kubernetes shapes. A key's comment is the
/// consecutive prose `#` lines directly above it (top-level for the flat
/// shape, inside the block at child indentation for the k8s shape) plus the
/// inline `# comment` on its own line, normalized and joined with single
/// spaces. A commented-out mapping line (`# KEY: value`) never attaches to a
/// key — it is compared among the "other comments" instead — and it breaks
/// any comment block above a key. Keys without any comment are omitted.
/// Comments that are not attached to a key (e.g. a header above `data:`) are
/// ignored here; they flow into the "other comments" pool.
pub fn parse_comments(content: &str) -> Vec<ParsedComment> {
    let lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();

    if is_k8s(&lines) {
        let mut out = Vec::new();
        for block in K8S_BLOCKS {
            if let Some(start) = find_k8s_block(&lines, block) {
                out.extend(collect_block_comments(&lines, start));
            }
        }
        out
    } else {
        collect_flat_comments(&lines)
    }
}

/// Collect the comment attached to each top-level key of a flat mapping.
fn collect_flat_comments(lines: &[String]) -> Vec<ParsedComment> {
    let mut pending: Vec<String> = Vec::new();
    let mut out = Vec::new();

    for line in lines {
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            pending.clear();
        } else if trimmed.starts_with('#') {
            // Only top-level comment lines attach to top-level keys.
            if indent_of(line) == 0 {
                let text = super::comment_text(line);
                if is_commented_out_assignment(&text) {
                    pending.clear();
                } else {
                    pending.push(text);
                }
            } else {
                pending.clear();
            }
        } else if let Some((0, key, value)) = mapping_of(line)
            && !value.is_empty()
        {
            push_comment(&mut out, key, &mut pending, line);
        } else {
            pending.clear();
        }
    }

    out
}

/// Collect the comment attached to each scalar child of a block.
///
/// Children are the lines indented more than the block header; the first line
/// indented the same or less ends the block. Comment lines at child
/// indentation attach to the next child; a commented-out mapping breaks the
/// block and stays among the "other comments". Comments at other indentation
/// are dropped (block-level documentation).
fn collect_block_comments(lines: &[String], block_line: usize) -> Vec<ParsedComment> {
    let block_indent = indent_of(&lines[block_line]);
    let mut pending: Vec<String> = Vec::new();
    let mut out = Vec::new();

    for line in &lines[block_line + 1..] {
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            pending.clear();
            continue;
        }
        let indent = indent_of(line);
        if indent <= block_indent {
            break; // dedented back out of the block
        }
        if trimmed.starts_with('#') {
            let text = super::comment_text(line);
            if is_commented_out_assignment(&text) {
                pending.clear();
            } else {
                pending.push(text);
            }
            continue;
        }
        if let Some((_, key, _)) = mapping_of(line) {
            push_comment(&mut out, key, &mut pending, line);
        } else {
            pending.clear();
        }
    }

    out
}

/// Whether `text` (a comment's content) is a commented-out mapping line.
///
/// Such lines look like `KEY: value` and are treated as standalone comments,
/// never as a key's documentation. Prose containing a colon (e.g.
/// `Note: this is a comment`) is structurally indistinguishable and is treated
/// the same way.
fn is_commented_out_assignment(text: &str) -> bool {
    mapping_of(text).is_some()
}

/// Append a key's combined comment (pending block + inline) to `out` if the
/// key has any comment text, and clear the pending block.
fn push_comment(out: &mut Vec<ParsedComment>, key: String, pending: &mut Vec<String>, line: &str) {
    let mut comment = pending.join(" ");
    let mut lines = pending.clone();
    if let Some(inline) = super::inline_comment_text(line) {
        if !comment.is_empty() {
            comment.push(' ');
        }
        comment.push_str(&inline);
        lines.push(inline);
    }
    if !comment.is_empty() {
        out.push(ParsedComment {
            key,
            comment,
            lines,
        });
    }
    pending.clear();
}

/// Set `key` to `value`, editing in place or creating it, preserving all other
/// formatting.
pub fn set_value(content: &str, key: &str, value: &str) -> String {
    let newline = crate::parser::detect_newline(content);
    let ends_with_newline = content.ends_with('\n') || content.is_empty();
    let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();

    if is_k8s(&lines) {
        set_in_k8s(&mut lines, key, value);
    } else {
        set_flat(&mut lines, key, value);
    }

    let mut out = lines.join(newline);
    if ends_with_newline || out.is_empty() {
        out.push_str(newline);
    }
    out
}

/// Remove `key` from YAML `content`, preserving all other formatting.
///
/// Works for both flat and Kubernetes-style YAML. If the key is not present,
/// the content is returned unchanged.
pub fn remove_key(content: &str, key: &str) -> String {
    let newline = crate::parser::detect_newline(content);
    let ends_with_newline = content.ends_with('\n') || content.is_empty();
    let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();

    let mut removed = false;
    lines.retain(|line| {
        if removed {
            return true;
        }
        if let Some((_, k, _)) = mapping_of(line)
            && k == key
        {
            removed = true;
            return false;
        }
        true
    });

    let mut out = lines.join(newline);
    if ends_with_newline || out.is_empty() {
        out.push_str(newline);
    }
    out
}

/// Edit or insert a key in a flat top-level mapping.
fn set_flat(lines: &mut Vec<String>, key: &str, value: &str) {
    // First try to find and overwrite an existing top-level key.
    for line in lines.iter_mut() {
        if let Some((0, k, _)) = mapping_of(line)
            && k == key
        {
            *line = format!("{key}: {}", yaml_scalar(value));
            return;
        }
    }
    // Not present: add it as a new top-level line.
    lines.push(format!("{key}: {}", yaml_scalar(value)));
}

/// Edit or insert a key under the first matching k8s block.
///
/// This runs in two passes. Pass 1 tries to overwrite the key wherever it
/// already exists. Only if it is nowhere to be found does pass 2 insert a brand
/// new line under the preferred block.
fn set_in_k8s(lines: &mut Vec<String>, key: &str, value: &str) {
    // Pass 1: prefer editing an existing key wherever it already lives.
    for block in K8S_BLOCKS {
        if let Some(start) = find_k8s_block(lines, block) {
            let block_indent = indent_of(&lines[start]);
            // Find where this block ends (first line that dedents to/below it).
            let mut end = lines.len();
            for (offset, line) in lines[start + 1..].iter().enumerate() {
                if line.trim().is_empty() {
                    continue;
                }
                if indent_of(line) <= block_indent {
                    end = start + 1 + offset;
                    break;
                }
            }
            for line in lines.iter_mut().take(end).skip(start + 1) {
                if let Some((_, k, _)) = mapping_of(line)
                    && k == key
                {
                    let child_indent = indent_of(line);
                    *line = format!("{}{key}: {}", " ".repeat(child_indent), yaml_scalar(value));
                    return;
                }
            }
        }
    }

    // Not found: insert under the first present block (prefer stringData).
    for block in K8S_BLOCKS {
        if let Some(start) = find_k8s_block(lines, block) {
            let block_indent = indent_of(&lines[start]);
            // Figure out the right line and indentation for the new entry.
            let (insert_at, child_indent) = block_insert_point(lines, start, block_indent);
            lines.insert(
                insert_at,
                format!("{}{key}: {}", " ".repeat(child_indent), yaml_scalar(value)),
            );
            return;
        }
    }
}

/// Determine where to insert a new child and at what indent, based on existing
/// children of the block.
fn block_insert_point(lines: &[String], start: usize, block_indent: usize) -> (usize, usize) {
    let mut child_indent = block_indent + 2;
    let mut last_child = start;
    let mut seen_child = false;
    for (offset, line) in lines[start + 1..].iter().enumerate() {
        let idx = start + 1 + offset;
        if line.trim().is_empty() {
            continue;
        }
        let indent = indent_of(line);
        if indent <= block_indent {
            break;
        }
        if !seen_child {
            child_indent = indent;
            seen_child = true;
        }
        last_child = idx;
    }
    (last_child + 1, child_indent)
}

/// Return `content` with its keys reordered to follow `order`.
///
/// Flat YAML files reorder their top-level keys; Kubernetes manifests reorder
/// the scalar children of each `stringData:`/`data:` block independently,
/// never moving a key between blocks. Keys absent from `order` move to the
/// bottom, keeping their relative order. Each key travels with its attached
/// comment block and trailing blank lines; every other line stays exactly where
/// it is. Values are never rewritten.
pub fn reorder(content: &str, order: &[String]) -> String {
    let newline = crate::parser::detect_newline(content);
    let ends_with_newline = content.ends_with('\n');
    let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();

    lines = if is_k8s(&lines) {
        reorder_k8s(&lines, order)
    } else {
        super::reorder::reorder_region(
            &lines,
            flat_key_of,
            flat_comment_attaches,
            |text| mapping_of(text).is_some(),
            order,
        )
    };

    let mut out = lines.join(newline);
    if !content.is_empty() && ends_with_newline {
        out.push_str(newline);
    }
    out
}

/// The key of a flat top-level mapping line, or `None` for anything else.
///
/// Matches the flat parser: only top-level (indent 0) entries with a non-empty
/// value count as keys.
fn flat_key_of(line: &str) -> Option<String> {
    mapping_of(line).and_then(|(indent, key, value)| {
        if indent == 0 && !value.is_empty() {
            Some(key)
        } else {
            None
        }
    })
}

/// Whether a full-line comment in a flat file can attach to a key (only
/// top-level comments do, matching the comment parser).
fn flat_comment_attaches(line: &str) -> bool {
    indent_of(line) == 0
}

/// The key of a k8s block's direct child at `child_indent`, or `None`.
///
/// Only direct children reorder; deeper-indented (nested) content stays fixed.
fn k8s_key_of(line: &str, child_indent: usize) -> Option<String> {
    mapping_of(line).and_then(|(indent, key, _)| {
        if indent == child_indent {
            Some(key)
        } else {
            None
        }
    })
}

/// Reorder each k8s block's children independently, leaving every line outside
/// the blocks untouched.
fn reorder_k8s(lines: &[String], order: &[String]) -> Vec<String> {
    // Locate every present block and process them in file order so their
    // regions never overlap.
    let mut starts: Vec<usize> = K8S_BLOCKS
        .iter()
        .filter_map(|b| find_k8s_block(lines, b))
        .collect();
    starts.sort_unstable();

    let mut result: Vec<String> = Vec::with_capacity(lines.len());
    let mut cursor = 0usize;
    for start in starts {
        // Copy everything up to and including the block header line.
        result.extend(lines[cursor..=start].iter().cloned());
        let (region, child_indent) = block_region(lines, start);
        let reordered = super::reorder::reorder_region(
            &region,
            |l| k8s_key_of(l, child_indent),
            |l| indent_of(l) == child_indent,
            |text| mapping_of(text).is_some(),
            order,
        );
        result.extend(reordered);
        cursor = start + 1 + region.len();
    }
    result.extend(lines[cursor..].iter().cloned());
    result
}

/// The child lines under the block at `block_line`, plus the indentation of its
/// direct children (the first child's indent, or `block_indent + 2` when the
/// block is empty).
fn block_region(lines: &[String], block_line: usize) -> (Vec<String>, usize) {
    let block_indent = indent_of(&lines[block_line]);
    let mut child_indent = block_indent + 2;
    let mut end = lines.len();
    let mut seen_child = false;
    for (offset, line) in lines[block_line + 1..].iter().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let indent = indent_of(line);
        if indent <= block_indent {
            end = block_line + 1 + offset;
            break;
        }
        if !seen_child {
            child_indent = indent;
            seen_child = true;
        }
    }
    (lines[block_line + 1..end].to_vec(), child_indent)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_flat_yaml() {
        let content = "# cfg\nFOO: bar\nBAZ: \"q u x\"\n";
        let pairs = parse(content);
        assert_eq!(pairs.len(), 2);
        assert_eq!(
            pairs[0],
            ParsedPair {
                key: "FOO".into(),
                value: "bar".into()
            }
        );
        assert_eq!(
            pairs[1],
            ParsedPair {
                key: "BAZ".into(),
                value: "q u x".into()
            }
        );
    }

    #[test]
    fn parses_k8s_manifest() {
        let content = "apiVersion: v1\nkind: ConfigMap\ndata:\n  FOO: bar\n  BAZ: qux\n";
        let pairs = parse(content);
        assert_eq!(pairs.len(), 2);
        assert_eq!(
            pairs[0],
            ParsedPair {
                key: "FOO".into(),
                value: "bar".into()
            }
        );
        assert_eq!(
            pairs[1],
            ParsedPair {
                key: "BAZ".into(),
                value: "qux".into()
            }
        );
    }

    #[test]
    fn edits_flat_in_place() {
        let content = "# top\nFOO: old\nBAR: keep\n";
        let out = set_value(content, "FOO", "new");
        assert_eq!(out, "# top\nFOO: new\nBAR: keep\n");
    }

    #[test]
    fn appends_flat_missing_key() {
        let out = set_value("FOO: bar\n", "NEW", "value");
        assert_eq!(out, "FOO: bar\nNEW: value\n");
    }

    #[test]
    fn edits_k8s_child_preserving_indent_and_siblings() {
        let content = "kind: ConfigMap\ndata:\n  FOO: old\n  BAR: keep\n";
        let out = set_value(content, "FOO", "new");
        assert_eq!(out, "kind: ConfigMap\ndata:\n  FOO: new\n  BAR: keep\n");
    }

    #[test]
    fn inserts_k8s_child_under_block() {
        let content = "kind: ConfigMap\ndata:\n  FOO: bar\n";
        let out = set_value(content, "NEW", "value");
        assert_eq!(out, "kind: ConfigMap\ndata:\n  FOO: bar\n  NEW: value\n");
    }

    #[test]
    fn prefers_stringdata_block_for_new_key() {
        let content = "kind: Secret\ndata:\n  A: x\nstringData:\n  B: y\n";
        let out = set_value(content, "NEW", "value");
        // stringData is preferred, key inserted at end of that block.
        assert_eq!(
            out,
            "kind: Secret\ndata:\n  A: x\nstringData:\n  B: y\n  NEW: value\n"
        );
    }

    #[test]
    fn empty_value_quoted() {
        let out = set_value("data:\n  FOO: old\n", "FOO", "");
        assert_eq!(out, "data:\n  FOO: \"\"\n");
    }

    #[test]
    fn quotes_ambiguous_scalars() {
        assert_eq!(yaml_scalar("true"), "\"true\"");
        assert_eq!(yaml_scalar("123"), "\"123\"");
        assert_eq!(yaml_scalar("a: b"), "\"a: b\"");
        assert_eq!(yaml_scalar("plain"), "plain");
    }

    #[test]
    fn comments_flat_block_and_inline() {
        let content = "# DB connection\nDATABASE_URL: postgres://x # prod\nFOO: bar\n";
        let comments = parse_comments(content);
        assert_eq!(comments.len(), 1);
        assert_eq!(
            comments[0],
            ParsedComment {
                key: "DATABASE_URL".into(),
                comment: "DB connection prod".into(),
                lines: vec!["DB connection".into(), "prod".into()]
            }
        );
    }

    #[test]
    fn comments_flat_header_attaches_to_first_key() {
        // A top-level header directly above a key is that key's comment.
        let content = "# Auth service\nA: 1\nB: 2\n";
        let comments = parse_comments(content);
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].key, "A");
        assert_eq!(comments[0].comment, "Auth service");
    }

    #[test]
    fn comments_flat_blank_line_breaks_block() {
        let content = "# orphan\n\nA: 1\n";
        let comments = parse_comments(content);
        assert!(comments.is_empty());
    }

    #[test]
    fn comments_k8s_child_block() {
        let content =
            "kind: ConfigMap\ndata:\n  # DB config\n  DATABASE_URL: x # prod\n  FOO: bar\n";
        let comments = parse_comments(content);
        assert_eq!(comments.len(), 1);
        assert_eq!(
            comments[0],
            ParsedComment {
                key: "DATABASE_URL".into(),
                comment: "DB config prod".into(),
                lines: vec!["DB config".into(), "prod".into()]
            }
        );
    }

    #[test]
    fn comments_k8s_header_ignored() {
        // The header sits above `data:` at top level, not inside the block.
        let content = "# Auth env\nkind: ConfigMap\ndata:\n  A: 1\n";
        let comments = parse_comments(content);
        assert!(comments.is_empty());
    }

    #[test]
    fn comments_flat_commented_out_assignment_not_attached() {
        let content = "# REDIS_ENTERPRISE_HOST: redis-enterprise\nDATABASE_URL: x\n";
        let comments = parse_comments(content);
        assert!(comments.is_empty());
    }

    #[test]
    fn comments_flat_commented_out_assignment_breaks_block() {
        let content =
            "# DB connection\n# REDIS_ENTERPRISE_HOST: redis-enterprise\nDATABASE_URL: x\n";
        let comments = parse_comments(content);
        assert!(comments.is_empty());
    }

    #[test]
    fn comments_k8s_commented_out_assignment_not_attached() {
        let content = "kind: ConfigMap\ndata:\n  # REDIS_ENTERPRISE_HOST: redis-enterprise\n  DATABASE_URL: x\n";
        let comments = parse_comments(content);
        assert!(comments.is_empty());
    }

    #[test]
    fn comments_k8s_block_children_are_not_shared_across_blocks() {
        let content =
            "kind: Secret\ndata:\n  # DB config\n  DATABASE_URL: x\nstringData:\n  TOKEN: y\n";
        let comments = parse_comments(content);
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].key, "DATABASE_URL");
    }

    fn reorder(content: &str, order: &[&str]) -> String {
        let order: Vec<String> = order.iter().map(|s| s.to_string()).collect();
        super::reorder(content, &order)
    }

    #[test]
    fn reorder_flat_yaml_keys() {
        let content = "C: 3\nA: 1\nB: 2\n";
        let out = reorder(content, &["A", "B", "C"]);
        assert_eq!(out, "A: 1\nB: 2\nC: 3\n");
    }

    #[test]
    fn reorder_flat_yaml_moves_attached_comment() {
        let content = "# doc C\nC: 3\nB: 2\nA: 1\n";
        let out = reorder(content, &["A", "B", "C"]);
        assert_eq!(out, "A: 1\nB: 2\n# doc C\nC: 3\n");
    }

    #[test]
    fn reorder_flat_yaml_extras_to_bottom() {
        let content = "X: 9\nC: 3\nA: 1\nY: 8\n";
        let out = reorder(content, &["A", "C"]);
        assert_eq!(out, "A: 1\nC: 3\nX: 9\nY: 8\n");
    }

    #[test]
    fn reorder_flat_yaml_empty_value_key_stays_in_place() {
        // Empty-value entries are not recognized as keys, so they are pinned.
        let content = "C: 3\nEMPTY:\nA: 1\n";
        let out = reorder(content, &["A", "C"]);
        assert_eq!(out, "A: 1\nEMPTY:\nC: 3\n");
    }

    #[test]
    fn reorder_flat_yaml_already_in_order_is_noop() {
        let content = "A: 1\nB: 2\n";
        let out = reorder(content, &["A", "B"]);
        assert_eq!(out, content);
    }

    #[test]
    fn reorder_k8s_block_children() {
        let content = "kind: ConfigMap\ndata:\n  C: 3\n  A: 1\n  B: 2\n";
        let out = reorder(content, &["A", "B", "C"]);
        assert_eq!(out, "kind: ConfigMap\ndata:\n  A: 1\n  B: 2\n  C: 3\n");
    }

    #[test]
    fn reorder_k8s_reorders_each_block_independently() {
        let content = "kind: Secret\nstringData:\n  B: 2\n  A: 1\ndata:\n  D: 4\n  C: 3\n";
        let out = reorder(content, &["A", "B", "C", "D"]);
        assert_eq!(
            out,
            "kind: Secret\nstringData:\n  A: 1\n  B: 2\ndata:\n  C: 3\n  D: 4\n"
        );
    }

    #[test]
    fn reorder_k8s_children_never_cross_blocks() {
        // The base order ranks data children after stringData children, but each
        // block only reorders its own keys; nothing crosses into another block.
        let content = "kind: Secret\ndata:\n  C: 3\n  A: 1\nstringData:\n  D: 4\n";
        let out = reorder(content, &["A", "B", "C", "D"]);
        assert_eq!(
            out,
            "kind: Secret\ndata:\n  A: 1\n  C: 3\nstringData:\n  D: 4\n"
        );
    }

    #[test]
    fn reorder_k8s_block_level_comment_stays() {
        // The header above `data:` is outside the block region and untouched.
        let content = "kind: ConfigMap\n# header\ndata:\n  C: 3\n  A: 1\n";
        let out = reorder(content, &["A", "C"]);
        assert_eq!(out, "kind: ConfigMap\n# header\ndata:\n  A: 1\n  C: 3\n");
    }

    #[test]
    fn reorder_k8s_attached_comment_moves_with_child() {
        let content = "kind: ConfigMap\ndata:\n  # doc C\n  C: 3\n  A: 1\n";
        let out = reorder(content, &["A", "C"]);
        assert_eq!(out, "kind: ConfigMap\ndata:\n  A: 1\n  # doc C\n  C: 3\n");
    }

    #[test]
    fn reorder_k8s_blank_separator_travels_with_the_unit() {
        // The blank after C belongs to C's unit, so it ends up after C again.
        let content = "kind: ConfigMap\ndata:\n  C: 3\n\n  A: 1\n";
        let out = reorder(content, &["A", "C"]);
        assert_eq!(out, "kind: ConfigMap\ndata:\n  A: 1\n  C: 3\n\n");
    }
}
