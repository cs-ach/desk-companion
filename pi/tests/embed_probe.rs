use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use i_slint_core::component_factory::{ComponentFactory, FactoryContext};
use slint_interpreter::{
    Compiler, ComponentDefinition, ComponentHandle, ComponentInstance, Value,
};

fn enable_experimental() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| unsafe {
        std::env::set_var("SLINT_ENABLE_EXPERIMENTAL_FEATURES", "1");
    });
}

fn compile_source(source: &str, name: &str) -> ComponentDefinition {
    let compiler = Compiler::default();
    let result = spin_on::spin_on(compiler.build_from_source(source.into(), Default::default()));

    for d in result.diagnostics() {
        eprintln!("{d}");
    }
    assert!(!result.has_errors(), "{name} did not compile");

    result.component(name).expect("the component is there")
}

fn compile_page(path: &PathBuf) -> ComponentDefinition {
    let mut compiler = Compiler::default();
    compiler.set_include_paths(vec![path.parent().expect("a parent").to_path_buf()]);

    let result = spin_on::spin_on(compiler.build_from_path(path));
    for d in result.diagnostics() {
        eprintln!("{d}");
    }
    assert!(!result.has_errors(), "{} did not compile", path.display());

    result.component("App").expect("App is exported")
}

fn host_shell() -> ComponentDefinition {
    compile_source(
        r#"
        export component Shell inherits Window {
            in property <component-factory> page;
            width: 690px;
            height: 408px;
            ComponentContainer {
                x: 0px; y: 0px; width: 690px; height: 408px;
                component-factory: root.page;
            }
        }
        "#,
        "Shell",
    )
}

/// Grafts `plugin` into a fresh shell and hands back the embedded instance.
fn embed(shell: &ComponentDefinition, plugin: ComponentDefinition) -> (ComponentInstance, ComponentInstance) {
    let instance = shell.create().expect("the shell instantiates");

    // The container takes the component the factory returns, so a clone has to
    // be kept here or there is no way to push state into it later.
    let embedded: Rc<RefCell<Option<ComponentInstance>>> = Rc::new(RefCell::new(None));
    let keep = embedded.clone();

    let factory = ComponentFactory::new(move |ctx: FactoryContext| {
        let made = plugin.create_embedded(ctx).ok()?;
        *keep.borrow_mut() = Some(made.clone_strong());
        Some(made)
    });

    instance
        .set_property("page", Value::ComponentFactory(factory))
        .expect("the factory is accepted");

    // Showing forces the layout pass that asks the container for its component.
    instance.show().expect("showing the shell");

    let made = embedded
        .borrow()
        .as_ref()
        .expect("the factory was called")
        .clone_strong();
    (instance, made)
}

#[test]
fn plugin_pages_embed_and_take_state() {
    enable_experimental();
    let shell = host_shell();

    // 1. The mechanism itself.
    let probe = compile_source(
        r#"
        export component App inherits Rectangle {
            in property <string> headline: "before";
            Text { text: root.headline; }
        }
        "#,
        "App",
    );

    let (shown, made) = embed(&shell, probe);
    made.set_property("headline", Value::String("after".into()))
        .expect("the embedded instance takes a property");
    assert_eq!(
        made.get_property("headline").unwrap(),
        Value::String("after".into()),
        "the value did not stick"
    );
    shown.hide().ok();

    // 2. The real thing: the Spotify page, driven by the properties the shell
    //    maps a state payload onto.
    let app = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("pi/ has a parent")
        .join("plugins/spotify/src/app.slint");

    if !app.is_file() {
        eprintln!("no spotify plugin checked out, skipping the second half");
        return;
    }

    let (shown, page) = embed(&shell, compile_page(&app));

    for (name, value) in [
        ("headline", Value::String("Weightless".into())),
        ("detail", Value::String("Marconi Union".into())),
        ("elapsed_ms", Value::Number(187_000.0)),
        ("total_ms", Value::Number(488_000.0)),
        ("advancing", Value::Bool(true)),
        ("playing", Value::Bool(true)),
    ] {
        page.set_property(name, value.clone())
            .unwrap_or_else(|e| panic!("the page rejected {name}: {e}"));
        assert_eq!(page.get_property(name).unwrap(), value, "{name} did not stick");
    }

    // And the callback the shell hooks to turn a control tap into an action.
    page.set_callback("activated", |_| Value::Void)
        .expect("the page exposes activated");

    shown.hide().ok();
}
