mod link;
mod renderer;

use std::{collections::HashMap, path::PathBuf, sync::{Arc, atomic::{AtomicBool, Ordering}}, thread::JoinHandle};

use anyhow::{Context, Result};
use jsrt::{Entry, Logger, RustFunction, Sandbox, globals};
use plugin::{FileManifest, Plugin, SyncPlan};
use protocol::FileEntry;

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
    fn stop(&mut self, id: &str) {
        if let Some(running) = self.plugins.remove(id) {
            running.stop(id);
        }
    }

    fn stop_all(self) {
        for running in self.plugins.values() {
            running.signal();
        }
        for (id, running) in self.plugins {
            running.join(&id);
        }
    }
}

struct Pi {
    dir: PathBuf,
    plugins: Vec<Arc<Plugin>>,
    running: Running,
}

impl Pi {

    pub fn new() -> Result<Self> {
        let dir = match std::env::args().nth(1) {
            Some(arg) => std::path::PathBuf::from(arg),
            None => plugin::user_plugins_dir().context("no application data directory")?,
        };
        Ok(Self {
            dir,
            plugins: Vec::new(),
            running: Running::default(),
        })
    }

    pub fn load(&mut self) -> Result<()> {
        std::fs::create_dir_all(&self.dir)
            .with_context(|| format!("creating {}", self.dir.display()))?;

        let (found, errors) = plugin::discover_device(&self.dir)
            .with_context(|| format!("scanning {}", self.dir.display()))?;

        for error in &errors {
            eprintln!("skipped plugin: {error}");
        }

        println!("{} plugin(s) in {}", found.len(), self.dir.display());
        for p in &found {
            let side = if p.manifest.requires_host {
                "host-backed"
            } else {
                "standalone"
            };
            println!("  {:<12} {:<12} {}", p.manifest.id, side, p.manifest.name);
        }

        self.plugins = found.into_iter().map(Arc::new).collect();
        Ok(())
    }

    fn spawn(&self, plugin: &Arc<Plugin>) -> RunningPlugin {
        let logger = Arc::new(Logger::new(&plugin.manifest.id, &plugin.logs_dir()));
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
                build: Box::new(|ctx, plugin| globals::config(ctx, plugin.config.device_values())),
            },
        ];

        let handle = Sandbox::start_thread(
            plugin.clone(),
            PLUGIN_MEMORY_LIMIT,
            functions,
            logger,
            shutdown.clone(),
            Entry::Pi,
        );

        RunningPlugin { shutdown, handle }
    }

    fn start_matching(&mut self, requires_host: bool) -> usize {
        let wanted: Vec<Arc<Plugin>> = self
            .plugins
            .iter()
            .filter(|p| p.manifest.requires_host == requires_host)
            .cloned()
            .collect();

        for plugin in &wanted {
            let id = &plugin.manifest.id;
            self.running.stop(id);
            let running = self.spawn(plugin);
            self.running.plugins.insert(id.clone(), running);
        }

        wanted.len()
    }

    pub fn start_all_pi(&mut self) -> usize {
        let started = self.start_matching(false);
        println!("started {started} standalone plugin(s)");
        started
    }

    pub fn start_all_host(&mut self) -> usize {
        let started = self.start_matching(true);
        println!("started {started} host-backed plugin(s)");
        started
    }

    pub fn stop_all_host(&mut self) {
        let ids: Vec<String> = self
            .plugins
            .iter()
            .filter(|p| p.manifest.requires_host)
            .map(|p| p.manifest.id.clone())
            .collect();

        for id in &ids {
            self.running.stop(id);
        }
    }

    pub fn stop_all(&mut self) {
        std::mem::take(&mut self.running).stop_all();
    }

    pub fn sync_plan(&self, desired: &[FileEntry]) -> Result<(SyncPlan, FileManifest)> {
        let local = self.local_files()?;
        let plan = plugin::sync_plan(desired, &local.files);
        Ok((plan, local))
    }

    pub fn local_files(&self) -> Result<FileManifest> {
        plugin::local_manifest(&self.dir)
            .with_context(|| format!("reading {}", self.dir.display()))
    }
}

fn main() -> Result<()> {
    let mut pi = Pi::new()?;
    pi.load()?;
    pi.start_all_pi();

    link::listen(&mut pi);
}
