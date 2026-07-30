//! `nv find` — fuzzy search keys across services with hierarchical colored output.

use std::collections::BTreeMap;

use anyhow::Result;

use super::{Cli, context};
use crate::color::{self, AnsiColor, ColorConfig};
use crate::display::{self, Output, TreeFile, TreeItem, TreeService};
use crate::model::EnvKey;
use crate::search;

/// Handle `nv find <query>`: print every key that fuzzy-matches the query
/// in a hierarchical, colorized format.
pub fn run(cli: &Cli, query: &str) -> Result<()> {
    let ctx = context::resolve(cli)?;
    context::print_banner(ctx.source);

    let index = search::build_index(&ctx.services);
    let results = search::search(&index, query);

    if results.is_empty() {
        eprintln!("No matches.");
        return Ok(());
    }

    let use_color = color::should_use_color();
    let colors = ctx.colors();

    print_legend(&colors, use_color);
    print_hierarchical_output(&results, &colors, use_color);

    Ok(())
}

/// Print a color legend at the top of the output.
fn print_legend(colors: &ColorConfig, use_color: bool) {
    eprintln!();
    eprintln!("Color legend:");
    eprintln!(
        "  {} microservice root",
        colorize_color_name(colors.service_root, use_color)
    );
    eprintln!(
        "  {} subfolder",
        colorize_color_name(colors.subfolder, use_color)
    );
    eprintln!("  {} file", colorize_color_name(colors.file, use_color));
    eprintln!("  {} key name", colorize_color_name(colors.key, use_color));
    eprintln!("  {} value", colorize_color_name(colors.value, use_color));
    eprintln!();
}

/// Colorize a color name for the legend display.
fn colorize_color_name(color: AnsiColor, use_color: bool) -> String {
    let name = color.name();
    if use_color {
        format!("{}{}{}", color.code(), name, AnsiColor::Reset.code())
    } else {
        name.to_string()
    }
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
    display::render_tree(&services, colors, use_color, &mut out);
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

    #[test]
    fn print_legend_does_not_panic() {
        let colors = ColorConfig::default();
        print_legend(&colors, false);
    }
}
