use std::path::{Path, PathBuf};

use slint_interpreter::Compiler;

/// The shell and the plugin pages both use ComponentContainer, which is
/// experimental in Slint 1.17 and removed from the type register unless this
/// is set. `main()` sets it before any thread exists; a test binary has to set
/// it itself, once, before the first compile.
fn enable_experimental() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| unsafe {
        std::env::set_var("SLINT_ENABLE_EXPERIMENTAL_FEATURES", "1");
    });
}

/// The shell is compiled at runtime by slint-interpreter, so a syntax error in
/// a .slint file is not a build failure -- it is a blank screen on the device.
#[test]
fn screens_compile() {
    enable_experimental();

    let dir = pi::screen::screens_dir();
    if let Err(e) = pi::screen::Screen::check(&dir) {
        panic!("{} did not compile: {e:#}", dir.display());
    }
}

/// Plugin markup is compiled the same way, from the plugin's own directory, so
/// it gets the same guarantee. Each file compiles alone: a plugin cannot
/// import the shell's tokens, and this is what proves it does not try.
#[test]
fn plugin_markup_compiles() {
    enable_experimental();

    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("pi/ has a parent")
        .to_path_buf();

    let mut checked = 0;
    for plugin in read_dirs(&repo.join("plugins")) {
        for file in slint_files(&plugin.join("src")) {
            compile(&file);
            checked += 1;
        }
        for file in slint_files(&plugin.join("src").join("widgets")) {
            compile(&file);
            checked += 1;
        }
    }

    println!("compiled {checked} plugin .slint file(s)");
}

fn compile(file: &Path) {
    let mut compiler = Compiler::default();
    compiler.set_include_paths(vec![file.parent().expect("a parent").to_path_buf()]);

    let result = spin_on::spin_on(compiler.build_from_path(file));

    for diagnostic in result.diagnostics() {
        eprintln!("{diagnostic}");
    }

    assert!(
        !result.has_errors(),
        "{} did not compile",
        file.display()
    );
}

fn read_dirs(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect()
}

fn slint_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|e| e == "slint"))
        .collect()
}
