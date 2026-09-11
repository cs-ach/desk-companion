use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::Local;

const FILE_STAMP: &str = "%Y-%m-%dT%H-%M-%S-%3f";
const LINE_STAMP: &str = "%Y-%m-%d %H:%M:%S%.3f";
const MAX_LOG_FILES: usize = 10;

pub struct Logger {
    id: String,
    sink: Option<Mutex<File>>,
}

impl Logger {
    pub fn new(id: &str, dir: &Path) -> Self {
        Self {
            id: id.to_owned(),
            sink: open(id, dir),
        }
    }

    pub fn log(&self, level: &str, message: &str) {
        let id = &self.id;
        println!("[{id}] {level}: {message}");

        let Some(sink) = &self.sink else { return };

        let mut file = sink.lock().unwrap_or_else(|e| e.into_inner());
        let stamp = Local::now().format(LINE_STAMP);

        if let Err(e) = writeln!(file, "{stamp} {level:<5} {message}").and_then(|()| file.flush()) {
            eprintln!("[{id}] log write failed: {e}");
        }
    }

    pub fn info(&self, message: &str){
        self.log("info", message);
    }

    pub fn debug(&self, message: &str){
        self.log("debug", message);
    }

    pub fn error(&self, message: &str) {
        self.log("error", message);
    }

    pub fn warn(&self, message: &str) {
        self.log("warn", message);
    }
}

fn open(id: &str, dir: &Path) -> Option<Mutex<File>> {
    let path: PathBuf = dir.join(format!("{}.log", Local::now().format(FILE_STAMP)));

    match fs::create_dir_all(dir).and_then(|()| File::create(&path)) {
        Ok(file) => {
            prune(id, dir);
            Some(Mutex::new(file))
        }
        Err(e) => {
            eprintln!(
                "[{id}] could not open {}: {e}; logging to the console only",
                path.display()
            );
            None
        }
    }
}

fn prune(id: &str, dir: &Path) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            eprintln!("[{id}] could not list {}: {e}", dir.display());
            return;
        }
    };

    let mut logs: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|t| t.is_file()))
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "log"))
        .collect();

    if logs.len() <= MAX_LOG_FILES {
        return;
    }

    logs.sort();
    let stale = logs.len() - MAX_LOG_FILES;
    for path in logs.into_iter().take(stale) {
        if let Err(e) = fs::remove_file(&path) {
            eprintln!("[{id}] could not remove {}: {e}", path.display());
        }
    }
}
