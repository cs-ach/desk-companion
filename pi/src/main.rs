mod link;

use anyhow::{Context, Result};

fn main() -> Result<()> {
    let dir = std::env::args()
        .nth(1)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("/var/lib/desk-companion/plugins"));

    let (plugins, errors) = match plugin::discover(&dir) {
        Ok(found) => found,

        Err(e) => {
            eprintln!("no plugins loaded: {e}");
            (Vec::new(), Vec::new())
        }
    };

    for error in &errors {
        eprintln!("skipped plugin: {error}");
    }

    println!("{} plugin(s) loaded from {}", plugins.len(), dir.display());
    for p in &plugins {
        println!("  {:<16} {}", p.manifest.id, p.manifest.name);
    }

    link::describe().context("describing the link")?;
    Ok(())
}
