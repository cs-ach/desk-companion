mod desk;
mod http;
mod link;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;

use anyhow::{Context, Result};
use jsrt::{Entry, Logger, RustFunction, Sandbox, globals};
use plugin::{ConfigStore, FileManifest, Plugin, device_manifest};

const PLUGIN_MEMORY_LIMIT: usize = 16 * 1024 * 1024;

struct RunningPlugin {
    shutdown: Arc<AtomicBool>,
    handle: JoinHandle<()>,
}

impl RunningPlugin {
    fn signal(&self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }

    fn join(self, id: &str) {
        if self.handle.join().is_err() {
            eprintln!("plugin {id} panicked");
        }
    }

    fn stop(self, id: &str) {
        self.signal();
        self.join(id);
    }
}

#[derive(Default)]
struct Running {
    plugins: HashMap<String, RunningPlugin>,
}

impl Running {
    fn stop_all(self) {
        for running in self.plugins.values() {
            running.signal();
        }
        for (id, running) in self.plugins {
            running.join(&id);
        }
    }
}

struct Host {
    plugins: Vec<Arc<Plugin>>,
    store: ConfigStore,
    running: Option<Running>,
}


impl Host {

    pub fn load() -> Result<Self> {
        let dir = plugins_dir()?;
        std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        let (found, errors) =
            plugin::discover(&dir).with_context(|| format!("scanning {}", dir.display()))?;

        for error in &errors {
            eprintln!("skipped plugin: {error}");
        }

        let store = ConfigStore::user_default()?;
        println!("{} plugin(s) in {}", found.len(), dir.display());

        let mut loaded = Vec::with_capacity(found.len());
        for p in found {
            let sides = if p.manifest.requires_host {
                "host+pi"
            } else {
                "pi only"
            };
            println!("  {:<12} {:<8} {}", p.manifest.id, sides, p.manifest.name);

            let (config, secrets) = settings(&store, &p)?;
            loaded.push(p.with_settings(config, secrets));
        }

        let manifest = device_manifest(&loaded)?;
        println!("\n{} file(s) to keep in sync with the device\n", manifest.files.len());

        Ok(Self {
            plugins: loaded.into_iter().map(Arc::new).collect(),
            store,
            running: None,
        })
    }

    fn find(&self, id: &str) -> Option<&Arc<Plugin>> {
        self.plugins.iter().find(|p| p.manifest.id == id)
    }

    fn spawn(&self, plugin: &Arc<Plugin>) -> RunningPlugin {
        let logger = Arc::new(Logger::new(&plugin.manifest.id, &plugin.logs_dir()));
        let client = http::HttpClient::new();
        let shutdown = Arc::new(AtomicBool::new(false));

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
                build: Box::new(|ctx, plugin| globals::config(ctx, plugin.config.host_values())),
            },
            RustFunction {
                name: "fetch",
                build: Box::new(move |ctx, plugin| desk::fetch(ctx, plugin, client.clone())),
            },
        ];

        let handle = Sandbox::start_thread(
            plugin.clone(),
            PLUGIN_MEMORY_LIMIT,
            functions,
            logger,
            shutdown.clone(),
            Entry::Host,
        );

        RunningPlugin { shutdown, handle }
    }

    fn start_all(&mut self) {
        self.stop_all();

        let mut plugins = HashMap::with_capacity(self.plugins.len());
        for p in &self.plugins {
            plugins.insert(p.manifest.id.clone(), self.spawn(p));
        }

        println!("started {} plugin(s)", plugins.len());
        self.running = Some(Running { plugins });
    }

    fn stop_all(&mut self) {
        if let Some(running) = self.running.take() {
            running.stop_all();
        }
    }

    fn start(&mut self, id: &str) -> Result<()> {
        self.stop(id);

        let plugin = self
            .find(id)
            .cloned()
            .with_context(|| format!("no plugin named {id}"))?;
        let running = self.spawn(&plugin);

        self.running
            .get_or_insert_with(Running::default)
            .plugins
            .insert(id.to_owned(), running);
        Ok(())
    }

    fn stop(&mut self, id: &str) {
        if let Some(running) = &mut self.running
            && let Some(plugin) = running.plugins.remove(id)
        {
            plugin.stop(id);
        }
    }
    
    pub fn get_sync_manifest(&self) -> Result<FileManifest> {
        device_manifest(self.plugins.iter().map(Arc::as_ref))
            .context("Failed to generate device sync manifest")
    }

}

fn settings(store: &ConfigStore, plugin: &Plugin) -> Result<(plugin::PluginConfig, plugin::Secrets)> {
    let config = store.load(&plugin.manifest)?;
    let secrets = store.load_secrets(&plugin.manifest)?;

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

    Ok((config, secrets))
}

fn main() -> Result<()> {
    let mut host = Host::load()?;
    
    link::listen(&mut host);

}


fn plugins_dir() -> Result<PathBuf> {
    if let Some(arg) = std::env::args().nth(1) {
        return Ok(PathBuf::from(arg));
    }
    plugin::user_plugins_dir().context("no application data directory on this platform")
}
