mod link;

use anyhow::{Context, Result};

fn main() -> Result<()> {
    let dir = match std::env::args().nth(1) {
        Some(arg) => std::path::PathBuf::from(arg),
        None => plugin::user_plugins_dir().context("no application data directory")?,
    };

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
