use std::{fs, io};
use std::path::{Path, PathBuf};

use crate::config::PluginConfig;
use crate::sync::{DEFAULT_PAGE, PUBLIC_DIR, SRC_DIR, WIDGETS_DIR};
use crate::manifest::Manifest;
use crate::secrets::Secrets;

#[derive(Debug, Clone)]
pub struct Plugin {
    pub manifest: Manifest,
    pub root: PathBuf,
    pub config: PluginConfig,
    pub secrets: Secrets,
}

impl Plugin {
    pub fn with_settings(mut self, config: PluginConfig, secrets: Secrets) -> Self {
        self.config = config;
        self.secrets = secrets;
        self
    }

    pub fn pi_js(&self) -> PathBuf {
        self.root.join("pi.js")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }

    pub fn auth_js(&self) -> Option<PathBuf> {
        let path = self.root.join("auth.js");
        path.is_file().then_some(path)
    }

    pub fn host_js(&self) -> Option<PathBuf> {
        self.manifest
            .requires_host
            .then(|| self.root.join("host.js"))
    }

    pub fn src_dir(&self) -> PathBuf {
        self.root.join(SRC_DIR)
    }

    pub fn pages_dir(&self) -> PathBuf {
        self.src_dir()
    }

    pub fn widgets_dir(&self) -> PathBuf {
        self.src_dir().join(WIDGETS_DIR)
    }

    pub fn public_dir(&self) -> PathBuf {
        self.src_dir().join(PUBLIC_DIR)
    }

    pub fn app_slint(&self) -> PathBuf {
        self.page_slint(DEFAULT_PAGE)
    }

    pub fn page_slint(&self, name: &str) -> PathBuf {
        self.pages_dir().join(format!("{name}.slint"))
    }

    pub fn widget_slint(&self, name: &str) -> PathBuf {
        self.widgets_dir().join(format!("{name}.slint"))
    }

    pub fn pages(&self) -> Result<Vec<String>, io::Error> {
        let mut pages = Vec::new();

        for entry in fs::read_dir(self.pages_dir())? {
            let path = entry?.path();
            if path.extension().is_some_and(|e| e == "slint")
                && let Some(name) = path.file_stem().and_then(|n| n.to_str())
            {
                pages.push(name.to_owned());
            }
        }

        pages.sort_by(|a, b| match (a.as_str(), b.as_str()) {
            (DEFAULT_PAGE, DEFAULT_PAGE) => std::cmp::Ordering::Equal,
            (DEFAULT_PAGE, _) => std::cmp::Ordering::Less,
            (_, DEFAULT_PAGE) => std::cmp::Ordering::Greater,
            _ => a.cmp(b),
        });

        Ok(pages)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DiscoveryError {
    #[error("could not read plugin directory {path}: {source}")]
    ReadDir {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path}: {source}")]
    ReadManifest {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path}: invalid manifest: {source}")]
    ParseManifest {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("{root}: {reason}")]
    Invalid { root: PathBuf, reason: String },
    #[error("plugin id {id:?} is claimed by both {first} and {second}")]
    DuplicateId {
        id: String,
        first: PathBuf,
        second: PathBuf,
    },
}

pub fn discover(dir: &Path) -> Result<(Vec<Plugin>, Vec<DiscoveryError>), DiscoveryError> {
    scan(dir, true)
}

pub fn discover_device(dir: &Path) -> Result<(Vec<Plugin>, Vec<DiscoveryError>), DiscoveryError> {
    scan(dir, false)
}

fn scan(dir: &Path, expect_host_js: bool) -> Result<(Vec<Plugin>, Vec<DiscoveryError>), DiscoveryError> {
    let entries = fs::read_dir(dir).map_err(|source| DiscoveryError::ReadDir {
        path: dir.to_owned(),
        source,
    })?;

    let mut plugins: Vec<Plugin> = Vec::new();
    let mut errors = Vec::new();

    for entry in entries {
        let root = match entry {
            Ok(e) if e.path().is_dir() => e.path(),
            Ok(_) => continue,
            Err(source) => {
                errors.push(DiscoveryError::ReadDir {
                    path: dir.to_owned(),
                    source,
                });
                continue;
            }
        };

        match load(&root, expect_host_js) {
            Ok(plugin) => {
                let clash = plugins
                    .iter()
                    .find(|p| p.manifest.id == plugin.manifest.id)
                    .map(|p| p.root.clone());
                match clash {
                    Some(first) => errors.push(DiscoveryError::DuplicateId {
                        id: plugin.manifest.id,
                        first,
                        second: plugin.root,
                    }),
                    None => plugins.push(plugin),
                }
            }
            Err(e) => errors.push(e),
        }
    }

    Ok((plugins, errors))
}

fn load(root: &Path, expect_host_js: bool) -> Result<Plugin, DiscoveryError> {
    let manifest_path = root.join("manifest.json");
    let raw =
        fs::read_to_string(&manifest_path).map_err(|source| DiscoveryError::ReadManifest {
            path: manifest_path.clone(),
            source,
        })?;
    let manifest: Manifest =
        serde_json::from_str(&raw).map_err(|source| DiscoveryError::ParseManifest {
            path: manifest_path,
            source,
        })?;

    let invalid = |reason: String| DiscoveryError::Invalid {
        root: root.to_owned(),
        reason,
    };

    if manifest.id.is_empty()
        || !manifest
            .id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(invalid(format!(
            "id {:?} must be non-empty lowercase ascii, digits or dashes",
            manifest.id
        )));
    }

    if !root.join("pi.js").is_file() {
        return Err(invalid("missing required pi.js".into()));
    }
    if !root.join(SRC_DIR).is_dir() {
        return Err(invalid(format!("missing required {SRC_DIR}/")));
    }

    if !root
        .join(SRC_DIR)
        .join(format!("{DEFAULT_PAGE}.slint"))
        .is_file()
    {
        return Err(invalid(format!(
            "missing required {SRC_DIR}/{DEFAULT_PAGE}.slint (the plugin's default page)"
        )));
    }

    let host_js = root.join("host.js").is_file();
    match (manifest.requires_host, host_js) {
        (true, false) if expect_host_js => {
            return Err(invalid("requiresHost is set but host.js is missing".into()));
        }
        (false, true) => {
            return Err(invalid(
                "host.js is present but requiresHost is not set, so it would never run".into(),
            ));
        }
        _ => {}
    }

    for widget in &manifest.widgets {
        let markup = root
            .join(SRC_DIR)
            .join(WIDGETS_DIR)
            .join(format!("{}.slint", widget.name));
        if !markup.is_file() {
            return Err(invalid(format!(
                "widget {:?} is declared but {SRC_DIR}/{WIDGETS_DIR}/{}.slint is missing",
                widget.name, widget.name
            )));
        }
    }

    Ok(Plugin {
        manifest,
        root: root.to_owned(),
        config: PluginConfig::default(),
        secrets: Secrets::default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scaffold(dir: &Path, id: &str, manifest: &str) -> PathBuf {
        let root = dir.join(id);
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/app.slint"), "").unwrap();
        fs::write(root.join("pi.js"), "").unwrap();
        fs::write(root.join("manifest.json"), manifest).unwrap();
        root
    }

    fn tempdir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("plugin-discovery-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn loads_a_widgetless_plugin() {
        let dir = tempdir("loads_a_widgetless_plugin");
        scaffold(
            &dir,
            "countdown",
            r#"{"id":"countdown","name":"Countdown","appId":"pub_countdown"}"#,
        );

        let (plugins, errors) = discover(&dir).unwrap();
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(plugins.len(), 1);
        assert!(plugins[0].manifest.widgets.is_empty());
        assert!(plugins[0].host_js().is_none());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn rejects_a_plugin_without_a_page() {
        let dir = tempdir("rejects_a_plugin_without_a_page");
        let root = scaffold(&dir, "broken", r#"{"id":"broken","name":"B"}"#);
        fs::remove_file(root.join("src/app.slint")).unwrap();

        let (plugins, errors) = discover(&dir).unwrap();
        assert!(plugins.is_empty());
        assert!(matches!(
            errors.as_slice(),
            [DiscoveryError::Invalid { .. }]
        ));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn rejects_a_declared_widget_with_no_markup() {
        let dir = tempdir("rejects_a_declared_widget_with_no_markup");
        scaffold(
            &dir,
            "tile",
            r#"{"id":"tile","name":"T","widgets":[{"name":"now","size":"2x2"}]}"#,
        );

        let (plugins, errors) = discover(&dir).unwrap();
        assert!(plugins.is_empty());
        assert!(matches!(
            errors.as_slice(),
            [DiscoveryError::Invalid { .. }]
        ));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn rejects_a_host_js_the_manifest_never_declared() {
        let dir = tempdir("rejects_a_host_js_the_manifest_never_declared");
        let root = scaffold(&dir, "silent", r#"{"id":"silent","name":"S"}"#);
        fs::write(root.join("host.js"), "").unwrap();

        let (plugins, errors) = discover(&dir).unwrap();
        assert!(plugins.is_empty());
        assert!(matches!(
            errors.as_slice(),
            [DiscoveryError::Invalid { .. }]
        ));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn loads_a_host_backed_plugin() {
        let dir = tempdir("loads_a_host_backed_plugin");
        let root = scaffold(
            &dir,
            "hn",
            r#"{"id":"hn","name":"Hacker News","requiresHost":true,
                "widgets":[{"name":"top","size":"4x2"}]}"#,
        );
        fs::write(root.join("host.js"), "").unwrap();
        fs::create_dir_all(root.join("src/widgets")).unwrap();
        fs::write(root.join("src/widgets/top.slint"), "").unwrap();

        let (plugins, errors) = discover(&dir).unwrap();
        assert!(errors.is_empty(), "{errors:?}");
        assert!(plugins[0].host_js().is_some());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn rejects_a_traversal_shaped_id() {
        let dir = tempdir("rejects_a_traversal_shaped_id");
        scaffold(&dir, "evil", r#"{"id":"../etc","name":"E"}"#);

        let (plugins, errors) = discover(&dir).unwrap();
        assert!(plugins.is_empty());
        assert!(matches!(
            errors.as_slice(),
            [DiscoveryError::Invalid { .. }]
        ));
        fs::remove_dir_all(&dir).unwrap();
    }
}
