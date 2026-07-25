//! `.env` / `.env.example` parsing and line-oriented editing.
//!
//! `.env` files are simple: one `KEY=value` per line, plus blank lines and
//! `#` comments. To preserve formatting we never rebuild the whole file; we
//! find the one line that defines a key and rewrite just that line.

use super::ParsedPair;

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
    Some((key.to_string(), unquote(raw_value.trim())))
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
fn unquote(value: &str) -> String {
    let bytes = value.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return value[1..value.len() - 1].to_string();
        }
    }
    value.to_string()
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

/// Remove `key` from `content`, preserving comments and blank lines.
///
/// If the key is not present, the content is returned unchanged.
pub fn remove_key(content: &str, key: &str) -> String {
    let newline = if content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
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
    let newline = if content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
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
}
