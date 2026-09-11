mod desk;
mod http;
mod sync;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use jsrt::{Logger, RustFunction, Sandbox, globals};
use plugin::ConfigStore;

const PLUGIN_MEMORY_LIMIT: usize = 16 * 1024 * 1024;

fn main() -> Result<()> {
    let dir = plugins_dir()?;
    let (plugins, errors) =
        plugin::discover(&dir).with_context(|| format!("scanning {}", dir.display()))?;

    for error in &errors {
        eprintln!("skipped plugin: {error}");
    }

    let store = ConfigStore::user_default()?;
    println!("{} plugin(s) in {}", plugins.len(), dir.display());

    let mut loaded = Vec::with_capacity(plugins.len());
    for p in plugins {
        let sides = if p.manifest.requires_host {
            "host+pi"
        } else {
            "pi only"
        };
        println!("  {:<12} {:<8} {}", p.manifest.id, sides, p.manifest.name);

        let config = store.load(&p.manifest)?;
        let secrets = store.load_secrets(&p.manifest)?;

        for warning in config.warnings.iter().chain(secrets.warnings.iter()) {
            eprintln!("    config: {warning}");
        }

        for (key, value) in config.device_values() {
            println!("    {key} = {value}");
        }
        for key in config.secret_keys() {
            let state = if secrets.get(key).is_some() {
                "set"
            } else {
                "unset"
            };
            println!("    {key} = <secret, {state}, host only>");
        }

        loaded.push(p.with_settings(config, secrets));
    }
    let plugins = loaded;

    let files = sync::device_manifest(&plugins)?;
    println!("\n{} file(s) to keep in sync with the device\n", files.len());

    let mut handles = Vec::new();
    for p in &plugins {

        let client = http::HttpClient::new();
        let logs_dir = store.dir_for(&p.manifest.id).join("logs");
        let logger = Arc::new(Logger::new(&p.manifest.id, &logs_dir));

        let functions = vec![
            RustFunction {
                name: "log",
                build: {
                    let logger = logger.clone();
                    Box::new(move |ctx, _| globals::log(ctx, logger.clone()))
                },
            },
            RustFunction {
                name: "config",
                build: Box::new(|ctx, plugin| {
                    globals::config(ctx, plugin.config.host_values())
                }),
            },
            RustFunction {
                name: "fetch",
                build: Box::new(move |ctx, plugin| {
                    desk::fetch(ctx, plugin, client.clone())
                }),
            },
        ];

        handles.push(Sandbox::start_thread(
            Arc::new(p.clone()),
            PLUGIN_MEMORY_LIMIT,
            functions,
            logger,
        ));
    }

    for handle in handles {
        if handle.join().is_err() {
            eprintln!("a plugin thread panicked");
        }
    }

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
