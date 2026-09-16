use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use pi::screen::{AppEntry, DockItem, DockSide, Fault, Page, Screen, Slot, SlotState};

const ALERT: u32 = 0xFF_FF453A;
const WHITE: u32 = 0xFF_FFFFFF;
const TINT_RED: u32 = 0x3D_FF453A;
const CLEAR: u32 = 0x00_000000;
const INDIGO: u32 = 0xFF_5E5CE6;
const GREEN: u32 = 0xFF_30D158;
const BLUE: u32 = 0xFF_0A84FF;

const PLUGINS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../plugins");

fn app(id: &str, name: &str, glyph: &str, fill: u32, fault: bool) -> AppEntry {
    let icon = PathBuf::from(PLUGINS).join(id).join("icon.png");

    AppEntry {
        id: id.into(),
        name: name.into(),
        glyph: glyph.into(),
        fill,
        fault,
        icon: icon.is_file().then_some(icon),
    }
}

fn wallpaper_options(dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    let mut found: Vec<(String, PathBuf)> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "png" || x == "jpg"))
        .filter_map(|p| {
            p.file_stem()
                .and_then(|s| s.to_str())
                .map(|s| (s.to_string(), p.clone()))
        })
        .collect();

    found.sort_by(|a, b| a.0.cmp(&b.0));
    found
}

fn dock(id: &str, glyph: &str, fault: bool) -> DockItem {
    let icon = PathBuf::from(PLUGINS).join(id).join("icon.png");

    DockItem {
        id: id.into(),
        glyph: glyph.into(),
        fault,
        icon: icon.is_file().then_some(icon),
    }
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let fault = args.iter().any(|a| a == "--fault");

    if args.iter().any(|a| a == "--check") {
        Screen::check(&pi::screen::screens_dir())?;
        println!("ok: shell compiles");
        return Ok(());
    }

    let screen = Screen::new()?;

    if let Some(i) = args.iter().position(|a| a == "--size") {
        let spec = args.get(i + 1).context("--size needs WxH, e.g. 1024x600")?;
        let (w, h) = spec.split_once('x').context("--size needs WxH, e.g. 1024x600")?;
        screen.set_screen_size(w.parse()?, h.parse()?);
    }

    if args.iter().any(|a| a == "--geometry") {
        for side in [DockSide::Left, DockSide::Right] {
            screen.set_dock_side(side);
            println!(
                "{:<6} dock_x {:>5}  content_x {:>5}  content_w {:>5}  slot {:>5} x {:>5}",
                format!("{side:?}"),
                screen.token_length("dock_x"),
                screen.token_length("content_x"),
                screen.token_length("content_w"),
                screen.token_length("content_w"),
                screen.token_length("slot_h"),
            );
        }
        println!(
            "screen {} x {}  cell {} x {}",
            screen.token_length("screen_w"),
            screen.token_length("screen_h"),
            screen.token_length("cell_w"),
            screen.token_length("cell_h"),
        );
        {
        }
        return Ok(());
    }

    screen.set_dock(&[
        dock("@home", "home", false),
        dock("quake", "wave", fault),
        dock("hello", "chat", false),
        dock("http-demo", "globe", false),
    ]);

    screen.set_apps(&[
        app("quake", "Earthquake", "wave", ALERT, fault),
        app("clock", "Clock", "clock", INDIGO, false),
        app("hello", "Hello", "chat", GREEN, false),
        app("http-demo", "HTTP Demo", "globe", BLUE, false),
    ]);

    let wallpapers = wallpaper_options(&pi::screen::wallpapers_dir());
    screen.set_wallpaper_options(&wallpapers);

    if let Some(i) = args.iter().position(|a| a == "--wallpaper") {
        if let Some(name) = args.get(i + 1) {
            match wallpapers.iter().find(|(id, _)| id == name) {
                Some((id, path)) => screen.set_wallpaper(id, path)?,
                None => eprintln!("no wallpaper named {name}"),
            }
        }
    }

    if args.iter().any(|a| a == "--opaque") {
        screen.set_reduce_transparency(true);
    }

    if args.iter().any(|a| a == "--right") {
        screen.set_dock_side(DockSide::Right);
    }

    if let Some(i) = args.iter().position(|a| a == "--background") {
        if let Some(name) = args.get(i + 1) {
            screen.set_background(name);
        }
    }

    if args.iter().any(|a| a == "--settings") {
        screen.set_page(Page::Settings);
    }

    if args.iter().any(|a| a == "--switcher") {
        screen.set_page(Page::Switcher);
    }

    if fault {
        screen.set_status("Earthquake Watch", true, true, "4 plugins");
        screen.show_fault(&Fault {
            body: "Earthquake Watch is still running and its data is current — only its screen failed to compile.".into(),
            source: "plugins/quake/src/pages/app.slint:21".into(),
            message: "unknown property 'colour' in Text".into(),
            last_value: "5.8".into(),
            last_headline: "120 km SSE of Tokyo".into(),
            last_age: "last reading · 4 min ago".into(),
            countdown: "Retrying in 18s".into(),
        });
    } else {
        screen.set_status("Tuesday, 16 September", false, true, "4 plugins");
        let plugins = vec![
            Slot {
                id: "quake".into(),
                title: "Earthquake Watch".into(),
                glyph: "wave".into(),
                col: 0,
                row: 0,
                cols: 2,
                rows: 2,
                state: SlotState::Ready,
                value: "5.8".into(),
                unit: "Mw".into(),
                headline: "120 km SSE of Tokyo".into(),
                detail: "42 km deep · over your 2.5 threshold".into(),
                meta: "3 min".into(),
                tint: TINT_RED,
                accent: ALERT,
                focused: true,
            },
            Slot {
                id: "clock".into(),
                title: "Clock".into(),
                glyph: "home".into(),
                col: 2,
                row: 0,
                cols: 2,
                rows: 1,
                state: SlotState::Ready,
                value: "14:32".into(),
                headline: "Sunset 18:52".into(),
                tint: CLEAR,
                accent: WHITE,
                ..Slot::default()
            },
            Slot {
                id: "hello".into(),
                title: "Hello".into(),
                glyph: "chat".into(),
                col: 2,
                row: 1,
                cols: 1,
                rows: 1,
                state: SlotState::Empty,
                ..Slot::default()
            },
            Slot {
                id: "http-demo".into(),
                title: "HTTP Demo".into(),
                glyph: "globe".into(),
                col: 3,
                row: 1,
                cols: 1,
                rows: 1,
                state: SlotState::Loading,
                ..Slot::default()
            },
        ];

        screen.set_slots(&plugins);
    }

    screen.on_dock_activated(|id| println!("dock: {id}"));
    screen.on_dock_side_changed(|side| println!("dock side: {side:?}"));
    screen.on_plugin_picked(|id| println!("picked: {id}"));
    let handle = screen.handle();
    let choices = wallpapers.clone();
    screen.on_background_changed(move |name| {
        match choices.iter().find(|(id, _)| id == name) {
            Some((id, path)) => handle.set_wallpaper(id, path),
            None => handle.set_background(name),
        }
        println!("background: {name}");
    });
    screen.on_slot_activated(|id| println!("slot: {id}"));
    screen.on_back(|| println!("back"));
    screen.on_retry(|| println!("retry"));
    screen.on_home(|| println!("home"));

    screen.run()
}
