//! `.env` / `.env.example` parsing and line-oriented editing.
//!
//! `.env` files are simple: one `KEY=value` per line, plus blank lines and
//! `#` comments. To preserve formatting we never rebuild the whole file; we
//! find the one line that defines a key and rewrite just that line.

use std::borrow::Cow;

use super::{ParsedComment, ParsedPair};

/// Split a dotenv line into an optional `(key, value)` pair.
///
/// Returns `None` for comments, blank lines, and anything that is not an
/// assignment. Supports an optional leading `export `. `Option<T>` means the
/// result is either `Some(value)` or `None`.
fn parse_line(line: &str) -> Option<(String, String)> {
    // `trim_start` drops leading spaces/tabs so indented lines still parse.
    let trimmed = line.trim_start();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None; // blank line or comment: not an assignment
    }
    // Allow `export KEY=value` (common in shell env files) by dropping the
    // prefix if present; otherwise keep the line as-is.
    let without_export = trimmed.strip_prefix("export ").unwrap_or(trimmed);
    // Find the first `=`. `?` returns `None` if there isn't one.
    let eq = without_export.find('=')?;
    let key = without_export[..eq].trim();
    if key.is_empty() || !is_valid_key(key) {
        return None;
    }
    let raw_value = &without_export[eq + 1..];
    Some((key.to_string(), unquote(raw_value.trim()).into_owned()))
}

/// A valid key starts with a letter or `_`, then letters/digits/`_`/`.`.
fn is_valid_key(key: &str) -> bool {
    let mut chars = key.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    key.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
}

/// Strip a single layer of matching single or double quotes.
fn unquote(value: &str) -> Cow<'_, str> {
    let bytes = value.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return Cow::Owned(value[1..value.len() - 1].to_string());
        }
    }
    Cow::Borrowed(value)
}

/// Quote a value for writing if it contains characters that would break a bare
/// dotenv assignment. Empty values are written as an empty string.
fn quote_if_needed(value: &str) -> String {
    if value.is_empty() {
        return String::new();
    }
    let needs_quote = value
        .chars()
        .any(|c| c.is_whitespace() || matches!(c, '#' | '"' | '\'' | '$' | '='));
    if needs_quote {
        let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
        format!("\"{escaped}\"")
    } else {
        value.to_string()
    }
}

/// Parse all assignments from dotenv `content`.
pub fn parse(content: &str) -> Vec<ParsedPair> {
    // This is an "iterator chain", read top to bottom:
    content
        .lines() // split the text into lines
        .filter_map(parse_line) // keep only lines that parse to a pair
        .map(|(key, value)| ParsedPair { key, value }) // convert to our struct
        .collect() // gather everything into a Vec
}

/// Parse the comment attached to each assignment in dotenv `content`.
///
/// A key's comment is the consecutive prose `#` lines directly above it (a
/// blank line, a non-comment line, or a commented-out assignment breaks the
/// block) plus the inline `# comment` on its own line, normalized and joined
/// with single spaces. A commented-out assignment line (`# KEY=value`) never
/// attaches to a key — it is compared among the "other comments" instead.
/// Keys without any comment are omitted.
pub fn parse_comments(content: &str) -> Vec<ParsedComment> {
    let mut pending: Vec<String> = Vec::new();
    let mut out = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            pending.clear();
        } else if trimmed.starts_with('#') {
            let text = super::comment_text(line);
            if is_commented_out_assignment(&text) {
                pending.clear();
            } else {
                pending.push(text);
            }
        } else if let Some((key, _)) = parse_line(line) {
            push_attached(&mut out, key, &mut pending, line);
        } else {
            pending.clear();
        }
    }

    out
}

/// Whether `text` (a comment's content) is a commented-out assignment.
///
/// Such lines look like `KEY=value` and are treated as standalone comments,
/// never as a key's documentation.
fn is_commented_out_assignment(text: &str) -> bool {
    parse_line(text).is_some()
}

/// Append a key's combined comment (pending block + inline) to `out` if the
/// key has any comment text, and clear the pending block.
fn push_attached(out: &mut Vec<ParsedComment>, key: String, pending: &mut Vec<String>, line: &str) {
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

/// Remove `key` from `content`, preserving comments and blank lines.
///
/// If the key is not present, the content is returned unchanged.
pub fn remove_key(content: &str, key: &str) -> String {
    let newline = crate::parser::detect_newline(content);
    let ends_with_newline = content.ends_with('\n') || content.is_empty();

    let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
    let mut removed = false;

    lines.retain(|line| {
        if removed {
            return true;
        }
        if let Some((k, _)) = parse_line(line)
            && k == key
        {
            removed = true;
            return false; // drop this line
        }
        true
    });

    let mut out = lines.join(newline);
    if ends_with_newline {
        out.push_str(newline);
    }
    out
}

/// Set `key` to `value`, editing the existing assignment line in place or
/// appending a new one. Comments and blank lines are preserved.
pub fn set_value(content: &str, key: &str, value: &str) -> String {
    // Match the file's existing line ending so we don't mix \n and \r\n.
    let newline = crate::parser::detect_newline(content);
    // Remember whether the original ended in a newline so we can restore it.
    // A brand-new (empty) file should end with one too.
    let ends_with_newline = content.ends_with('\n') || content.is_empty();

    // Work on an owned copy of each line so we can modify one in place.
    let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
    let mut found = false;

    for line in &mut lines {
        // If this line defines the key we're after, rewrite just its value.
        if let Some((k, _)) = parse_line(line)
            && k == key
        {
            *line = rewrite_line(line, value);
            found = true;
            break;
        }
    }

    // The key wasn't present anywhere, so create it on a new line at the end.
    if !found {
        lines.push(format!("{key}={}", quote_if_needed(value)));
    }

    let mut out = lines.join(newline);
    if ends_with_newline {
        out.push_str(newline);
    }
    out
}

/// Rebuild an assignment line with a new value, preserving indentation and an
/// optional `export ` prefix.
fn rewrite_line(line: &str, value: &str) -> String {
    let indent_len = line.len() - line.trim_start().len();
    let indent = &line[..indent_len];
    let rest = &line[indent_len..];
    let (prefix, body) = match rest.strip_prefix("export ") {
        Some(body) => ("export ", body),
        None => ("", rest),
    };
    let key = body.split('=').next().unwrap_or("").trim_end();
    format!("{indent}{prefix}{key}={}", quote_if_needed(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_and_exported() {
        let content = "# comment\nFOO=bar\nexport BAZ=qux\n\n";
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
    fn unquotes_values() {
        let pairs = parse("A=\"hello world\"\nB='single'\n");
        assert_eq!(pairs[0].value, "hello world");
        assert_eq!(pairs[1].value, "single");
    }

    #[test]
    fn edits_in_place_preserving_comments() {
        let content = "# top\nFOO=old # inline\nBAR=keep\n";
        let out = set_value(content, "FOO", "new");
        assert_eq!(out, "# top\nFOO=new\nBAR=keep\n");
    }

    #[test]
    fn preserves_export_and_indent() {
        let content = "  export FOO=old\n";
        let out = set_value(content, "FOO", "new");
        assert_eq!(out, "  export FOO=new\n");
    }

    #[test]
    fn appends_missing_key() {
        let content = "FOO=bar\n";
        let out = set_value(content, "NEW", "value");
        assert_eq!(out, "FOO=bar\nNEW=value\n");
    }

    #[test]
    fn quotes_values_with_spaces() {
        let out = set_value("", "KEY", "a b c");
        assert_eq!(out, "KEY=\"a b c\"\n");
    }

    #[test]
    fn empty_value_written_bare() {
        let out = set_value("KEY=old\n", "KEY", "");
        assert_eq!(out, "KEY=\n");
    }

    #[test]
    fn preserves_crlf() {
        let content = "FOO=old\r\nBAR=keep\r\n";
        let out = set_value(content, "FOO", "new");
        assert_eq!(out, "FOO=new\r\nBAR=keep\r\n");
    }

    #[test]
    fn comments_block_above_key() {
        let content = "# DB connection\nDATABASE_URL=postgres://x\n";
        let comments = parse_comments(content);
        assert_eq!(comments.len(), 1);
        assert_eq!(
            comments[0],
            ParsedComment {
                key: "DATABASE_URL".into(),
                comment: "DB connection".into(),
                lines: vec!["DB connection".into()]
            }
        );
    }

    #[test]
    fn comments_inline_on_own_line() {
        let comments = parse_comments("FOO=bar # the foo\n");
        assert_eq!(comments.len(), 1);
        assert_eq!(
            comments[0],
            ParsedComment {
                key: "FOO".into(),
                comment: "the foo".into(),
                lines: vec!["the foo".into()]
            }
        );
    }

    #[test]
    fn comments_block_plus_inline_join() {
        let content = "# DB\n# uses TLS\nDATABASE_URL=x # production\n";
        let comments = parse_comments(content);
        assert_eq!(comments.len(), 1);
        assert_eq!(
            comments[0],
            ParsedComment {
                key: "DATABASE_URL".into(),
                comment: "DB uses TLS production".into(),
                lines: vec!["DB".into(), "uses TLS".into(), "production".into()]
            }
        );
    }

    #[test]
    fn comments_blank_line_breaks_block() {
        let content = "# orphan\n\nFOO=bar\n";
        let comments = parse_comments(content);
        assert!(comments.is_empty());
    }

    #[test]
    fn comments_key_without_comment_omitted() {
        let content = "# doc\nA=1\nB=2\n";
        let comments = parse_comments(content);
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].key, "A");
    }

    #[test]
    fn comments_exported_line() {
        let comments = parse_comments("export FOO=bar # exported\n");
        assert_eq!(comments.len(), 1);
        assert_eq!(
            comments[0],
            ParsedComment {
                key: "FOO".into(),
                comment: "exported".into(),
                lines: vec!["exported".into()]
            }
        );
    }

    #[test]
    fn comments_hash_inside_value_is_not_inline() {
        let comments = parse_comments("FOO=abc#def\n");
        assert!(comments.is_empty());
    }

    #[test]
    fn comments_quoted_hash_ignored() {
        let comments = parse_comments("FOO=\"a # b\" # real\n");
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].comment, "real");
    }

    #[test]
    fn comments_commented_out_assignment_not_attached() {
        // Even directly above a real key, a commented-out assignment is not
        // the key's documentation — it stays among the "other comments".
        let content = "# REDIS_ENTERPRISE_HOST=redis-enterprise\nDATABASE_URL=x\n";
        let comments = parse_comments(content);
        assert!(comments.is_empty());
    }

    #[test]
    fn comments_commented_out_assignment_breaks_block() {
        // The commented-out line interrupts the prose block above the key.
        let content = "# DB connection\n# REDIS_ENTERPRISE_HOST=redis-enterprise\nDATABASE_URL=x\n";
        let comments = parse_comments(content);
        assert!(comments.is_empty());
    }

    #[test]
    fn comments_prose_after_commented_out_assignment_attaches() {
        // Prose below a commented-out line still attaches to the next key.
        let content = "# REDIS_ENTERPRISE_HOST=redis-enterprise\n# DB connection\nDATABASE_URL=x\n";
        let comments = parse_comments(content);
        assert_eq!(comments.len(), 1);
        assert_eq!(
            comments[0],
            ParsedComment {
                key: "DATABASE_URL".into(),
                comment: "DB connection".into(),
                lines: vec!["DB connection".into()]
            }
        );
    }

    #[test]
    fn comments_prose_not_an_assignment_attaches() {
        // `# Redis settings` has no `=`, so it is prose and attaches.
        let content = "# Redis settings\nREDIS_HOST=localhost\n";
        let comments = parse_comments(content);
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].comment, "Redis settings");
    }
}
