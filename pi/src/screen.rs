use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::wallpaper;

use anyhow::{Context, Result, bail};
use base64::Engine;
use i_slint_core::graphics::{Brush, Color, Image, Rgba8Pixel, SharedPixelBuffer};
use i_slint_core::model::{ModelRc, VecModel};
use i_slint_core::component_factory::{ComponentFactory, FactoryContext};
use slint_interpreter::{
    ComponentHandle, ComponentInstance, Compiler, SharedString, Struct, Value, ValueType, Weak,
};

const DEV_SCREENS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src/screens");
const DEV_WALLPAPERS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/wallpapers");
const DEV_FONTS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fonts");

fn locate(name: &str, dev: &str) -> PathBuf {
    if let Some(root) = std::env::var_os("DESK_ROOT") {
        let candidate = PathBuf::from(root).join(name);
        if candidate.is_dir() {
            return candidate;
        }
    }

    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(PathBuf::from)) {
        let candidate = dir.join(name);
        if candidate.is_dir() {
            return candidate;
        }
    }

    PathBuf::from(dev)
}

pub fn screens_dir() -> PathBuf {
    locate("screens", DEV_SCREENS)
}

pub fn wallpapers_dir() -> PathBuf {
    locate("wallpapers", DEV_WALLPAPERS)
}

pub fn fonts_dir() -> PathBuf {
    locate("fonts", DEV_FONTS)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SlotState {
    #[default]
    Empty,
    Loading,
    Ready,
    Stale,
    Error,
}

impl SlotState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::Loading => "loading",
            Self::Ready => "ready",
            Self::Stale => "stale",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DockSide {
    #[default]
    Left,
    Right,
}

impl DockSide {
    fn is_right(self) -> bool {
        matches!(self, Self::Right)
    }

    pub fn from_right(right: bool) -> Self {
        if right { Self::Right } else { Self::Left }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Page {
    #[default]
    Home,
    Switcher,
    Settings,
    Plugin(String),
}

impl Page {
    fn as_str(&self) -> &str {
        match self {
            Self::Home => "@home",
            Self::Switcher => "@switcher",
            Self::Settings => "@settings",
            Self::Plugin(id) => id,
        }
    }

    pub fn from_id(name: &str) -> Self {
        Self::from_str(name)
    }

    fn from_str(name: &str) -> Self {
        match name {
            "@home" => Self::Home,
            "@switcher" => Self::Switcher,
            "@settings" => Self::Settings,
            other => Self::Plugin(other.to_string()),
        }
    }
}

pub const BACKGROUNDS: [&str; 4] = ["ember", "tide", "violet", "mono"];

#[derive(Debug, Clone, Default)]
pub struct DockItem {
    pub id: String,
    pub glyph: String,
    pub fault: bool,
    pub icon: Option<PathBuf>,
}

#[derive(Debug, Clone, Default)]
pub struct SettingsPlugin {
    pub id: String,
    pub name: String,
    pub status: String,
    pub running: bool,
    pub can_authorize: bool,
}

impl SettingsPlugin {
    fn to_value(&self) -> Value {
        Value::Struct(Struct::from_iter([
            ("id".into(), text(&self.id)),
            ("name".into(), text(&self.name)),
            ("status".into(), text(&self.status)),
            ("running".into(), Value::Bool(self.running)),
            ("can_authorize".into(), Value::Bool(self.can_authorize)),
        ]))
    }
}

#[derive(Debug, Clone, Default)]
pub struct AppEntry {
    pub id: String,
    pub name: String,
    pub glyph: String,
    pub fill: u32,
    pub fault: bool,
    pub icon: Option<PathBuf>,
}

impl AppEntry {
    fn to_value(&self) -> Value {
        let icon = load_icon(self.icon.as_deref());

        Value::Struct(Struct::from_iter([
            ("id".into(), text(&self.id)),
            ("name".into(), text(&self.name)),
            ("glyph".into(), text(&self.glyph)),
            ("fill".into(), brush(self.fill)),
            ("fault".into(), Value::Bool(self.fault)),
            ("icon".into(), Value::Image(icon.clone().unwrap_or_else(wallpaper::empty_image))),
            ("has_icon".into(), Value::Bool(icon.is_some())),
        ]))
    }
}

#[derive(Debug, Clone, Default)]
pub struct Slot {
    pub id: String,
    pub title: String,
    pub glyph: String,
    pub col: i32,
    pub row: i32,
    pub cols: i32,
    pub rows: i32,
    pub state: SlotState,
    pub value: String,
    pub unit: String,
    pub headline: String,
    pub detail: String,
    pub meta: String,
    pub tint: u32,
    pub accent: u32,
    pub focused: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Fault {
    pub body: String,
    pub source: String,
    pub message: String,
    pub last_value: String,
    pub last_headline: String,
    pub last_age: String,
    pub countdown: String,
}

fn load_icon(path: Option<&Path>) -> Option<i_slint_core::graphics::Image> {
    let path = path?;
    match wallpaper::load(path) {
        Ok(image) => Some(image),
        Err(e) => {
            eprintln!("shell: {e}");
            None
        }
    }
}

fn text(v: &str) -> Value {
    Value::String(SharedString::from(v))
}

fn brush(argb: u32) -> Value {
    Value::Brush(Brush::SolidColor(Color::from_argb_encoded(argb)))
}

impl DockItem {
    fn to_value(&self) -> Value {
        let icon = load_icon(self.icon.as_deref());

        Value::Struct(Struct::from_iter([
            ("id".into(), text(&self.id)),
            ("glyph".into(), text(&self.glyph)),
            ("fault".into(), Value::Bool(self.fault)),
            ("icon".into(), Value::Image(icon.clone().unwrap_or_else(wallpaper::empty_image))),
            ("has_icon".into(), Value::Bool(icon.is_some())),
        ]))
    }
}

impl Slot {
    fn to_value(&self) -> Value {
        Value::Struct(Struct::from_iter([
            ("id".into(), text(&self.id)),
            ("title".into(), text(&self.title)),
            ("glyph".into(), text(&self.glyph)),
            ("col".into(), Value::Number(self.col as f64)),
            ("row".into(), Value::Number(self.row as f64)),
            ("cols".into(), Value::Number(self.cols as f64)),
            ("rows".into(), Value::Number(self.rows as f64)),
            ("state".into(), text(self.state.as_str())),
            ("value".into(), text(&self.value)),
            ("unit".into(), text(&self.unit)),
            ("headline".into(), text(&self.headline)),
            ("detail".into(), text(&self.detail)),
            ("meta".into(), text(&self.meta)),
            ("tint".into(), brush(self.tint)),
            ("accent".into(), brush(self.accent)),
            ("focused".into(), Value::Bool(self.focused)),
        ]))
    }
}

fn page_properties(page: &ComponentInstance, payload: &serde_json::Value) -> Vec<(String, Value)> {
    let Some(fields) = payload.as_object() else {
        return Vec::new();
    };

    let declared: HashMap<String, ValueType> = page.definition().properties().collect();
    let mut out = Vec::new();

    for (key, value) in fields {
        let name = snake_case(key);

        let Some(kind) = declared.get(&name) else {
            continue;
        };

        match coerce(value, *kind) {
            Ok(Some(coerced)) => out.push((name, coerced)),
            Ok(None) => {}
            Err(e) => eprintln!("shell: {key} ({e})"),
        }
    }

    out
}

fn coerce(value: &serde_json::Value, kind: ValueType) -> Result<Option<Value>> {
    let coerced = match kind {
        ValueType::String => value.as_str().map(text),
        ValueType::Number => value.as_f64().map(Value::Number),
        ValueType::Bool => value.as_bool().map(Value::Bool),
        ValueType::Image => match value.as_str() {
            Some(data) => Some(Value::Image(decode_image(data)?)),
            None => None,
        },
        _ => None,
    };

    Ok(coerced)
}

fn snake_case(key: &str) -> String {
    let mut out = String::with_capacity(key.len() + 4);

    for c in key.chars() {
        if c.is_ascii_uppercase() {
            if !out.is_empty() {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }

    out
}

fn decode_image(base64_data: &str) -> Result<Image> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(base64_data)
        .context("not valid base64")?;

    let decoded = image::load_from_memory(&bytes)
        .context("not a readable image")?
        .into_rgba8();

    let (width, height) = (decoded.width(), decoded.height());
    let mut buffer = SharedPixelBuffer::<Rgba8Pixel>::new(width, height);

    let raw = decoded.into_raw();
    let expected = buffer.make_mut_bytes().len();
    if raw.len() != expected {
        bail!("{width}x{height} decoded to {} bytes, expected {expected}", raw.len());
    }

    buffer.make_mut_bytes().copy_from_slice(&raw);
    Ok(Image::from_rgba8(buffer))
}

fn model<T>(items: &[T], to_value: impl Fn(&T) -> Value) -> Value {
    Value::Model(ModelRc::new(VecModel::from(
        items.iter().map(to_value).collect::<Vec<_>>(),
    )))
}

fn put(instance: &ComponentInstance, name: &str, value: Value) {
    if let Err(e) = instance.set_property(name, value) {
        eprintln!("shell: could not set {name}: {e}");
    }
}

fn put_token(instance: &ComponentInstance, name: &str, value: Value) {
    if let Err(e) = instance.set_global_property("Tokens", name, value) {
        eprintln!("shell: could not set Tokens.{name}: {e}");
    }
}

fn read_length(instance: &ComponentInstance, name: &str) -> f32 {
    match instance.get_global_property("Tokens", name) {
        Ok(Value::Number(v)) => v as f32,
        _ => f32::NAN,
    }
}

fn sync_size(instance: &ComponentInstance) -> bool {
    let window = instance.window();
    let size = window.size().to_logical(window.scale_factor());

    if size.width <= 0.0 || size.height <= 0.0 {
        return false;
    }

    if read_length(instance, "screen_w") == size.width
        && read_length(instance, "screen_h") == size.height
    {
        return false;
    }

    put_token(instance, "screen_w", Value::Number(size.width as f64));
    put_token(instance, "screen_h", Value::Number(size.height as f64));
    true
}

fn window_pixels(instance: &ComponentInstance) -> (u32, u32) {
    let size = instance.window().size();
    if size.width > 0 && size.height > 0 {
        return (size.width, size.height);
    }

    let w = read_length(instance, "screen_w");
    let h = read_length(instance, "screen_h");
    (w.max(1.0) as u32, h.max(1.0) as u32)
}

fn apply_wallpaper(instance: &ComponentInstance, id: &str, path: &Path) -> Result<()> {
    let (width, height) = window_pixels(instance);
    let prepared = wallpaper::prepare(path, width, height)?;
    put_token(instance, "wallpaper", Value::Image(prepared.sharp));
    put_token(instance, "backdrop_panel", Value::Image(prepared.panel));
    put_token(instance, "backdrop_menu", Value::Image(prepared.menu));
    put_token(instance, "has_wallpaper", Value::Bool(true));
    put_token(instance, "background", text(id));
    Ok(())
}

fn apply_background(instance: &ComponentInstance, name: &str) {
    put_token(instance, "has_wallpaper", Value::Bool(false));
    put_token(instance, "background", text(name));
}

#[derive(Default)]
struct WallpaperState {
    chosen: Option<(String, PathBuf)>,
    baked: Option<(u32, u32)>,
}

type Wallpaper = Arc<Mutex<WallpaperState>>;

fn bake(instance: &ComponentInstance, state: &Wallpaper, force: bool) {
    let mut state = state.lock().expect("wallpaper state poisoned");

    let Some((id, path)) = state.chosen.clone() else {
        return;
    };

    let size = window_pixels(instance);
    if !force && state.baked == Some(size) {
        return;
    }

    match apply_wallpaper(instance, &id, &path) {
        Ok(()) => state.baked = Some(size),
        Err(e) => {
            eprintln!("shell: {e:#}");
            state.baked = None;
        }
    }
}

#[derive(Clone)]
pub struct ScreenHandle {
    weak: Weak<ComponentInstance>,
    wallpaper: Wallpaper,
}

impl ScreenHandle {
    fn dispatch(&self, what: &'static str, f: impl FnOnce(ComponentInstance) + Send + 'static) {
        if let Err(e) = self.weak.upgrade_in_event_loop(f) {
            eprintln!("shell: dropped a {what} update ({e})");
        }
    }

    pub fn set_wallpaper(&self, id: &str, path: &Path) {
        self.wallpaper
            .lock()
            .expect("wallpaper state poisoned")
            .chosen = Some((id.to_string(), path.to_path_buf()));

        let state = self.wallpaper.clone();
        self.dispatch("wallpaper", move |instance| bake(&instance, &state, true));
    }

    pub fn resized(&self) {
        let state = self.wallpaper.clone();
        self.dispatch("geometry", move |instance| {
            if sync_size(&instance) {
                let size = instance.window().size();
                println!("shell: display is {}x{} physical px", size.width, size.height);
            }
            bake(&instance, &state, false);
        });
    }

    pub fn set_background(&self, name: &str) {
        self.wallpaper
            .lock()
            .expect("wallpaper state poisoned")
            .chosen = None;

        let name = name.to_string();
        self.dispatch("background", move |instance| {
            apply_background(&instance, &name)
        });
    }

    pub fn set_page(&self, page: Page) {
        self.dispatch("page", move |instance| {
            put(&instance, "page", text(page.as_str()))
        });
    }

    pub fn set_dock(&self, items: Vec<DockItem>) {
        self.dispatch("dock", move |instance| {
            put(&instance, "dock_items", model(&items, DockItem::to_value));
        });
    }

    pub fn set_clock(&self, clock: String) {
        self.dispatch("clock", move |instance| {
            put(&instance, "clock", text(&clock));
        });
    }

    pub fn set_settings_plugins(&self, plugins: Vec<SettingsPlugin>) {
        self.dispatch("settings plugins", move |instance| {
            put(&instance, "settings_plugins", model(&plugins, SettingsPlugin::to_value));
        });
    }

    pub fn show_plugin_page(
        &self,
        module_id: String,
        app_slint: PathBuf,
        initial_state: Option<serde_json::Value>,
    ) {
        self.dispatch("plugin page", move |instance| {
            let definition = PLUGIN_PAGES.with(|pages| {
                let mut pages = pages.borrow_mut();

                if !pages.definitions.contains_key(&module_id) {
                    match compile_plugin_page(&app_slint) {
                        Ok(def) => {
                            pages.definitions.insert(module_id.clone(), def);
                        }
                        Err(e) => {
                            eprintln!("shell: {module_id} page unavailable ({e:#})");
                            return None;
                        }
                    }
                }

                pages.definitions.get(&module_id).cloned()
            });

            let Some(definition) = definition else {
                put(&instance, "plugin_page_id", text(""));
                return;
            };

            let id = module_id.clone();
            let factory = ComponentFactory::new(move |ctx: FactoryContext| {
                let made = match definition.create_embedded(ctx) {
                    Ok(made) => made,
                    Err(e) => {
                        eprintln!("shell: {id} page could not be embedded ({e})");
                        return None;
                    }
                };

                if let Some(payload) = &initial_state {
                    for (property, value) in page_properties(&made, payload) {
                        let _ = made.set_property(&property, value);
                    }
                }

                PLUGIN_PAGES.with(|pages| {
                    let mut pages = pages.borrow_mut();

                    if let Some(on_action) = pages.on_action.clone() {
                        let id = id.clone();
                        let _ = made.set_callback("activated", move |args| {
                            if let Some(Value::String(name)) = args.first() {
                                on_action(&id, name.as_str());
                            }
                            Value::Void
                        });
                    }

                    pages.instances.insert(id.clone(), made.clone_strong());
                });

                Some(made)
            });

            put(&instance, "plugin_page", Value::ComponentFactory(factory));
            put(&instance, "plugin_page_id", text(&module_id));
        });
    }

    pub fn set_plugin_state(&self, module_id: String, payload: serde_json::Value) {
        self.dispatch("plugin state", move |_instance| {
            PLUGIN_PAGES.with(|pages| {
                let pages = pages.borrow();
                let Some(page) = pages.instances.get(&module_id) else {
                    return;
                };

                for (property, value) in page_properties(page, &payload) {
                    let _ = page.set_property(&property, value);
                }
            });
        });
    }

    pub fn set_device_line(&self, line: String) {
        self.dispatch("device line", move |instance| {
            put(&instance, "device_line", text(&line));
        });
    }

    pub fn set_reduce_transparency(&self, on: bool) {
        let state = self.wallpaper.clone();
        self.dispatch("reduce transparency", move |instance| {
            put_token(&instance, "reduce_transparency", Value::Bool(on));
            bake(&instance, &state, true);
        });
    }
}

#[derive(Default)]
struct PluginPages {
    definitions: HashMap<String, slint_interpreter::ComponentDefinition>,
    instances: HashMap<String, ComponentInstance>,
    on_action: Option<Rc<dyn Fn(&str, &str)>>,
}

thread_local! {
    static PLUGIN_PAGES: RefCell<PluginPages> = RefCell::new(PluginPages::default());
}

fn compile_plugin_page(app_slint: &Path) -> Result<slint_interpreter::ComponentDefinition> {
    let mut compiler = Compiler::default();
    if let Some(dir) = app_slint.parent() {
        compiler.set_include_paths(vec![dir.to_path_buf()]);
    }

    let result = spin_on::spin_on(compiler.build_from_path(app_slint));

    for diagnostic in result.diagnostics() {
        eprintln!("plugin page: {diagnostic}");
    }

    if result.has_errors() {
        bail!("{} did not compile", app_slint.display());
    }

    result
        .component("App")
        .with_context(|| format!("{} defines no component named App", app_slint.display()))
}

pub struct Screen {
    instance: ComponentInstance,
    wallpaper: Wallpaper,
}

impl Screen {
    pub fn check(dir: &Path) -> Result<()> {
        Self::compile(dir).map(|_| ())
    }

    fn compile(dir: &Path) -> Result<slint_interpreter::ComponentDefinition> {
        let entry = dir.join("main.slint");

        let mut compiler = Compiler::default();
        compiler.set_include_paths(vec![dir.to_path_buf()]);

        let result = spin_on::spin_on(compiler.build_from_path(&entry));

        for diagnostic in result.diagnostics() {
            eprintln!("{diagnostic}");
        }

        if result.has_errors() {
            bail!("{} did not compile", entry.display());
        }

        result
            .component("Shell")
            .with_context(|| format!("{} defines no component named Shell", entry.display()))
    }

    pub fn new() -> Result<Self> {
        Self::from_dir(&screens_dir())
    }

    pub fn from_dir(dir: &Path) -> Result<Self> {
        let definition = Self::compile(dir)?;
        let instance = definition.create().context("creating Shell")?;
        Ok(Self {
            instance,
            wallpaper: Wallpaper::default(),
        })
    }

    pub fn set_screen_size(&self, width: f32, height: f32) {
        self.set_token("screen_w", Value::Number(width as f64));
        self.set_token("screen_h", Value::Number(height as f64));
    }

    pub fn size(&self) -> (u32, u32) {
        let size = self.instance.window().size();
        (size.width, size.height)
    }

    pub fn scale_factor(&self) -> f32 {
        self.instance.window().scale_factor()
    }

    fn set(&self, name: &str, value: Value) {
        if let Err(e) = self.instance.set_property(name, value) {
            eprintln!("shell: could not set {name}: {e}");
        }
    }

    fn set_token(&self, name: &str, value: Value) {
        if let Err(e) = self.instance.set_global_property("Tokens", name, value) {
            eprintln!("shell: could not set Tokens.{name}: {e}");
        }
    }

    pub fn set_dock(&self, items: &[DockItem]) {
        self.set("dock_items", model(items, DockItem::to_value));
    }

    pub fn set_dock_side(&self, side: DockSide) {
        self.set_token("dock_right", Value::Bool(side.is_right()));
    }

    pub fn dock_side(&self) -> DockSide {
        match self.instance.get_global_property("Tokens", "dock_right") {
            Ok(Value::Bool(right)) => DockSide::from_right(right),
            _ => DockSide::Left,
        }
    }

    pub fn set_background(&self, name: &str) {
        self.wallpaper
            .lock()
            .expect("wallpaper state poisoned")
            .chosen = None;
        apply_background(&self.instance, name);
    }

    pub fn background(&self) -> String {
        match self.instance.get_global_property("Tokens", "background") {
            Ok(Value::String(name)) => name.to_string(),
            _ => "ember".to_string(),
        }
    }

    pub fn token_length(&self, name: &str) -> f32 {
        match self.instance.get_global_property("Tokens", name) {
            Ok(Value::Number(v)) => v as f32,
            _ => f32::NAN,
        }
    }

    pub fn set_page(&self, page: Page) {
        self.set("page", text(page.as_str()));
    }

    pub fn page(&self) -> Page {
        match self.instance.get_property("page") {
            Ok(Value::String(name)) => Page::from_str(name.as_str()),
            _ => Page::Home,
        }
    }

    pub fn set_apps(&self, apps: &[AppEntry]) {
        self.set("apps", model(apps, AppEntry::to_value));
    }

    pub fn handle(&self) -> ScreenHandle {
        ScreenHandle {
            weak: self.instance.as_weak(),
            wallpaper: self.wallpaper.clone(),
        }
    }

    pub fn set_wallpaper(&self, id: &str, path: &Path) -> Result<()> {
        self.wallpaper
            .lock()
            .expect("wallpaper state poisoned")
            .chosen = Some((id.to_string(), path.to_path_buf()));

        apply_wallpaper(&self.instance, id, path)
    }

    pub fn on_resized(&self, handler: impl Fn(f32, f32) + 'static) {
        let _ = self.instance.set_callback("resized", move |args| {
            if let (Some(Value::Number(w)), Some(Value::Number(h))) = (args.first(), args.get(1)) {
                handler(*w as f32, *h as f32);
            }
            Value::Void
        });
    }

    pub fn set_reduce_transparency(&self, on: bool) {
        self.set_token("reduce_transparency", Value::Bool(on));
    }

    pub fn clear_wallpaper(&self) {
        self.wallpaper
            .lock()
            .expect("wallpaper state poisoned")
            .chosen = None;
        self.set_token("has_wallpaper", Value::Bool(false));
    }

    pub fn set_wallpaper_options(&self, options: &[(String, PathBuf)]) {
        let values: Vec<Value> = options
            .iter()
            .filter_map(|(id, path)| match wallpaper::thumbnail(path, 192, 104) {
                Ok(thumb) => Some(Value::Struct(Struct::from_iter([
                    ("id".into(), text(id)),
                    ("thumb".into(), Value::Image(thumb)),
                ]))),
                Err(e) => {
                    eprintln!("shell: {e}");
                    None
                }
            })
            .collect();
        self.set("wallpapers", Value::Model(ModelRc::new(VecModel::from(values))));
    }

    pub fn on_plugin_picked(&self, handler: impl Fn(&str) + 'static) {
        self.on_id("plugin_picked", handler);
    }

    pub fn set_status(&self, lead: &str, back: bool, linked: bool, clock: &str) {
        self.set("status_lead", text(lead));
        self.set("status_back", Value::Bool(back));
        self.set("linked", Value::Bool(linked));
        self.set("clock", text(clock));
    }

    pub fn set_device_line(&self, line: &str) {
        self.set("device_line", text(line));
    }

    pub fn set_slots(&self, slots: &[Slot]) {
        self.set("slots", model(slots, Slot::to_value));
    }

    pub fn show_fault(&self, fault: &Fault) {
        self.set("fault_body", text(&fault.body));
        self.set("fault_source", text(&fault.source));
        self.set("fault_message", text(&fault.message));
        self.set("fault_last_value", text(&fault.last_value));
        self.set("fault_last_headline", text(&fault.last_headline));
        self.set("fault_last_age", text(&fault.last_age));
        self.set("fault_countdown", text(&fault.countdown));
        self.set("fault_visible", Value::Bool(true));
    }

    pub fn clear_fault(&self) {
        self.set("fault_visible", Value::Bool(false));
    }

    fn on_id(&self, callback: &str, handler: impl Fn(&str) + 'static) {
        let _ = self.instance.set_callback(callback, move |args| {
            if let Some(Value::String(id)) = args.first() {
                handler(id.as_str());
            }
            Value::Void
        });
    }

    pub fn on_dock_activated(&self, handler: impl Fn(&str) + 'static) {
        self.on_id("dock_activated", handler);
    }

    pub fn on_slot_activated(&self, handler: impl Fn(&str) + 'static) {
        self.on_id("slot_activated", handler);
    }

    fn on_bare(&self, callback: &str, handler: impl Fn() + 'static) {
        let _ = self.instance.set_callback(callback, move |_| {
            handler();
            Value::Void
        });
    }

    pub fn on_dock_side_changed(&self, handler: impl Fn(DockSide) + 'static) {
        let _ = self.instance.set_callback("dock_side_changed", move |args| {
            if let Some(Value::Bool(right)) = args.first() {
                handler(DockSide::from_right(*right));
            }
            Value::Void
        });
    }

    pub fn on_background_changed(&self, handler: impl Fn(&str) + 'static) {
        self.on_id("background_changed", handler);
    }

    pub fn on_plugin_action(&self, handler: impl Fn(&str, &str) + 'static) {
        PLUGIN_PAGES.with(|pages| {
            pages.borrow_mut().on_action = Some(Rc::new(handler));
        });
    }

    pub fn on_connect_plugin(&self, handler: impl Fn(&str) + 'static) {
        self.on_id("connect_plugin", handler);
    }

    pub fn on_transparency_changed(&self, handler: impl Fn(bool) + 'static) {
        let _ = self.instance.set_callback("transparency_changed", move |args| {
            if let Some(Value::Bool(on)) = args.first() {
                handler(*on);
            }
            Value::Void
        });
    }

    pub fn on_back(&self, handler: impl Fn() + 'static) {
        self.on_bare("back", handler);
    }

    pub fn on_retry(&self, handler: impl Fn() + 'static) {
        self.on_bare("retry", handler);
    }

    pub fn on_home(&self, handler: impl Fn() + 'static) {
        self.on_bare("home", handler);
    }

    pub fn run(self) -> Result<()> {
        self.instance.run().context("running the event loop")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_base64(w: u32, h: u32) -> String {
        let mut bytes = Vec::new();
        image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(w, h, image::Rgba([9, 8, 7, 255])))
            .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
            .expect("encode");
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    #[test]
    fn decodes_base64_png() {
        let image = decode_image(&png_base64(7, 3)).expect("decode");
        assert_eq!(image.size().width, 7);
        assert_eq!(image.size().height, 3);
    }

    #[test]
    fn rejects_junk_without_panicking() {
        assert!(decode_image("not base64!!").is_err());
        assert!(decode_image(&base64::engine::general_purpose::STANDARD.encode("not an image")).is_err());
    }

    #[test]
    fn camel_case_keys_become_snake_case_properties() {
        assert_eq!(snake_case("elapsedMs"), "elapsed_ms");
        assert_eq!(snake_case("headline"), "headline");
        assert_eq!(snake_case("totalMs"), "total_ms");
        assert_eq!(snake_case("has_image"), "has_image");
        assert_eq!(snake_case("Image"), "image");
    }

    #[test]
    fn coerces_to_the_declared_type() {
        let s = serde_json::json!("hello");
        let n = serde_json::json!(42);
        let b = serde_json::json!(true);

        assert!(matches!(coerce(&s, ValueType::String), Ok(Some(Value::String(_)))));
        assert!(matches!(coerce(&n, ValueType::Number), Ok(Some(Value::Number(_)))));
        assert!(matches!(coerce(&b, ValueType::Bool), Ok(Some(Value::Bool(true)))));

        assert!(matches!(coerce(&s, ValueType::Number), Ok(None)));
        assert!(matches!(coerce(&n, ValueType::Bool), Ok(None)));
    }

    #[test]
    fn coerces_base64_into_an_image() {
        let data = serde_json::json!(png_base64(2, 2));
        assert!(matches!(coerce(&data, ValueType::Image), Ok(Some(Value::Image(_)))));

        let junk = serde_json::json!("not an image");
        assert!(coerce(&junk, ValueType::Image).is_err());
    }
}
