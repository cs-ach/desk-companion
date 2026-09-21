use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HostToPi {
    Hello {
        protocol_version: u16,
        host_version: String,
    },

    State {
        module_id: String,
        name: String,
        payload: Value,
    },

    Config {
        module_id: String,
        values: Value,
    },

    Heartbeat,

    SyncManifest {
        files: Vec<FileEntry>,
    },

    FilePush {
        path: String,
        seq: u32,
        total: u32,
        data: String,
    },

    SyncComplete,

    WallpaperRequest,

    WallpaperComplete,

    UpdateBegin {
        version: String,
        size_bytes: u64,
        sha256: String,
    },
    UpdateChunk {
        seq: u32,
        data: String,
    },
    UpdateComplete,
    
    Ready,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PiToHost {
    Hello {
        protocol_version: u16,
        device_version: String,
    },

    Action {
        id: u32,
        module_id: String,
        name: String,
        payload: Value,
    },

    Heartbeat,

    StateRequest,

    Ready,

    SyncNeed {
        paths: Vec<String>,
    },

    Log {
        module_id: Option<String>,
        level: LogLevel,
        message: String,
    },

    UpdateAck {
        seq: u32,
    },
    UpdateResult {
        success: bool,
        error: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileEntry {
    pub path: String,
    pub sha256: String,
    pub size_bytes: u64,
}
