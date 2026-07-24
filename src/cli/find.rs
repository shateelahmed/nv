//! `nv find` — fuzzy search keys across services with hierarchical colored output.

use std::collections::BTreeMap;

use anyhow::Result;

use super::{Cli, context};
use crate::color::{self, AnsiColor, ColorConfig, colorize};
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
    let colors = ctx
        .config
        .as_ref()
        .map(|c| c.colors.clone())
        .unwrap_or_default();

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

/// Print the hierarchical grouped output.
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

    for (service, files) in &grouped {
        // Print service name (root folder).
        println!(
            "{}",
            colorize(&format!("{}/", service), colors.service_root, use_color)
        );

        // Group files by their parent folder for hierarchical display.
        let mut folder_files: BTreeMap<String, Vec<(&String, &Vec<&EnvKey>)>> = BTreeMap::new();

        for (file_display, keys) in files {
            if let Some((folder, _filename)) = file_display.rsplit_once('/') {
                folder_files
                    .entry(folder.to_string())
                    .or_default()
                    .push((file_display, keys));
            } else {
                // File is at the service root level (no folder).
                folder_files
                    .entry(String::new())
                    .or_default()
                    .push((file_display, keys));
            }
        }

        for (folder, file_entries) in &folder_files {
            if !folder.is_empty() {
                // Print subfolder with indentation.
                println!(
                    "  {}",
                    colorize(&format!("{}/", folder), colors.subfolder, use_color)
                );
            }

            for (filename, keys) in file_entries {
                // Extract just the filename part (after last slash).
                let name = filename
                    .rsplit_once('/')
                    .map(|(_, n)| n)
                    .unwrap_or(filename);
                // Print file name with indentation.
                let indent = if folder.is_empty() { "  " } else { "    " };
                println!("{}{}", indent, colorize(name, colors.file, use_color));

                // Print each key-value pair under the file.
                for key in keys.iter() {
                    let kv_indent = if folder.is_empty() { "    " } else { "      " };
                    let key_display = colorize(&key.key, colors.key, use_color);
                    let value_display = colorize(&key.value, colors.value, use_color);
                    println!("{}{} = {}", kv_indent, key_display, value_display);
                }
            }
        }
    }
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
