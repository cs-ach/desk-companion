mod config;
mod discovery;
mod manifest;

pub use config::{ConfigError, ConfigStore, PluginConfig};
pub use discovery::{DiscoveryError, Plugin, discover};
pub use manifest::{Capabilities, ConfigField, ConfigType, EventRef, InvokeRef, Manifest, Widget};
