//! `nv find` — fuzzy search keys across services with hierarchical colored output.

use std::collections::BTreeMap;

use anyhow::Result;

use super::{Cli, context};
use crate::color::{self, ColorConfig};
use crate::display::{self, Output, TreeFile, TreeItem, TreeService};
use crate::model::{EnvKey, Service};
use crate::search;

/// Handle `nv find <query>`: print every key that fuzzy-matches the query
/// in a hierarchical, colorized format. `--exact` matches whole key names and
/// `--pattern` glob-matches key names instead of fuzzy matching.
pub fn run(cli: &Cli, query: &str, exact: bool, pattern: Option<&str>) -> Result<()> {
    let ctx = context::resolve(cli)?;
    context::print_banner(&ctx);

    // `--service`/`-s` scopes the search to the named services.
    let service_filter = ctx.service_filter(cli);
    let mut services: Vec<Service> = if service_filter.is_empty() {
        ctx.services.clone()
    } else {
        ctx.services
            .iter()
            .filter(|s| service_filter.contains(&s.name))
            .cloned()
            .collect()
    };

    // `commands.find.skip_files` (global + per-service) excludes files from
    // the search index.
    for service in &mut services {
        let skip_files = context::build_skip_files(&ctx.config, &service.name, |cfg, svc| {
            cfg.find_skip_files_for(svc)
        });
        if !skip_files.is_empty() {
            service
                .files
                .retain(|f| !context::file_is_skipped(f, &service.path, &skip_files));
        }
    }

    let index = search::build_index(&services);
    // The matching mode is chosen by the flags: `--exact` requires the whole
    // key name, `--pattern` glob-matches the key name, and the default fuzzy
    // match is unchanged.
    let results = if exact {
        search::search_exact(&index, query)
    } else if let Some(pattern) = pattern {
        search::search_glob(&index, pattern)?
    } else {
        search::search(&index, query)
    };

    if results.is_empty() {
        eprintln!("No matches.");
        return Ok(());
    }

    let use_color = color::should_use_color();
    let colors = ctx.colors();

    print_hierarchical_output(&results, &colors, use_color);

    Ok(())
}

/// Print the hierarchical grouped output with tree-style vertical lines.
fn print_hierarchical_output(results: &[&EnvKey], colors: &ColorConfig, use_color: bool) {
    // Group by service, then by file display path.
    let mut grouped: BTreeMap<String, BTreeMap<String, Vec<&EnvKey>>> = BTreeMap::new();

    for key in results {
        grouped
            .entry(key.service.clone())
            .or_default()
            .entry(key.file_display.clone())
            .or_default()
            .push(key);
    }

    let services: Vec<TreeService> = grouped
        .into_iter()
        .map(|(service_name, files)| {
            let file_count = files.len();
            let tree_files: Vec<TreeFile> = files
                .into_iter()
                .map(|(file_name, keys)| {
                    let items: Vec<TreeItem> = keys
                        .iter()
                        .map(|k| TreeItem {
                            label: color::colored_kv_label(
                                &k.key,
                                &k.value,
                                colors.key,
                                colors.value,
                                use_color,
                            ),
                            color: colors.key,
                        })
                        .collect();
                    TreeFile {
                        name: file_name,
                        count: items.len(),
                        items,
                    }
                })
                .collect();
            TreeService {
                name: service_name,
                count: file_count,
                files: tree_files,
            }
        })
        .collect();

    let mut out = Output::Stdout;
    display::render_tree(&services, colors, use_color, true, &mut out);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::FileKind;
    use std::path::PathBuf;

    fn make_key(service: &str, file_display: &str, key: &str, value: &str) -> EnvKey {
        EnvKey {
            service: service.into(),
            file_display: file_display.into(),
            file_kind: FileKind::Dotenv,
            file_path: PathBuf::from("/tmp/.env"),
            key: key.into(),
            value: value.into(),
        }
    }

    #[test]
    fn groups_by_service() {
        let keys = vec![
            make_key("auth", ".env", "DB_URL", "postgres://..."),
            make_key("billing", ".env", "API_KEY", "sk-..."),
        ];
        let refs: Vec<&EnvKey> = keys.iter().collect();
        let colors = ColorConfig::default();
        print_hierarchical_output(&refs, &colors, false);
    }

    #[test]
    fn groups_by_folder() {
        let keys = vec![
            make_key("auth", "src/.env", "DB_URL", "postgres://..."),
            make_key("auth", "docker/.env", "API_KEY", "sk-..."),
        ];
        let refs: Vec<&EnvKey> = keys.iter().collect();
        let colors = ColorConfig::default();
        print_hierarchical_output(&refs, &colors, false);
    }

    #[test]
    fn handles_duplicate_keys() {
        let keys = vec![
            make_key("auth", ".env", "DB_URL", "postgres://..."),
            make_key("auth", ".env", "DB_URL", "mysql://..."),
        ];
        let refs: Vec<&EnvKey> = keys.iter().collect();
        let colors = ColorConfig::default();
        print_hierarchical_output(&refs, &colors, false);
    }
}
