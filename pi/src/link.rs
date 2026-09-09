use anyhow::Result;
use protocol::{PROTOCOL_VERSION, PiToHost};

pub const GADGET_SERIAL: &str = "/dev/ttyGS0";

pub fn describe() -> Result<()> {
    let hello = PiToHost::Hello {
        protocol_version: PROTOCOL_VERSION,
        device_version: env!("CARGO_PKG_VERSION").to_string(),
    };
    println!(
        "\nlink: {GADGET_SERIAL}, protocol v{PROTOCOL_VERSION}\nhandshake: {}",
        serde_json::to_string(&hello)?
    );
    Ok(())
}
