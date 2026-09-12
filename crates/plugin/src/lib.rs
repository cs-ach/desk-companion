mod config;
mod discovery;
mod manifest;
mod secrets;
mod sync;

use std::path::PathBuf;

pub use config::{ConfigError, ConfigStore, PluginConfig};
pub use discovery::{DiscoveryError, Plugin, discover};
pub use secrets::Secrets;
pub use manifest::{Capabilities, ConfigField, ConfigType, EventRef, InvokeRef, Manifest, Widget};
pub use sync::{device_manifest};

pub fn user_plugins_dir() -> Option<PathBuf> {
    Some(dirs::data_dir()?.join("desk").join("plugins"))
}
