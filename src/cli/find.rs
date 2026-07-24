//! `nv find` — fuzzy search keys across services.

use anyhow::Result;

use super::{Cli, context};
use crate::search;

/// Handle `nv find <query>`: print every key that fuzzy-matches the query.
pub fn run(cli: &Cli, query: &str) -> Result<()> {
    // Figure out where config comes from and which services exist.
    let ctx = context::resolve(cli)?;
    context::print_banner(ctx.source);

    // Build the searchable list of keys, then filter it by the query.
    let index = search::build_index(&ctx.services);
    let results = search::search(&index, query);

    if results.is_empty() {
        eprintln!("No matches.");
        return Ok(());
    }

    // Print aligned columns: service, key, file, value. The `:<20` etc. pad
    // each column to a fixed width so everything lines up.
    for entry in results {
        println!(
            "{:<20} {:<24} {:<18} = {}",
            entry.service, entry.key, entry.file_display, entry.value
        );
    }
    Ok(())
}
