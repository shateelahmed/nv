//! Shared hierarchical tree rendering for CLI output.
//!
//! All commands that display grouped data (leaks, fake-secrets, duplicates,
//! find, unused, diffs) use the same `├──`/`└──`/`│` tree format. This
//! module provides a single renderer to avoid duplication.

use crate::color::{self, AnsiColor, ColorConfig};

/// A single displayable item (leaf node in the tree).
pub struct TreeItem {
    pub label: String,
    pub color: AnsiColor,
}

/// A file node containing items.
pub struct TreeFile {
    pub name: String,
    pub count: usize,
    pub items: Vec<TreeItem>,
}

/// A service node containing files.
pub struct TreeService {
    pub name: String,
    pub count: usize,
    pub files: Vec<TreeFile>,
}

/// Where to write the rendered tree.
pub enum Output {
    Stdout,
    Stderr,
    String(String),
}

/// Render a hierarchical tree to the given output.
///
/// `show_file_counts` controls whether each file line carries its item count
/// (`file_name (count)`); set to `false` for bare file listings.
///
/// Structure:
/// ```text
/// service_name/ (count)
/// ├── file_name (count)
/// │   ├── item_one
/// │   └── item_two
/// └── other_file (count)
///     └── item_three
/// ```
pub fn render_tree(
    services: &[TreeService],
    colors: &ColorConfig,
    use_color: bool,
    show_file_counts: bool,
    out: &mut Output,
) {
    for service in services {
        // Service root line: "service_name/ (count)"
        let line = format!(
            "{} {}\n",
            color::colorize(
                &format!("{}/", service.name),
                colors.service_root,
                use_color
            ),
            color::colorize(
                &format!("({})", service.count),
                colors.service_root,
                use_color
            ),
        );
        write_output(out, &line);

        let file_count = service.files.len();
        for (i, file) in service.files.iter().enumerate() {
            let is_last_file = i + 1 == file_count;
            let branch = if is_last_file {
                "└── "
            } else {
                "├── "
            };
            let pipe = if is_last_file { "    " } else { "│   " };

            // File-level branch in service color (parent node). The count is
            // optional; some listings (e.g. the compare "available files"
            // diagnostic) show bare file names.
            let file_line = if show_file_counts {
                format!(
                    "{}{} {}\n",
                    color::colorize(branch, colors.service_root, use_color),
                    color::colorize(&file.name, colors.file, use_color),
                    color::colorize(&format!("({})", file.count), colors.file, use_color),
                )
            } else {
                format!(
                    "{}{}\n",
                    color::colorize(branch, colors.service_root, use_color),
                    color::colorize(&file.name, colors.file, use_color),
                )
            };
            write_output(out, &file_line);

            // Items under this file.
            for (j, item) in file.items.iter().enumerate() {
                let is_last_item = j + 1 == file.items.len();
                let key_branch = if is_last_item {
                    "└── "
                } else {
                    "├── "
                };

                let item_line = format!(
                    "{}{}{}\n",
                    color::colorize(pipe, colors.service_root, use_color),
                    color::colorize(key_branch, colors.file, use_color),
                    color::colorize(&item.label, item.color, use_color),
                );
                write_output(out, &item_line);
            }
        }
    }
}

/// Render a summary line after the tree.
pub fn render_summary(message: &str, colors: &ColorConfig, use_color: bool, out: &mut Output) {
    let line = format!(
        "\n{}\n",
        color::colorize(message, colors.service_root, use_color),
    );
    write_output(out, &line);
}

fn write_output(out: &mut Output, text: &str) {
    match out {
        Output::Stdout => print!("{text}"),
        Output::Stderr => eprint!("{text}"),
        Output::String(buf) => buf.push_str(text),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output_string(out: &Output) -> &str {
        match out {
            Output::String(s) => s,
            _ => panic!("expected String output"),
        }
    }

    #[test]
    fn empty_services_produces_no_output() {
        let colors = ColorConfig::default();
        let mut out = Output::String(String::new());
        render_tree(&[], &colors, false, true, &mut out);
        assert!(output_string(&out).is_empty());
    }

    #[test]
    fn single_service_single_file_single_item() {
        let services = vec![TreeService {
            name: "auth".into(),
            count: 1,
            files: vec![TreeFile {
                name: ".env".into(),
                count: 1,
                items: vec![TreeItem {
                    label: "DB_URL = postgres://localhost".into(),
                    color: AnsiColor::Green,
                }],
            }],
        }];
        let colors = ColorConfig::default();
        let mut out = Output::String(String::new());
        render_tree(&services, &colors, false, true, &mut out);
        let text = output_string(&out);
        assert!(text.contains("auth/ (1)"));
        assert!(text.contains(".env (1)"));
        assert!(text.contains("DB_URL"));
        // Last file uses └──, last item uses └──
        assert!(text.contains("└──"));
        assert!(!text.contains("├──"));
    }

    #[test]
    fn multiple_services_multiple_files() {
        let services = vec![
            TreeService {
                name: "auth".into(),
                count: 2,
                files: vec![
                    TreeFile {
                        name: ".env".into(),
                        count: 1,
                        items: vec![TreeItem {
                            label: "KEY1".into(),
                            color: AnsiColor::Green,
                        }],
                    },
                    TreeFile {
                        name: "configmap.yml".into(),
                        count: 1,
                        items: vec![TreeItem {
                            label: "KEY2".into(),
                            color: AnsiColor::Green,
                        }],
                    },
                ],
            },
            TreeService {
                name: "billing".into(),
                count: 1,
                files: vec![TreeFile {
                    name: ".env".into(),
                    count: 1,
                    items: vec![TreeItem {
                        label: "KEY3".into(),
                        color: AnsiColor::Green,
                    }],
                }],
            },
        ];
        let colors = ColorConfig::default();
        let mut out = Output::String(String::new());
        render_tree(&services, &colors, false, true, &mut out);
        let text = output_string(&out);
        // First service has 2 files → first uses ├──, second uses └──
        assert!(text.contains("├── .env (1)"));
        assert!(text.contains("└── configmap.yml (1)"));
        // Second service is last → uses └──
        assert!(text.contains("└── .env (1)"));
    }
}
