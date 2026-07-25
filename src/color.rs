//! ANSI terminal color support for `nv`.
//!
//! Provides configurable colors for CLI output. Respects the `NO_COLOR`
//! environment variable (https://no-color.org/) and TTY detection.

use std::fmt;

use serde::{Deserialize, Serialize};

/// ANSI color codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnsiColor {
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    /// Resets all styling.
    #[default]
    Reset,
}

impl AnsiColor {
    /// Return the ANSI escape code for this color.
    pub fn code(self) -> &'static str {
        match self {
            AnsiColor::Black => "\x1b[30m",
            AnsiColor::Red => "\x1b[31m",
            AnsiColor::Green => "\x1b[32m",
            AnsiColor::Yellow => "\x1b[33m",
            AnsiColor::Blue => "\x1b[34m",
            AnsiColor::Magenta => "\x1b[35m",
            AnsiColor::Cyan => "\x1b[36m",
            AnsiColor::White => "\x1b[37m",
            AnsiColor::Reset => "\x1b[0m",
        }
    }

    /// Human-readable label for the legend.
    pub fn name(self) -> &'static str {
        match self {
            AnsiColor::Black => "black",
            AnsiColor::Red => "red",
            AnsiColor::Green => "green",
            AnsiColor::Yellow => "yellow",
            AnsiColor::Blue => "blue",
            AnsiColor::Magenta => "magenta",
            AnsiColor::Cyan => "cyan",
            AnsiColor::White => "white",
            AnsiColor::Reset => "reset",
        }
    }
}

impl fmt::Display for AnsiColor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Color configuration for CLI output elements.
///
/// Each field controls the color of a specific part of the hierarchical output.
/// All colors can be customized in `nv.yml` under the `colors:` section.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColorConfig {
    /// Color for microservice root folder names.
    #[serde(default = "default_service_root")]
    pub service_root: AnsiColor,

    /// Color for subfolder names within a service.
    #[serde(default = "default_subfolder")]
    pub subfolder: AnsiColor,

    /// Color for file names.
    #[serde(default = "default_file")]
    pub file: AnsiColor,

    /// Color for environment variable key names.
    #[serde(default = "default_key")]
    pub key: AnsiColor,

    /// Color for environment variable values.
    #[serde(default = "default_value")]
    pub value: AnsiColor,

    /// Color for added lines in diff previews (`+` prefix).
    #[serde(default = "default_added")]
    pub added: AnsiColor,

    /// Color for removed lines in diff previews (`-` prefix).
    #[serde(default = "default_removed")]
    pub removed: AnsiColor,
}

fn default_service_root() -> AnsiColor {
    AnsiColor::Magenta
}

fn default_subfolder() -> AnsiColor {
    AnsiColor::Blue
}

fn default_file() -> AnsiColor {
    AnsiColor::Cyan
}

fn default_key() -> AnsiColor {
    AnsiColor::Green
}

fn default_value() -> AnsiColor {
    AnsiColor::Yellow
}

fn default_added() -> AnsiColor {
    AnsiColor::Green
}

fn default_removed() -> AnsiColor {
    AnsiColor::Red
}

impl Default for ColorConfig {
    fn default() -> Self {
        Self {
            service_root: default_service_root(),
            subfolder: default_subfolder(),
            file: default_file(),
            key: default_key(),
            value: default_value(),
            added: default_added(),
            removed: default_removed(),
        }
    }
}

/// Whether color output should be used.
///
/// Returns `false` if:
/// - The `NO_COLOR` environment variable is set
/// - stdout is not a terminal (TTY)
pub fn should_use_color() -> bool {
    if std::env::var("NO_COLOR").is_ok() {
        return false;
    }
    // Check if stdout is a terminal
    use std::io::IsTerminal;
    std::io::stdout().is_terminal()
}

/// A wrapper that applies ANSI color codes to text.
pub struct Colorizer<'a> {
    text: &'a str,
    color: AnsiColor,
    enabled: bool,
}

impl<'a> Colorizer<'a> {
    pub fn new(text: &'a str, color: AnsiColor, enabled: bool) -> Self {
        Self {
            text,
            color,
            enabled,
        }
    }
}

impl<'a> fmt::Display for Colorizer<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.enabled && self.color != AnsiColor::Reset {
            write!(
                f,
                "{}{}{}",
                self.color.code(),
                self.text,
                AnsiColor::Reset.code()
            )
        } else {
            f.write_str(self.text)
        }
    }
}

/// Helper to create a colored string.
pub fn colorize<'a>(text: &'a str, color: AnsiColor, enabled: bool) -> Colorizer<'a> {
    Colorizer::new(text, color, enabled)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ansi_color_codes_are_valid() {
        assert_eq!(AnsiColor::Red.code(), "\x1b[31m");
        assert_eq!(AnsiColor::Reset.code(), "\x1b[0m");
    }

    #[test]
    fn colorizer_with_disabled_just_returns_text() {
        let colored = colorize("hello", AnsiColor::Red, false);
        assert_eq!(colored.to_string(), "hello");
    }

    #[test]
    fn colorizer_with_enabled_wraps_in_codes() {
        let colored = colorize("hello", AnsiColor::Red, true);
        assert_eq!(colored.to_string(), "\x1b[31mhello\x1b[0m");
    }

    #[test]
    fn default_color_config_has_sensible_defaults() {
        let config = ColorConfig::default();
        assert_eq!(config.service_root, AnsiColor::Magenta);
        assert_eq!(config.subfolder, AnsiColor::Blue);
        assert_eq!(config.file, AnsiColor::Cyan);
        assert_eq!(config.key, AnsiColor::Green);
        assert_eq!(config.value, AnsiColor::Yellow);
        assert_eq!(config.added, AnsiColor::Green);
        assert_eq!(config.removed, AnsiColor::Red);
    }

    #[test]
    fn color_config_serde_roundtrip() {
        let config = ColorConfig::default();
        let yaml = serde_yaml::to_string(&config).unwrap();
        let parsed: ColorConfig = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(config.service_root, parsed.service_root);
        assert_eq!(config.subfolder, parsed.subfolder);
        assert_eq!(config.file, parsed.file);
        assert_eq!(config.key, parsed.key);
        assert_eq!(config.value, parsed.value);
        assert_eq!(config.added, parsed.added);
        assert_eq!(config.removed, parsed.removed);
    }
}
