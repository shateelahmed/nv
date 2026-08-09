//! Fuzzy search over env keys across all services.
//!
//! "Fuzzy" search matches loosely: typing `dburl` can find `DATABASE_URL`.
//! We use the `nucleo-matcher` library for the scoring and just feed it strings.

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher};

use anyhow::Result;

use crate::model::{EnvKey, Service};
use crate::parser;

/// Read every configured file and collect all key/value pairs into a flat
/// index used for searching and listing. Missing files are skipped silently.
pub fn build_index(services: &[Service]) -> Vec<EnvKey> {
    let mut index = Vec::new();
    for service in services {
        for file in &service.files {
            // A file listed in config may not exist yet; skip it if unreadable.
            let content = match std::fs::read_to_string(&file.path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            // Parse the file and record where each key/value came from.
            for pair in parser::parse(&content, file.kind) {
                index.push(EnvKey {
                    service: service.name.clone(),
                    file_display: file.display.clone(),
                    file_kind: file.kind,
                    file_path: file.path.clone(),
                    key: pair.key,
                    value: pair.value,
                });
            }
        }
    }
    index
}

/// A newtype so match results can be traced back to their index entry.
///
/// The matcher works on things that look like strings (`AsRef<str>`). We wrap
/// the searchable text together with the original position so we can recover
/// the full [`EnvKey`] after matching.
struct Haystack {
    idx: usize,
    text: String,
}

impl AsRef<str> for Haystack {
    fn as_ref(&self) -> &str {
        &self.text
    }
}

/// Fuzzy-search the `index` for `query`, returning matching entries ordered by
/// descending relevance. An empty query returns everything unchanged.
///
/// The `<'a>` is a lifetime: it says the returned references borrow from
/// `index`, so the results can't outlive it.
pub fn search<'a>(index: &'a [EnvKey], query: &str) -> Vec<&'a EnvKey> {
    if query.trim().is_empty() {
        return index.iter().collect();
    }

    let mut matcher = Matcher::new(Config::DEFAULT);
    let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);

    // Build one searchable string per entry: "service KEY file".
    let haystacks = index.iter().enumerate().map(|(idx, k)| Haystack {
        idx,
        text: format!("{} {} {}", k.service, k.key, k.file_display),
    });

    let mut matched = pattern.match_list(haystacks, &mut matcher);
    // `match_list` already sorts by score descending; map back to entries.
    matched.drain(..).map(|(h, _score)| &index[h.idx]).collect()
}

/// Exact-match `index` for `query`: a key matches only when its whole name
/// equals the query (ASCII case-insensitive). Unlike [`search`], there is no
/// substring or fuzzy matching. Results keep index order; an empty query
/// returns everything unchanged.
pub fn search_exact<'a>(index: &'a [EnvKey], query: &str) -> Vec<&'a EnvKey> {
    if query.trim().is_empty() {
        return index.iter().collect();
    }
    index
        .iter()
        .filter(|k| k.key.eq_ignore_ascii_case(query))
        .collect()
}

/// Glob-match `index` for `query`, using the same syntax as
/// `commands.find.skip_files` (`*`/`**`/`?`). The pattern is matched against
/// the whole key name, ASCII case-insensitively (the `glob` crate has no
/// case-insensitive mode, so both sides are lowercased). Results keep index
/// order; an empty query returns everything unchanged. Returns `Err` for an
/// invalid glob pattern.
pub fn search_glob<'a>(index: &'a [EnvKey], query: &str) -> Result<Vec<&'a EnvKey>> {
    if query.trim().is_empty() {
        return Ok(index.iter().collect());
    }
    let lowered = query.to_ascii_lowercase();
    let pattern = glob::Pattern::new(&lowered)
        .map_err(|e| anyhow::anyhow!("invalid pattern '{query}': {e}"))?;
    Ok(index
        .iter()
        .filter(|k| pattern.matches(&k.key.to_ascii_lowercase()))
        .collect())
}

/// Return the distinct set of keys in the index, sorted, for key-oriented
/// pickers (used by the TUI's first screen).
pub fn distinct_keys(index: &[EnvKey]) -> Vec<String> {
    let mut keys: Vec<String> = index.iter().map(|k| k.key.clone()).collect();
    keys.sort();
    keys.dedup(); // remove neighbouring duplicates (works because it's sorted)
    keys
}

/// Fuzzy-filter a list of strings by `query`, returning matches ordered by
/// descending relevance. An empty query returns the input order unchanged.
///
/// This is the same idea as [`search`] but for a plain list of strings (the
/// TUI uses it to filter the list of key names).
pub fn fuzzy_strings(items: &[String], query: &str) -> Vec<String> {
    if query.trim().is_empty() {
        return items.to_vec();
    }
    let mut matcher = Matcher::new(Config::DEFAULT);
    let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
    pattern
        .match_list(items.iter().cloned(), &mut matcher)
        .into_iter()
        .map(|(item, _)| item)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::FileKind;
    use std::path::PathBuf;

    fn key(service: &str, key: &str) -> EnvKey {
        EnvKey {
            service: service.into(),
            file_display: ".env".into(),
            file_kind: FileKind::Dotenv,
            file_path: PathBuf::from("/tmp/.env"),
            key: key.into(),
            value: "v".into(),
        }
    }

    #[test]
    fn empty_query_returns_all() {
        let index = vec![key("auth", "DB_URL"), key("billing", "API_KEY")];
        assert_eq!(search(&index, "").len(), 2);
    }

    #[test]
    fn fuzzy_matches_key_name() {
        let index = vec![key("auth", "DATABASE_URL"), key("billing", "API_KEY")];
        let results = search(&index, "dburl");
        assert!(!results.is_empty());
        assert_eq!(results[0].key, "DATABASE_URL");
    }

    #[test]
    fn distinct_keys_dedups() {
        let index = vec![
            key("auth", "SHARED"),
            key("billing", "SHARED"),
            key("auth", "X"),
        ];
        assert_eq!(
            distinct_keys(&index),
            vec!["SHARED".to_string(), "X".to_string()]
        );
    }

    #[test]
    fn exact_matches_whole_key() {
        let index = vec![
            key("auth", "DATABASE_URL"),
            key("billing", "API_DATABASE_URL"),
            key("billing", "API_KEY"),
        ];
        let results = search_exact(&index, "DATABASE_URL");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].key, "DATABASE_URL");
    }

    #[test]
    fn exact_is_case_insensitive() {
        let index = vec![key("auth", "DATABASE_URL"), key("auth", "API_KEY")];
        let results = search_exact(&index, "database_url");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].key, "DATABASE_URL");
    }

    #[test]
    fn exact_does_not_substring_match() {
        let index = vec![key("auth", "DATABASE_URL")];
        assert!(search_exact(&index, "URL").is_empty());
    }

    #[test]
    fn exact_empty_query_returns_all() {
        let index = vec![key("auth", "A"), key("auth", "B")];
        assert_eq!(search_exact(&index, "").len(), 2);
    }

    #[test]
    fn glob_matches_prefix() {
        let index = vec![
            key("auth", "DB_HOST"),
            key("auth", "DB_PORT"),
            key("billing", "REDIS_URL"),
        ];
        let results = search_glob(&index, "DB_*").unwrap();
        let keys: Vec<&str> = results.iter().map(|k| k.key.as_str()).collect();
        assert_eq!(keys, vec!["DB_HOST", "DB_PORT"]);
    }

    #[test]
    fn glob_matches_suffix() {
        let index = vec![key("auth", "DATABASE_URL"), key("auth", "API_KEY")];
        let results = search_glob(&index, "*_URL").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].key, "DATABASE_URL");
    }

    #[test]
    fn glob_is_case_insensitive() {
        let index = vec![key("auth", "DB_HOST"), key("auth", "REDIS_URL")];
        let results = search_glob(&index, "db_*").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].key, "DB_HOST");
    }

    #[test]
    fn glob_star_matches_all() {
        let index = vec![key("auth", "DB_HOST"), key("auth", "REDIS_URL")];
        assert_eq!(search_glob(&index, "*").unwrap().len(), 2);
    }

    #[test]
    fn glob_empty_query_returns_all() {
        let index = vec![key("auth", "A"), key("auth", "B")];
        assert_eq!(search_glob(&index, "").unwrap().len(), 2);
    }

    #[test]
    fn glob_invalid_pattern_errors() {
        let index = vec![key("auth", "A")];
        assert!(search_glob(&index, "[").is_err());
    }
}
