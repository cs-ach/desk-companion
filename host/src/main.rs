mod sync;

use std::path::PathBuf;

use anyhow::{Context, Result};
use plugin::ConfigStore;

fn main() -> Result<()> {
    let dir = plugins_dir()?;
    let (plugins, errors) =
        plugin::discover(&dir).with_context(|| format!("scanning {}", dir.display()))?;

    for error in &errors {
        eprintln!("skipped plugin: {error}");
    }

    let store = ConfigStore::user_default()?;
    println!("{} plugin(s) in {}", plugins.len(), dir.display());
    for p in &plugins {
        let sides = if p.manifest.requires_host {
            "host+pi"
        } else {
            "pi only"
        };
        println!("  {:<12} {:<8} {}", p.manifest.id, sides, p.manifest.name);

        let config = store.load(&p.manifest)?;
        for warning in &config.warnings {
            eprintln!("    config: {warning}");
        }

        for (key, value) in config.device_values() {
            println!("    {key} = {value}");
        }
        for key in config.secret_keys() {
            println!("    {key} = <keychain, host only>");
        }
    }
    println!("\nconfig read from {}", store.path_for("<id>").display());

    let files = sync::device_manifest(&plugins)?;
    println!("\n{} file(s) to keep in sync with the device", files.len());

    Ok(())
}

fn plugins_dir() -> Result<PathBuf> {
    if let Some(arg) = std::env::args().nth(1) {
        return Ok(PathBuf::from(arg));
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .context("workspace root")?
        .join("plugins");
    Ok(root)
}
