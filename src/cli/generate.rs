//! `nv gen` — generate a random secret and set it on a key.
//!
//! (The file is named `generate.rs`, not `gen.rs`, because `gen` is a reserved
//! word in this edition of Rust.)

use anyhow::{Result, bail};

use super::{Cli, FormatArg, context};
use crate::edit::{self, ChangeSet};
use crate::secret::{self, SecretSpec};

/// Handle `nv gen <KEY>`: create a secret and write it to the selected targets.
pub fn run(
    cli: &Cli,
    key: &str,
    length: Option<usize>,
    format: Option<FormatArg>,
    charset: Option<String>,
    unique: bool,
) -> Result<()> {
    let ctx = context::resolve(cli)?;
    context::print_banner(ctx.source);

    let service_filter = ctx.service_filter(cli);
    let kind_filter = ctx.kind_filter(cli)?;
    let targets = edit::collect_targets(&ctx.services, &service_filter, &kind_filter);

    if targets.is_empty() {
        bail!("no matching files; check --service/--file filters or your nv.yml");
    }

    // Start from any nv.yml preset for this key, then apply CLI overrides.
    let mut spec = ctx
        .config
        .as_ref()
        .and_then(|c| c.secret_preset(key))
        .map(SecretSpec::from)
        .unwrap_or_default();
    // Command-line flags win over the preset when provided.
    if let Some(len) = length {
        spec.length = len;
    }
    if let Some(fmt) = format {
        spec.format = fmt.into();
    }
    if let Some(cs) = charset {
        spec.charset = Some(cs);
    }

    // Example files always get an empty value. Real (non-example) targets get
    // either one shared secret or a distinct one each when --unique is set.
    let real_count = targets.iter().filter(|t| !t.file.kind.is_example()).count();

    // Pre-generate the values up front: either a pool of distinct secrets (one
    // per real target) or a single shared secret reused everywhere.
    let unique_pool: Vec<String> = if unique {
        secret::generate_unique(&spec, real_count)?
    } else {
        Vec::new()
    };
    let shared = if unique {
        String::new()
    } else {
        secret::generate(&spec)?
    };

    // `next_unique` walks through the pool, handing the next secret to each
    // real target. This closure decides the value written to each file.
    let mut next_unique = 0usize;
    let changes = ChangeSet::build(&targets, key, |t| {
        if t.file.kind.is_example() {
            String::new() // never put a real secret in an example file
        } else if unique {
            let v = unique_pool.get(next_unique).cloned().unwrap_or_default();
            next_unique += 1;
            v
        } else {
            shared.clone()
        }
    })?;

    context::preview_and_apply(cli, &changes)
}
