//! `nv set` — set a key to a value across selected services and files.

use anyhow::{Result, bail};

use super::{Cli, context};
use crate::color;
use crate::edit::{self, ChangeSet};

/// Handle `nv set <KEY> <VALUE>`: write the same value to every selected target.
pub fn run(cli: &Cli, key: &str, value: &str) -> Result<()> {
    let ctx = context::resolve(cli)?;
    context::print_banner(ctx.source);

    // Which services and file kinds to touch (empty = all).
    let service_filter = ctx.service_filter(cli);
    let kind_filter = ctx.kind_filter(cli)?;
    let targets = edit::collect_targets(&ctx.services, &service_filter, &kind_filter);

    if targets.is_empty() {
        bail!("no matching files; check --service/--file filters or your nv.yml");
    }

    // The closure `|_| value.to_string()` ignores the target and always returns
    // the same value, so every file gets identical content for this key.
    let changes = ChangeSet::build(&targets, key, |_| value.to_string())?;

    let use_color = color::should_use_color();
    let colors = ctx.colors();
    context::preview_and_apply(cli, &changes, &colors, use_color)
}
