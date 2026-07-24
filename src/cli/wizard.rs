//! Interactive configuration wizard for creating `nv.yml`.
//!
//! Uses the `dialoguer` library to ask questions in the terminal: text input,
//! yes/no confirms, and multi-select checklists. It runs on `nv init` and on
//! the very first launch of the TUI when no config exists yet.

use std::path::{Path, PathBuf};

use anyhow::Result;
use dialoguer::{Confirm, Input, MultiSelect};

use super::Cli;
use crate::config::{self, Config, ServiceConfig, ServiceFiles};
use crate::discovery;
use crate::model::FileKind;

/// Handle `nv init`: create (or overwrite) `nv.yml` in the current directory.
pub fn run_init(_cli: &Cli) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let config_path = cwd.join(config::CONFIG_FILE);

    if config_path.exists() {
        let overwrite = Confirm::new()
            .with_prompt(format!("{} exists. Overwrite?", config::CONFIG_FILE))
            .default(false)
            .interact()?;
        if !overwrite {
            eprintln!("Keeping existing configuration.");
            return Ok(());
        }
    }

    let config = build_interactively(&cwd)?;
    config::save(&config_path, &config)?;
    eprintln!("Wrote {}", config_path.display());
    Ok(())
}

/// If no config exists, offer to run the wizard. Returns the config path when
/// one is available (existing or freshly created).
pub fn first_run_if_needed(cwd: &Path) -> Result<Option<PathBuf>> {
    if let Some(path) = config::find_config(cwd) {
        return Ok(Some(path));
    }

    eprintln!("No {} found.", config::CONFIG_FILE);
    let create = Confirm::new()
        .with_prompt("Create one now?")
        .default(true)
        .interact()?;
    if !create {
        return Ok(None);
    }

    let config = build_interactively(cwd)?;
    let path = cwd.join(config::CONFIG_FILE);
    config::save(&path, &config)?;
    eprintln!("Wrote {}", path.display());
    Ok(Some(path))
}

/// Drive the interactive prompts and produce a [`Config`].
fn build_interactively(base: &Path) -> Result<Config> {
    // 1. Ask where the services live (defaulting to the current directory).
    let services_root: String = Input::new()
        .with_prompt("Where are your microservices? (path)")
        .default(".".to_string())
        .interact_text()?;

    let mut config = Config {
        services_root: services_root.clone(),
        ..Default::default()
    };

    // 2. Scan that folder so we can offer the discovered subfolders to choose.
    let root_abs = config.services_root_abs(base);
    let discovered = discovery::discover_scanned(&root_abs, &[]).unwrap_or_default();

    if discovered.is_empty() {
        eprintln!(
            "No subfolders found under {}. You can edit {} later.",
            root_abs.display(),
            config::CONFIG_FILE
        );
        return Ok(config);
    }

    // 3. Let the user tick which folders are actually services.
    let names: Vec<String> = discovered.iter().map(|s| s.name.clone()).collect();
    let selected_idx = MultiSelect::new()
        .with_prompt("Select folders to treat as microservices (space to toggle, enter to confirm)")
        .items(&names)
        .defaults(&vec![true; names.len()])
        .interact()?;

    // If everything is selected we can rely on auto-discovery and keep nv.yml
    // minimal; otherwise record an explicit list.
    let all_selected = selected_idx.len() == names.len();

    // 4. Optionally let the user pick exact files per service.
    let customize_files = Confirm::new()
        .with_prompt("Customize which files count as env files per service?")
        .default(false)
        .interact()?;

    if all_selected && !customize_files {
        // Minimal config: services_root only, auto-discover the rest.
        return Ok(config);
    }

    // 5. Record the chosen services (and files, if customized) in the config.
    for &idx in &selected_idx {
        let service = &discovered[idx];
        let files = if customize_files {
            Some(select_files_for(service)?)
        } else {
            None
        };
        config.services.push(ServiceConfig {
            name: service.name.clone(),
            path: None,
            files,
        });
    }

    Ok(config)
}

/// Prompt the user to choose which discovered files belong to a service.
fn select_files_for(service: &crate::model::Service) -> Result<ServiceFiles> {
    let labels: Vec<String> = service
        .files
        .iter()
        .map(|f| format!("{} [{}]", f.display, f.kind))
        .collect();

    let mut files = ServiceFiles::default();
    if labels.is_empty() {
        return Ok(files);
    }

    let chosen = MultiSelect::new()
        .with_prompt(format!("Files for '{}'", service.name))
        .items(&labels)
        .defaults(&vec![true; labels.len()])
        .interact()?;

    for &idx in &chosen {
        let file = &service.files[idx];
        let bucket = match file.kind {
            FileKind::Dotenv => &mut files.dotenv,
            FileKind::DotenvExample => &mut files.dotenv_example,
            FileKind::ConfigMap => &mut files.configmap,
            FileKind::Secret => &mut files.secret,
        };
        bucket
            .get_or_insert_with(Vec::new)
            .push(file.display.clone());
    }
    Ok(files)
}
