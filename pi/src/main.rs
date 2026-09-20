mod link;
mod renderer;
mod desk;

use std::{collections::HashMap, path::PathBuf, sync::{Arc, atomic::{AtomicBool, Ordering}}, thread::JoinHandle};

use anyhow::{Context, Result};
use jsrt::{Entry, Logger, Running, RunningPlugin, RustFunction, Sandbox, globals};
use pi::screen::{DockItem, Screen, ScreenHandle};
use std::path::Path;
use plugin::{FileManifest, Plugin, SyncPlan};
use protocol::FileEntry;

const PLUGIN_MEMORY_LIMIT: usize = 16 * 1024 * 1024;
const DOCK_PLUGINS: usize = 3;
const GEOMETRY_POLL: std::time::Duration = std::time::Duration::from_secs(1);

struct Pi {
    dir: PathBuf,
    plugins: Vec<Arc<Plugin>>,
    running: Running,
    screen: Option<ScreenHandle>,
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
            screen: None,
        })
    }

    pub fn attach(&mut self, screen: ScreenHandle) {
        self.screen = Some(screen);
        self.publish();
    }

    fn loaded(&self) -> Vec<Arc<Plugin>> {
        self.plugins
            .iter()
            .filter(|p| self.running.contains(&p.manifest.id))
            .cloned()
            .collect()
    }

    fn publish(&self) {
        let Some(screen) = &self.screen else {
            return;
        };

        let loaded = self.loaded();
        screen.set_status_trailing(match loaded.len() {
            1 => "1 plugin".to_string(),
            n => format!("{n} plugins"),
        });
        screen.set_dock(dock_from(&loaded));
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

        let gone: Vec<String> = self
            .running
            .ids()
            .filter(|id| !self.plugins.iter().any(|p| &&p.manifest.id == id))
            .cloned()
            .collect();
        for id in &gone {
            self.running.stop(id);
        }

        self.publish();
        Ok(())
    }

    fn spawn(&self, plugin: &Arc<Plugin>) -> RunningPlugin {
        let logger = Arc::new(Logger::new(&plugin.manifest.id, &plugin.logs_dir()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let client = jsrt::HttpClient::new();

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
            RustFunction {
                name: "fetch",
                build: Box::new(move |ctx, plugin| globals::fetch(ctx, plugin, client.clone())),
            },
        ];

        let (calls, handle) = Sandbox::start_thread(
            plugin.clone(),
            PLUGIN_MEMORY_LIMIT,
            functions,
            logger,
            shutdown.clone(),
            Entry::Pi,
        );

        RunningPlugin::new(shutdown, calls, handle)
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
            self.running.insert(id.clone(), running);
        }

        self.publish();
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

        self.publish();
    }

    pub fn stop_all(&mut self) {
        std::mem::take(&mut self.running).stop_all();
        self.publish();
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

fn dock_from(plugins: &[Arc<Plugin>]) -> Vec<DockItem> {
    let mut items = vec![DockItem {
        id: "@home".into(),
        glyph: "home".into(),
        fault: false,
        icon: None,
    }];

    items.extend(plugins.iter().take(DOCK_PLUGINS).map(|plugin| {
        let icon = plugin.root.join("icon.png");

        DockItem {
            id: plugin.manifest.id.clone(),
            glyph: "plugin".into(),
            fault: false,
            icon: icon.is_file().then_some(icon),
        }
    }));

    items
}

fn main() -> Result<()> {
    let mut pi = Pi::new()?;
    pi.load()?;

    let screen = Screen::new()?;

    screen.set_dock(&[]);
    screen.set_status("Desk Companion", false, false, "0 plugins");

    let wallpapers = pi::wallpaper::options(&pi::screen::wallpapers_dir());
    screen.set_wallpaper_options(&wallpapers);

    if let Some((id, path)) = wallpapers.first() {
        if let Err(e) = screen.set_wallpaper(id, path) {
            eprintln!("{e}");
        }
    }

    let handle = screen.handle();
    let choices = wallpapers.clone();
    screen.on_background_changed(move |name| {
        match choices.iter().find(|(id, _)| id == name) {
            Some((id, path)) => handle.set_wallpaper(id, path),
            None => handle.set_background(name),
        }
    });

    let handle = screen.handle();
    screen.on_resized(move |_w, _h| handle.resized());

    let handle = screen.handle();
    let geometry = i_slint_core::timers::Timer::default();
    geometry.start(
        i_slint_core::timers::TimerMode::Repeated,
        GEOMETRY_POLL,
        move || handle.resized(),
    );

    pi.attach(screen.handle());
    
    i_slint_core::timers::Timer::single_shot(std::time::Duration::ZERO, move || {
        std::thread::spawn(move || {
            pi.start_all_pi();
            link::listen(&mut pi);
        });
    });

    screen.run()
}
