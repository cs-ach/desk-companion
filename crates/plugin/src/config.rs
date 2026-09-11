use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::manifest::{ConfigType, Manifest};
use crate::secrets::Secrets;

#[derive(Debug, Clone)]
pub struct ConfigStore {
    dir: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("no user config directory on this platform")]
    NoConfigDir,
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path}: {source}")]
    Parse {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("{path}: top level must be a JSON object")]
    NotAnObject { path: PathBuf },
}

impl ConfigStore {
    pub fn user_default() -> Result<Self, ConfigError> {
        let dir = dirs::config_dir().ok_or(ConfigError::NoConfigDir)?.join("desk");
        Ok(Self::new(dir))
    }

    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir_for(&self, id: &str) -> PathBuf {
        self.dir.join(id)
    }

    pub fn config_path(&self, id: &str) -> PathBuf {
        self.dir_for(id).join("config.json")
    }

    pub fn secrets_path(&self, id: &str) -> PathBuf {
        self.dir_for(id).join("secrets.json")
    }

    pub fn load(&self, manifest: &Manifest) -> Result<PluginConfig, ConfigError> {
        let stored = read_object(self.config_path(&manifest.id))?;
        Ok(resolve(manifest, stored))
    }

    pub fn load_secrets(&self, manifest: &Manifest) -> Result<Secrets, ConfigError> {
        let stored = read_object(self.secrets_path(&manifest.id))?;
        Ok(Secrets::resolve(manifest, stored))
    }

    pub fn save(
        &self,
        manifest: &Manifest,
        values: &Map<String, Value>,
    ) -> Result<(), ConfigError> {
        let mut out = Map::new();
        for field in &manifest.config {
            if field.secret {
                continue;
            }
            if let Some(value) = values.get(&field.key)
                && matches(field.kind, value)
            {
                out.insert(field.key.clone(), value.clone());
            }
        }

        write_json(self.config_path(&manifest.id), &Value::Object(out), false)
    }

    pub fn save_secrets(
        &self,
        manifest: &Manifest,
        values: &Map<String, Value>,
    ) -> Result<(), ConfigError> {
        let mut out = Map::new();
        for field in manifest.config.iter().filter(|f| f.secret) {
            if let Some(Value::String(value)) = values.get(&field.key) {
                out.insert(field.key.clone(), Value::String(value.clone()));
            }
        }

        write_json(self.secrets_path(&manifest.id), &Value::Object(out), true)
    }
}

fn read_object(path: PathBuf) -> Result<Map<String, Value>, ConfigError> {
    match fs::read_to_string(&path) {
        Ok(raw) => match serde_json::from_str::<Value>(&raw) {
            Ok(Value::Object(map)) => Ok(map),
            Ok(_) => Err(ConfigError::NotAnObject { path }),
            Err(source) => Err(ConfigError::Parse { path, source }),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Map::new()),
        Err(source) => Err(ConfigError::Io { path, source }),
    }
}

fn write_json(path: PathBuf, value: &Value, private: bool) -> Result<(), ConfigError> {
    if let Some(parent) = path.parent() {
        create_dir_all(parent)?;
        if private {
            restrict(parent, 0o700)?;
        }
    }

    let body = serde_json::to_string_pretty(value).expect("map serialises");
    fs::write(&path, body).map_err(|source| ConfigError::Io {
        path: path.clone(),
        source,
    })?;

    if private {
        restrict(&path, 0o600)?;
    }
    Ok(())
}

#[cfg(unix)]
fn restrict(path: &Path, mode: u32) -> Result<(), ConfigError> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(|source| ConfigError::Io {
        path: path.to_owned(),
        source,
    })
}

#[cfg(not(unix))]
fn restrict(_path: &Path, _mode: u32) -> Result<(), ConfigError> {
    Ok(())
}

fn create_dir_all(dir: &Path) -> Result<(), ConfigError> {
    fs::create_dir_all(dir).map_err(|source| ConfigError::Io {
        path: dir.to_owned(),
        source,
    })
}

#[derive(Debug, Clone, Default)]
pub struct PluginConfig {
    values: BTreeMap<String, Value>,
    secrets: Vec<String>,

    pub warnings: Vec<String>,
}

impl PluginConfig {
    pub fn host_value(&self, key: &str) -> Option<Value> {
        if self.secrets.iter().any(|k| k == key) {
            return Some(Value::String(format!("{{{{{key}}}}}")));
        }
        self.values.get(key).cloned()
    }

    pub fn device_value(&self, key: &str) -> Option<Value> {
        if self.secrets.iter().any(|k| k == key) {
            return None;
        }
        self.values.get(key).cloned()
    }

    pub fn device_values(&self) -> Map<String, Value> {
        self.values
            .iter()
            .filter(|(k, _)| !self.secrets.iter().any(|s| s == *k))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    pub fn host_values(&self) -> Map<String, Value> {
        let mut out = Map::new();
        for (key, value) in &self.values {
            out.insert(key.clone(), value.clone());
        }
        for key in &self.secrets {
            out.insert(key.clone(), Value::String(format!("{{{{{key}}}}}")));
        }
        out
    }

    pub fn secret_keys(&self) -> &[String] {
        &self.secrets
    }
}

fn resolve(manifest: &Manifest, stored: Map<String, Value>) -> PluginConfig {
    let mut config = PluginConfig::default();

    for field in &manifest.config {
        if field.secret {
            config.secrets.push(field.key.clone());

            continue;
        }

        match stored.get(&field.key) {
            Some(value) if matches(field.kind, value) => {
                config.values.insert(field.key.clone(), value.clone());
            }
            Some(value) => {
                config.warnings.push(format!(
                    "{}: expected {:?}, found {}; using default",
                    field.key,
                    field.kind,
                    kind_of(value)
                ));
                if let Some(default) = &field.default {
                    config.values.insert(field.key.clone(), default.clone());
                }
            }
            None => {
                if let Some(default) = &field.default {
                    config.values.insert(field.key.clone(), default.clone());
                }
            }
        }
    }

    for key in stored.keys() {
        if !manifest.config.iter().any(|f| &f.key == key) {
            config
                .warnings
                .push(format!("{key}: not declared in the manifest; ignored"));
        }
    }

    config
}

fn matches(kind: ConfigType, value: &Value) -> bool {
    match kind {
        ConfigType::String => value.is_string(),
        ConfigType::Number => value.is_number(),
        ConfigType::Boolean => value.is_boolean(),
    }
}

pub(crate) fn kind_of(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn manifest() -> Manifest {
        serde_json::from_value(json!({
            "id": "stockprice",
            "name": "Stock Price",
            "config": [
                { "key": "ticker", "type": "string", "label": "Ticker", "default": "AAPL" },
                { "key": "count", "type": "number", "label": "Count", "default": 10 },
                { "key": "token", "type": "string", "label": "Token", "secret": true }
            ]
        }))
        .unwrap()
    }

    fn store(name: &str) -> ConfigStore {
        let dir = std::env::temp_dir().join(format!("dc-config-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        ConfigStore::new(dir)
    }

    #[test]
    fn missing_file_yields_schema_defaults() {
        let config = store("missing").load(&manifest()).unwrap();
        assert_eq!(config.host_value("ticker"), Some(json!("AAPL")));
        assert_eq!(config.host_value("count"), Some(json!(10)));
        assert!(config.warnings.is_empty());
    }

    #[test]
    fn stored_values_layer_over_defaults() {
        let store = store("layer");
        let manifest = manifest();
        store
            .save(&manifest, json!({ "ticker": "MSFT" }).as_object().unwrap())
            .unwrap();

        let config = store.load(&manifest).unwrap();
        assert_eq!(config.host_value("ticker"), Some(json!("MSFT")));
        assert_eq!(config.host_value("count"), Some(json!(10)));
    }

    #[test]
    fn secrets_never_reach_the_file_or_the_device() {
        let store = store("secrets");
        let manifest = manifest();
        store
            .save(
                &manifest,
                json!({ "ticker": "MSFT", "token": "hunter2" })
                    .as_object()
                    .unwrap(),
            )
            .unwrap();

        let on_disk = fs::read_to_string(store.config_path("stockprice")).unwrap();
        assert!(!on_disk.contains("hunter2"), "secret written to disk");
        assert!(!on_disk.contains("token"), "secret key written to disk");

        let config = store.load(&manifest).unwrap();
        assert_eq!(config.secret_keys(), ["token"]);

        assert_eq!(config.host_value("token"), Some(json!("{{token}}")));
        assert_eq!(config.device_value("token"), None);
        assert!(!config.device_values().contains_key("token"));
        assert!(config.device_values().contains_key("ticker"));
    }

    #[test]
    fn wrong_type_falls_back_to_default_and_warns() {
        let store = store("badtype");
        let manifest = manifest();
        fs::create_dir_all(store.dir_for("stockprice")).unwrap();
        fs::write(store.config_path("stockprice"), r#"{"count":"ten"}"#).unwrap();

        let config = store.load(&manifest).unwrap();
        assert_eq!(config.host_value("count"), Some(json!(10)));
        assert_eq!(config.warnings.len(), 1);
        assert!(config.warnings[0].contains("count"));
    }

    #[test]
    fn secrets_round_trip_through_their_own_file() {
        let store = store("secretfile");
        let manifest = manifest();
        store
            .save(&manifest, json!({ "ticker": "MSFT" }).as_object().unwrap())
            .unwrap();
        store
            .save_secrets(&manifest, json!({ "token": "hunter2" }).as_object().unwrap())
            .unwrap();

        let config_file = fs::read_to_string(store.config_path("stockprice")).unwrap();
        assert!(!config_file.contains("token"), "secret key in config.json");
        assert!(!config_file.contains("hunter2"), "secret value in config.json");

        let secrets_file = fs::read_to_string(store.secrets_path("stockprice")).unwrap();
        assert!(secrets_file.contains("hunter2"));
        assert!(!secrets_file.contains("ticker"));

        let secrets = store.load_secrets(&manifest).unwrap();
        assert_eq!(secrets.get("token"), Some("hunter2"));
    }

    #[test]
    #[cfg(unix)]
    fn secrets_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let store = store("secretperms");
        let manifest = manifest();
        store
            .save_secrets(&manifest, json!({ "token": "hunter2" }).as_object().unwrap())
            .unwrap();

        let mode = fs::metadata(store.secrets_path("stockprice"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "secrets.json is group/world readable");
    }

    #[test]
    fn undeclared_secrets_are_rejected() {
        let store = store("secretstray");
        let manifest = manifest();
        fs::create_dir_all(store.dir_for("stockprice")).unwrap();
        fs::write(
            store.secrets_path("stockprice"),
            r#"{"ticker":"MSFT","nope":"x"}"#,
        )
        .unwrap();

        let secrets = store.load_secrets(&manifest).unwrap();
        assert!(secrets.get("ticker").is_none());
        assert!(secrets.get("nope").is_none());
        assert_eq!(secrets.warnings.len(), 3);
    }

    #[test]
    fn host_values_shows_placeholders_for_secrets() {
        let config = store("hostvalues").load(&manifest()).unwrap();
        let values = config.host_values();

        assert_eq!(values.get("ticker"), Some(&json!("AAPL")));
        assert_eq!(values.get("token"), Some(&json!("{{token}}")));
    }

    #[test]
    fn secrets_debug_does_not_leak_values() {
        let store = store("secretdebug");
        let manifest = manifest();
        store
            .save_secrets(&manifest, json!({ "token": "hunter2" }).as_object().unwrap())
            .unwrap();

        let rendered = format!("{:?}", store.load_secrets(&manifest).unwrap());
        assert!(!rendered.contains("hunter2"), "{rendered}");
        assert!(rendered.contains("token"));
    }

    #[test]
    fn undeclared_keys_are_dropped_with_a_warning() {
        let store = store("stale");
        let manifest = manifest();
        fs::create_dir_all(store.dir_for("stockprice")).unwrap();
        fs::write(store.config_path("stockprice"), r#"{"removed_setting":1}"#).unwrap();

        let config = store.load(&manifest).unwrap();
        assert!(config.host_value("removed_setting").is_none());
        assert_eq!(config.warnings.len(), 1);
    }
}
