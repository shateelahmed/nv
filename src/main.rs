//! `nv` — configure environment variables across multiple microservices.
//!
//! This is the program's entry point. In Rust, a binary crate starts at
//! `main()`. The real work is split into modules (declared with `mod` below);
//! each `mod name;` line pulls in the file `src/name.rs` or `src/name/mod.rs`.

// Each `mod` declaration makes another source file part of this program.
mod cli; // command-line parsing, the wizard, and command handlers
mod config; // reading/writing the `nv.yml` config file
mod discovery; // finding services and their env files on disk
mod edit; // building, previewing (diffing), and applying file changes
mod model; // shared data types used everywhere
mod parser; // reading/editing .env and YAML files without losing formatting
mod search; // fuzzy searching keys
mod secret; // generating random secrets
mod tui; // the interactive full-screen terminal UI

fn main() {
    // `cli::run()` returns a `Result`: `Ok` on success, or `Err` with a message.
    // `if let Err(err) = ...` runs this block only when something went wrong.
    if let Err(err) = cli::run() {
        // `{err:#}` prints the error and any underlying causes (the `#` flag).
        eprintln!("error: {err:#}");
        // A non-zero exit code tells the shell the program failed.
        std::process::exit(1);
    }
}
