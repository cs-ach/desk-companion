pub mod frame;

mod messages;

pub use messages::{FileEntry, HostToPi, LogLevel, PiToHost};

pub const PROTOCOL_VERSION: u16 = 1;
