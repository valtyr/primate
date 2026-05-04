//! Generates `primate.schema.json` from the `Config` struct in
//! `src/config/mod.rs`. Run with `cargo run --bin gen-schema`; CI
//! runs the same command and fails if the committed schema doesn't
//! match the generated one (so the `Config` struct stays the source
//! of truth).

use primate::config::Config;
use std::path::PathBuf;

const SCHEMA_FILE: &str = "primate.schema.json";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let schema = schemars::schema_for!(Config);
    let mut json = serde_json::to_string_pretty(&schema)?;
    // Trailing newline so the file matches POSIX expectations and a
    // `git diff --exit-code` check after running this doesn't trip
    // on missing-newline whitespace.
    json.push('\n');

    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(SCHEMA_FILE);
    std::fs::write(&path, json)?;
    println!("Wrote {}", path.display());
    Ok(())
}
