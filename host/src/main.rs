mod desk;
mod link;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool};

use anyhow::{Context, Result};
use jsrt::{Entry, Logger, Running, RunningPlugin, RustFunction, Sandbox, globals};
use plugin::{ConfigStore, FileManifest, Plugin, device_manifest};

use crate::link::{AuthDoneReceiver, AuthDoneSender, StateReceiver, StateSender};

const PLUGIN_MEMORY_LIMIT: usize = 16 * 1024 * 1024;

pub type EventUpdate = (String, String, serde_json::Value);
pub type EventSender = std::sync::mpsc::Sender<EventUpdate>;
pub type EventReceiver = std::sync::mpsc::Receiver<EventUpdate>;

struct Host {
    plugins: Vec<Arc<Plugin>>,
    store: ConfigStore,
    running: Option<Running>,
    state_tx: StateSender,
    state_rx: StateReceiver,
    event_tx: EventSender,
    event_rx: EventReceiver,
    auth_done_tx: AuthDoneSender,
    auth_done_rx: AuthDoneReceiver,
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

        let (state_tx, state_rx) = std::sync::mpsc::channel();
        let (event_tx, event_rx) = std::sync::mpsc::channel();
        let (auth_done_tx, auth_done_rx) = std::sync::mpsc::channel();

        Ok(Self {
            plugins: loaded.into_iter().map(Arc::new).collect(),
            store,
            running: None,
            state_tx,
            state_rx,
            event_tx,
            event_rx,
            auth_done_tx,
            auth_done_rx,
        })
    }

    fn find(&self, id: &str) -> Option<&Arc<Plugin>> {
        self.plugins.iter().find(|p| p.manifest.id == id)
    }

    fn bindings(plugin: &Arc<Plugin>, logger: &Arc<Logger>) -> Vec<RustFunction> {
        let client = jsrt::HttpClient::new();
        let secrets = jsrt::SecretStore::new(&plugin.secrets);

        vec![
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
                build: Box::new(move |ctx, plugin| globals::fetch(ctx, plugin, client.clone(), secrets.clone())),
            },
        ]
    }

    fn spawn(&self, plugin: &Arc<Plugin>) -> RunningPlugin {
        let logger = Arc::new(Logger::new(&plugin.manifest.id, &plugin.logs_dir()));
        let state_tx = self.state_tx.clone();
        let event_tx = self.event_tx.clone();
        let shutdown = Arc::new(AtomicBool::new(false));

        let mut functions = Self::bindings(plugin, &logger);
        functions.push(RustFunction {
            name: "sendState",
            build: Box::new(move |ctx, plugin| desk::send_state(ctx, plugin, state_tx.clone())),
        });

        functions.push(RustFunction { 
            name: "announce", 
            build: Box::new(move |ctx, plugin| desk::announce(ctx, plugin, event_tx.clone()) ) 
        });

        let (calls, handle) = Sandbox::start_thread(
            plugin.clone(),
            PLUGIN_MEMORY_LIMIT,
            functions,
            logger,
            shutdown.clone(),
            Entry::Host,
        );

        RunningPlugin::new(shutdown, calls, handle, plugin.manifest.clone())
    }

    fn start_all(&mut self) {
        self.stop_all();

        let mut running = Running::default();
        for p in &self.plugins {
            running.insert(p.manifest.id.clone(), self.spawn(p));
        }

        println!("started {} plugin(s)", running.len());
        self.running = Some(running);
    }

    pub fn stop_all(&mut self) {
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
            .insert(id.to_owned(), running);
        Ok(())
    }

    fn stop(&mut self, id: &str) {
        if let Some(running) = &mut self.running {
            running.stop(id);
        }
    }
    
    pub fn reload(&mut self, module_id: &str) -> Result<()> {
        let index = self
            .plugins
            .iter()
            .position(|p| p.manifest.id == module_id)
            .with_context(|| format!("no plugin named {module_id}"))?;

        let base = (*self.plugins[index]).clone();
        let (config, secrets) = settings(&self.store, &base)?;
        self.plugins[index] = Arc::new(base.with_settings(config, secrets));

        self.start(module_id)
    }

    fn needs_auth(plugin: &Arc<Plugin>) -> bool {
        if plugin.auth_js().is_none() {
            return false;
        }

        plugin
            .config
            .secret_keys()
            .iter()
            .any(|key| plugin.secrets.get(key).is_none())
    }

    pub fn auth_states(&self) -> Vec<(String, bool)> {
        self.plugins
            .iter()
            .filter(|p| p.auth_js().is_some())
            .map(|p| (p.manifest.id.clone(), Self::needs_auth(p)))
            .collect()
    }

    pub fn authorize(&self, module_id: &str) {
        let Some(plugin) = self.find(module_id).cloned() else {
            eprintln!("authorize: no plugin named {module_id}");
            return;
        };

        if plugin.auth_js().is_none() {
            eprintln!("authorize: {module_id} ships no auth.js");
            return;
        }

        let store = self.store.clone();
        let done = self.auth_done_tx.clone();
        std::thread::spawn(move || {
            let id = plugin.manifest.id.clone();
            match run_auth(plugin, store) {
                Ok(true) => {
                    println!("authorize: {id} finished");
                    let _ = done.send(id);
                }
                Ok(false) => eprintln!("authorize: {id} defines no login() in auth.js"),
                Err(e) => eprintln!("authorize: {id} failed: {e}"),
            }
        });
    }

    pub fn get_sync_manifest(&self) -> Result<FileManifest> {
        device_manifest(self.plugins.iter().map(Arc::as_ref))
            .context("Failed to generate device sync manifest")
    }

}

fn run_auth(plugin: Arc<Plugin>, store: ConfigStore) -> Result<bool> {
    let logger = Arc::new(Logger::new(&plugin.manifest.id, &plugin.logs_dir()));
    let manifest = Arc::new(plugin.manifest.clone());
    let secrets = plugin.secrets.clone();
    let store = Arc::new(store);

    let mut functions = Host::bindings(&plugin, &logger);
    functions.push(RustFunction {
        name: "authorize",
        build: Box::new(|ctx, _| desk::authorize(ctx)),
    });
    functions.push(RustFunction {
        name: "randomString",
        build: Box::new(|ctx, _| desk::random_string(ctx)),
    });
    functions.push(RustFunction {
        name: "sha256Base64Url",
        build: Box::new(|ctx, _| desk::sha256_base64url(ctx)),
    });
    functions.push(RustFunction {
        name: "setSecret",
        build: Box::new(move |ctx, _| {
            desk::set_secret(ctx, manifest.clone(), store.clone(), secrets.clone())
        }),
    });

    Sandbox::run_once(
        plugin.clone(),
        PLUGIN_MEMORY_LIMIT,
        functions,
        logger,
        Entry::Auth,
        "login",
    )
    .with_context(|| format!("running login for {}", plugin.manifest.id))
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
    let thread = std::thread::spawn(move || {
        let mut host = match Host::load() {
            Ok(host) => host,
            Err(e) => {
                eprintln!("Error: {e}");
                return;
            }
        };
        
        link::listen(&mut host);
    });

    let _ = thread.join();
    
    Ok(())

}


fn plugins_dir() -> Result<PathBuf> {
    if let Some(arg) = std::env::args().nth(1) {
        return Ok(PathBuf::from(arg));
    }
    plugin::user_plugins_dir().context("no application data directory on this platform")
}
