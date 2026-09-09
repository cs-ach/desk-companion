use std::fs;
use std::path::{Path, PathBuf};

use crate::manifest::Manifest;

#[derive(Debug, Clone)]
pub struct Plugin {
    pub manifest: Manifest,
    pub root: PathBuf,
}

impl Plugin {
    pub fn pi_js(&self) -> PathBuf {
        self.root.join("pi.js")
    }

    pub fn host_js(&self) -> Option<PathBuf> {
        self.manifest
            .requires_host
            .then(|| self.root.join("host.js"))
    }

    pub fn app_slint(&self) -> PathBuf {
        self.root.join("src").join("app.slint")
    }

    pub fn widget_slint(&self, name: &str) -> PathBuf {
        self.root.join("widgets").join(format!("{name}.slint"))
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

        match load(&root) {
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

fn load(root: &Path) -> Result<Plugin, DiscoveryError> {
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
    if !root.join("src").is_dir() {
        return Err(invalid("missing required src/".into()));
    }
    if !root.join("src/app.slint").is_file() {
        return Err(invalid(
            "missing required src/app.slint (the plugin's page)".into(),
        ));
    }

    let host_js = root.join("host.js").is_file();
    match (manifest.requires_host, host_js) {
        (true, false) => return Err(invalid("requiresHost is set but host.js is missing".into())),
        (false, true) => {
            return Err(invalid(
                "host.js is present but requiresHost is not set, so it would never run".into(),
            ));
        }
        _ => {}
    }

    for widget in &manifest.widgets {
        let markup = root.join("widgets").join(format!("{}.slint", widget.name));
        if !markup.is_file() {
            return Err(invalid(format!(
                "widget {:?} is declared but widgets/{}.slint is missing",
                widget.name, widget.name
            )));
        }
    }

    Ok(Plugin {
        manifest,
        root: root.to_owned(),
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
        fs::create_dir_all(root.join("widgets")).unwrap();
        fs::write(root.join("widgets/top.slint"), "").unwrap();

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
