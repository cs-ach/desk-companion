use std::io::{self, Write};
use std::path::PathBuf;

use anyhow::{Context, Result};
use slint_interpreter::{ComponentHandle, Compiler, SharedString, Value};

fn render_once(path: &PathBuf, wanted: &str) -> Result<()> {
    let result = spin_on::spin_on(Compiler::default().build_from_path(path));

    for diagnostic in result.diagnostics() {
        eprintln!("{diagnostic}");
    }

    if result.has_errors() {
        eprintln!("{} did not compile", path.display());
        return Ok(());
    }

    let Some(definition) = result.component(wanted) else {
        let found: Vec<&str> = result.component_names().collect();
        eprintln!("no component named {wanted:?}; {} defines {found:?}", path.display());
        return Ok(());
    };

    let instance = definition
        .create()
        .with_context(|| format!("creating {wanted}"))?;

    let demo = [
        ("headline", "M5.8 -- 120km SSE of Tokyo"),
        ("subtitle", "3 minutes ago, depth 42km"),
        ("status", "set from render.rs"),
    ];

    for (name, text) in demo {
        match instance.set_property(name, Value::String(SharedString::from(text))) {
            Ok(()) => println!("set {name}"),
            Err(e) => eprintln!("could not set {name}: {e}"),
        }
    }

    instance.run().context("running the event loop")?;
    Ok(())
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);

    let mut path = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("app.slint"));
    let wanted = args.next().unwrap_or_else(|| "App".to_owned());

    loop {
        println!("rendering {}", path.display());
        render_once(&path, &wanted)?;

        print!("window closed. enter next .slint path (blank to quit): ");
        io::stdout().flush().ok();

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim();

        if input.is_empty() {
            break;
        }
        path = PathBuf::from(input);
    }

    Ok(())
}