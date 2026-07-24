//! Fuzzy search over env keys across all services.
//!
//! "Fuzzy" search matches loosely: typing `dburl` can find `DATABASE_URL`.
//! We use the `nucleo-matcher` library for the scoring and just feed it strings.

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher};

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
}
