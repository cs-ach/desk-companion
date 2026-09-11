mod config;
mod discovery;
mod manifest;
mod secrets;

pub use config::{ConfigError, ConfigStore, PluginConfig};
pub use discovery::{DiscoveryError, Plugin, discover};
pub use secrets::Secrets;
pub use manifest::{Capabilities, ConfigField, ConfigType, EventRef, InvokeRef, Manifest, Widget};
