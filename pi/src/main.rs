mod link;
mod renderer;
mod desk;

use std::{collections::HashMap, path::PathBuf, sync::{Arc, atomic::{AtomicBool, Ordering}}, thread::JoinHandle};

use anyhow::{Context, Result};
use jsrt::{Entry, Logger, Running, RunningPlugin, RustFunction, Sandbox, globals};
use pi::screen::{DockItem, Screen, ScreenHandle, SettingsPlugin};
use std::path::Path;
use plugin::{FileManifest, Plugin, SyncPlan};
use protocol::FileEntry;

use crate::link::{
    ActionReceiver, ActionSender, AuthReceiver, AuthSender, RenderReceiver, RenderSender,
    TapReceiver, TapSender,
};

const PLUGIN_MEMORY_LIMIT: usize = 16 * 1024 * 1024;
const DOCK_PLUGINS: usize = 3;
const GEOMETRY_POLL: std::time::Duration = std::time::Duration::from_secs(1);
const CLOCK_POLL: std::time::Duration = std::time::Duration::from_secs(10);

struct Pi {
    dir: PathBuf,
    plugins: Vec<Arc<Plugin>>,
    running: Running,
    screen: Option<ScreenHandle>,
    action_tx: ActionSender,
    action_rx: ActionReceiver,
    auth_tx: AuthSender,
    auth_rx: AuthReceiver,
    tap_tx: TapSender,
    pub tap_rx: TapReceiver,
    render_tx: RenderSender,
    pub render_rx: RenderReceiver,
    auth_states: HashMap<String, bool>,
    page: String,
    states: HashMap<String, serde_json::Value>,
}

impl Pi {

    pub fn new() -> Result<Self> {
        let dir = match std::env::args().nth(1) {
            Some(arg) => std::path::PathBuf::from(arg),
            None => plugin::user_plugins_dir().context("no application data directory")?,
        };
        let (action_tx, action_rx) = std::sync::mpsc::channel();
        let (auth_tx, auth_rx) = std::sync::mpsc::channel();
        let (tap_tx, tap_rx) = std::sync::mpsc::channel();
        let (render_tx, render_rx) = std::sync::mpsc::channel();

        Ok(Self {
            dir,
            plugins: Vec::new(),
            running: Running::default(),
            screen: None,
            action_tx,
            action_rx,
            auth_tx,
            auth_rx,
            tap_tx,
            tap_rx,
            render_tx,
            render_rx,
            auth_states: HashMap::new(),
            page: std::env::var("DESK_PAGE").unwrap_or_else(|_| "@home".into()),
            states: HashMap::new(),
        })
    }

    pub fn attach(&mut self, screen: ScreenHandle) {
        self.screen = Some(screen);
        self.publish();

        let page = self.page.clone();
        self.set_page(page);
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
        screen.set_dock(dock_from(&loaded));
        screen.set_settings_plugins(settings_plugins(
            &self.plugins,
            &self.running,
            &self.auth_states,
        ));
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

    pub fn auths(&self) -> AuthSender {
        self.auth_tx.clone()
    }

    pub fn taps(&self) -> TapSender {
        self.tap_tx.clone()
    }

    pub fn renders(&self) -> RenderSender {
        self.render_tx.clone()
    }

    pub fn tap(&mut self, kind: &str, target: &str) {
        match kind {
            "page" => self.set_page(target.to_owned()),
            "action" => {
                let Some((module_id, name)) = target.split_once(':') else {
                    eprintln!("action {target}: expected module:action");
                    return;
                };

                let Some(plugin) = self.running.get_plugin(module_id) else {
                    eprintln!("action {target}: {module_id} is not running");
                    return;
                };

                let called = plugin.call(
                    "onAction".to_owned(),
                    name.to_owned(),
                    serde_json::Value::Null,
                );

                if !called {
                    eprintln!("action {target}: {module_id} defines no onAction");
                }
            }
            other => eprintln!("tap {other} {target}: unknown kind"),
        }
    }

    pub fn set_page(&mut self, page: String) {
        self.page = page;

        let Some(screen) = &self.screen else {
            return;
        };

        screen.set_page(pi::screen::Page::from_id(&self.page));

        if let Some(plugin) = self.plugins.iter().find(|p| p.manifest.id == self.page) {
            println!("shell: opening {} ({})", self.page, plugin.app_slint().display());
            let initial_state = self.states.get(&self.page).cloned();
            screen.show_plugin_page(plugin.manifest.id.clone(), plugin.app_slint(), initial_state);
        }
    }

    pub fn store_state(&mut self, module_id: String, payload: serde_json::Value) {
        if self.page == module_id
            && let Some(screen) = &self.screen
        {
            screen.set_plugin_state(module_id.clone(), payload.clone());
        }

        match (self.states.get_mut(&module_id), payload) {
            (Some(serde_json::Value::Object(cached)), serde_json::Value::Object(fresh)) => {
                cached.extend(fresh);
            }
            (_, payload) => {
                self.states.insert(module_id, payload);
            }
        }
    }

    pub fn render(&mut self, module_id: String, payload: serde_json::Value) {
        self.store_state(module_id, payload);
    }

    pub fn set_auth_state(&mut self, module_id: String, needs_auth: bool) {
        self.auth_states.insert(module_id, needs_auth);
        self.publish();
    }

    pub fn clear_auth_states(&mut self) {
        self.auth_states.clear();
        self.publish();
    }

    fn spawn(&self, plugin: &Arc<Plugin>) -> RunningPlugin {
        let logger = Arc::new(Logger::new(&plugin.manifest.id, &plugin.logs_dir()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let client = jsrt::HttpClient::new();
        let secrets = jsrt::SecretStore::new(&plugin.secrets);
        let action_tx = self.action_tx.clone();
        let render_tx = self.render_tx.clone();

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
                build: Box::new(move |ctx, plugin| globals::fetch(ctx, plugin, client.clone(), secrets.clone())),
            },
            RustFunction {
                name: "sendAction",
                build: Box::new(move |ctx, plugin| desk::send_action(ctx, plugin, action_tx.clone())),
            },
            RustFunction {
                name: "render",
                build: Box::new(move |ctx, plugin| desk::render(ctx, plugin, render_tx.clone())),
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

fn settings_plugins(
    plugins: &[Arc<Plugin>],
    running: &Running,
    auth_states: &HashMap<String, bool>,
) -> Vec<SettingsPlugin> {
    plugins
        .iter()
        .map(|plugin| {
            let up = running.contains(&plugin.manifest.id);

            SettingsPlugin {
                id: plugin.manifest.id.clone(),
                can_authorize: auth_states
                    .get(&plugin.manifest.id)
                    .copied()
                    .unwrap_or(false),
                name: plugin.manifest.name.clone(),
                status: match (up, auth_states.get(&plugin.manifest.id)) {
                    (_, Some(true)) => "Needs sign-in".to_string(),
                    (true, Some(false)) => "Connected".to_string(),
                    (true, None) => "Running".to_string(),
                    (false, _) if plugin.manifest.requires_host => {
                        "Waiting for the host".to_string()
                    }
                    (false, _) => "Stopped".to_string(),
                },
                running: up,
            }
        })
        .collect()
}

fn now() -> String {
    chrono::Local::now().format("%H:%M").to_string()
}

fn today() -> String {
    chrono::Local::now().format("%A %-d %B").to_string()
}

fn device_line() -> String {
    format!(
        "Device {} \u{b7} protocol v{} \u{b7} {}",
        env!("CARGO_PKG_VERSION"),
        protocol::PROTOCOL_VERSION,
        link::GADGET_SERIAL,
    )
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
    unsafe { std::env::set_var("SLINT_ENABLE_EXPERIMENTAL_FEATURES", "1") };

    let fonts = pi::screen::fonts_dir();
    if fonts.is_dir() {
        let path = match std::env::var_os("SLINT_FONT_PATH") {
            Some(existing) => {
                let mut paths: Vec<PathBuf> = std::env::split_paths(&existing).collect();
                paths.push(fonts);
                std::env::join_paths(paths).ok()
            }
            None => Some(fonts.into_os_string()),
        };

        if let Some(path) = path {
            unsafe { std::env::set_var("SLINT_FONT_PATH", path) };
        }
    }

    let mut pi = Pi::new()?;
    pi.load()?;

    let screen = Screen::new()?;

    screen.set_dock(&[]);
    screen.set_status(&today(), false, false, &now());
    screen.set_device_line(&device_line());

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

    let handle = screen.handle();
    let clock = i_slint_core::timers::Timer::default();
    clock.start(
        i_slint_core::timers::TimerMode::Repeated,
        CLOCK_POLL,
        move || handle.set_clock(now()),
    );

    let handle = screen.handle();
    screen.on_transparency_changed(move |on| handle.set_reduce_transparency(on));

    let auths = pi.auths();
    screen.on_connect_plugin(move |id| {
        let _ = auths.send(id.to_owned());
    });

    let taps = pi.taps();
    screen.on_dock_activated(move |id| {
        let _ = taps.send(("page".to_owned(), id.to_owned()));
    });

    let taps = pi.taps();
    screen.on_plugin_action(move |module_id, action| {
        let _ = taps.send(("action".to_owned(), format!("{module_id}:{action}")));
    });

    pi.attach(screen.handle());
    
    i_slint_core::timers::Timer::single_shot(std::time::Duration::ZERO, move || {
        std::thread::spawn(move || {
            pi.start_all_pi();
            link::listen(&mut pi);
        });
    });

    screen.run()
}
