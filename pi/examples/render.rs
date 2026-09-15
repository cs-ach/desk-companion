use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use slint_interpreter::{ComponentHandle, Compiler, SharedString, Value};

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);

    let path = args
        .next()
        .map(PathBuf::from)
        .context("usage: render <path/to/app.slint> [component]")?;
    let wanted = args.next().unwrap_or_else(|| "App".to_owned());

    let result = spin_on::spin_on(Compiler::default().build_from_path(&path));

    for diagnostic in result.diagnostics() {
        eprintln!("{diagnostic}");
    }

    if result.has_errors() {
        bail!("{} did not compile", path.display());
    }

    let Some(definition) = result.component(&wanted) else {
        let found: Vec<&str> = result.component_names().collect();
        bail!("no component named {wanted:?}; {} defines {found:?}", path.display());
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
