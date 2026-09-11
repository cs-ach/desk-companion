use std::collections::BTreeMap;
use std::fmt;

use serde_json::{Map, Value};

use crate::manifest::Manifest;

#[derive(Clone, Default)]
pub struct Secrets {
    values: BTreeMap<String, String>,
    pub warnings: Vec<String>,
}

impl Secrets {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.values.keys().map(String::as_str)
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub(crate) fn resolve(manifest: &Manifest, stored: Map<String, Value>) -> Self {
        let mut secrets = Secrets::default();

        for (key, value) in stored {
            let declared = manifest
                .config
                .iter()
                .find(|field| field.key == key && field.secret);

            match (declared, value) {
                (Some(_), Value::String(value)) => {
                    secrets.values.insert(key, value);
                }
                (Some(_), other) => secrets.warnings.push(format!(
                    "{key}: secrets must be strings, found {}; ignored",
                    crate::config::kind_of(&other)
                )),
                (None, _) => secrets.warnings.push(format!(
                    "{key}: not declared in the manifest as a secret; ignored"
                )),
            }
        }

        for field in manifest.config.iter().filter(|f| f.secret) {
            if !secrets.values.contains_key(&field.key) {
                secrets
                    .warnings
                    .push(format!("{}: declared secret has no value set", field.key));
            }
        }

        secrets
    }
}

impl fmt::Debug for Secrets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Secrets")
            .field("keys", &self.values.keys().collect::<Vec<_>>())
            .field("values", &"<redacted>")
            .finish()
    }
}
