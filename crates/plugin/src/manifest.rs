use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub id: String,
    pub name: String,

    #[serde(default, rename = "appId")]
    pub app_id: Option<String>,

    #[serde(default)]
    pub widgets: Vec<Widget>,

    #[serde(default, rename = "requiresHost")]
    pub requires_host: bool,

    #[serde(default)]
    pub capabilities: Capabilities,

    #[serde(default)]
    pub subscribes: Vec<EventRef>,

    #[serde(default)]
    pub invokes: Vec<InvokeRef>,

    #[serde(default)]
    pub config: Vec<ConfigField>,

    #[serde(default, rename = "tickDeadlineMs")]
    pub tick_deadline_ms: Option<u32>,

    #[serde(default)]
    pub memory_limit_mb: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Widget {
    pub name: String,

    pub size: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Capabilities {
    #[serde(default)]
    pub ipc: Vec<String>,

    #[serde(default)]
    pub http: Vec<String>,

    #[serde(default)]
    pub raw_secrets: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventRef {
    pub module_id: String,
    pub event: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvokeRef {
    pub module_id: String,
    pub action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigField {
    pub key: String,
    #[serde(rename = "type")]
    pub kind: ConfigType,
    pub label: String,
    #[serde(default)]
    pub default: Option<serde_json::Value>,

    #[serde(default)]
    pub secret: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConfigType {
    String,
    Number,
    Boolean,
}
