pub mod frame;

mod messages;

pub use messages::{FileEntry, HostToPi, LogLevel, PiToHost};

pub const PROTOCOL_VERSION: u16 = 1;
pub const CHUNK_SIZE: usize = 4 * 1024;