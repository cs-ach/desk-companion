use std::collections::HashMap;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use base64::Engine;
use plugin::FileManifest;
use protocol::frame::{FrameError, read_frame, write_frame};
use protocol::{HostToPi, PROTOCOL_VERSION, PiToHost};
use serialport::{ClearBuffer, SerialPort};

use crate::Host;

pub const GADGET_SERIAL: &str = "/dev/ttyACM0";
const READ_TIMEOUT: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_secs(1);
const NOT_ALIVE_LIMIT: Duration = Duration::from_secs(30);

pub struct Session<'a> {
    host: &'a mut Host,
    port: Box<dyn SerialPort>,
    last_alive: Instant,
    offered: Option<FileManifest>,
}

impl<'a> Session<'a> {
    fn open(host: &'a mut Host) -> Result<Self> {
        let port = serialport::new(GADGET_SERIAL, protocol::BAUD_RATE)
            .timeout(READ_TIMEOUT)
            .open()
            .with_context(|| format!("opening {GADGET_SERIAL}"))?;

        port.clear(ClearBuffer::Input)
            .context("clearing stale input")?;

        println!("link: {GADGET_SERIAL} open");

        Ok(Self {
            host,
            port,
            last_alive: Instant::now(),
            offered: None,
        })
    }

    fn run(&mut self) -> Result<&'static str> {
        loop {
            if self.last_alive.elapsed() >= NOT_ALIVE_LIMIT {
                eprintln!("link: nothing from the device in {NOT_ALIVE_LIMIT:?}");
                return Ok("device silent");
            }

            match read_frame::<_, PiToHost>(&mut self.port) {
                Ok(message) => self.handle(message)?,

                Err(FrameError::Io(e)) if e.kind() == ErrorKind::TimedOut => continue,

                Err(FrameError::Io(e)) => {
                    eprintln!("link: lost the device ({e})");
                    return Ok("disconnected");
                }
                Err(FrameError::Json(e)) => {
                    eprintln!("link: could not decode a frame ({e})");
                    return Ok("reconnecting");
                }
                Err(e) => return Err(e).context("reading from the device"),
            }
        }
    }

    fn handle(&mut self, message: PiToHost) -> Result<()> {
        self.last_alive = Instant::now();

        match message {
            PiToHost::Hello {
                protocol_version,
                device_version,
            } => {
                println!("link: device v{device_version} speaking protocol v{protocol_version}");

                if protocol_version != PROTOCOL_VERSION {
                    eprintln!(
                        "link: protocol mismatch, host speaks v{PROTOCOL_VERSION} \
                         and the device speaks v{protocol_version}"
                    );
                }

                let reply = HostToPi::Hello {
                    protocol_version: PROTOCOL_VERSION,
                    host_version: env!("CARGO_PKG_VERSION").to_string(),
                };
                write_frame(&mut self.port, &reply).context("replying to hello")?;

                self.send_manifest()?;
            }

            PiToHost::SyncNeed { paths } => {
                println!("link: device needs {} file(s): {paths:?}", paths.len());

                let Some(manifest) = self.offered.as_ref() else {
                    eprintln!("link: device asked for files before a manifest was offered");
                    return Ok(());
                };

                let mut to_send: Vec<(String, PathBuf)> = Vec::with_capacity(paths.len());
                for path in &paths {
                    match manifest.source(path) {
                        Some(real) => to_send.push((path.clone(), real.to_path_buf())),
                        None => eprintln!("link: device asked for {path}, which was never offered"),
                    }
                }

                for (device_path, real) in to_send {
                    let bytes = match fs::read(&real) {
                        Ok(bytes) => bytes,
                        Err(e) => {
                            eprintln!("link: could not read {} ({e}), skipping", real.display());
                            continue;
                        }
                    };

                    self.push_file(&device_path, &bytes)?;
                }

                write_frame(&mut self.port, &HostToPi::SyncComplete)
                    .context("ending the sync")?;
            }

            PiToHost::Ready => {
                println!("link: device reports it is in sync");
                self.host.start_all();
            }

            PiToHost::Heartbeat => {
                println!("link: heartbeat");
            }

            other => println!("link: {other:?}"),
        }

        Ok(())
    }

    pub fn send_state(&mut self, module_id: String, state: HashMap<String, String>) -> Result<()> {
        let payload = serde_json::to_value(&state)?;
        Ok(write_frame(&mut self.port, &HostToPi::State { module_id, payload })?)
    }

    fn push_file(&mut self, device_path: &str, bytes: &[u8]) -> Result<()> {
        let chunks: Vec<&[u8]> = if bytes.is_empty() {
            vec![&[]]
        } else {
            bytes.chunks(protocol::CHUNK_SIZE).collect()
        };
        let total = chunks.len() as u32;

        for (seq, chunk) in chunks.into_iter().enumerate() {
            let push = HostToPi::FilePush {
                path: device_path.to_owned(),
                seq: seq as u32,
                total,
                data: base64::engine::general_purpose::STANDARD.encode(chunk),
            };

            write_frame(&mut self.port, &push)
                .with_context(|| format!("pushing {device_path} chunk {seq}/{total}"))?;
        }

        println!("link: pushed {device_path} ({} bytes, {total} chunk(s))", bytes.len());
        Ok(())
    }

    fn send_manifest(&mut self) -> Result<()> {
        let manifest = self.host.get_sync_manifest()?;
        let files = manifest.files.clone();
        self.offered = Some(manifest);

        write_frame(&mut self.port, &HostToPi::SyncManifest { files })
            .context("sending the sync manifest")?;

        println!("link: offered {} file(s)", self.offered.as_ref().map_or(0, |m| m.files.len()));
        Ok(())
    }
}

pub fn listen(host: &mut Host) -> ! {
    loop {
        wait_for_device();

        match Session::open(host).and_then(|mut s| s.run()) {
            Ok(reason) => println!("link: {reason}"),
            Err(e) => eprintln!("link: {e:#}"),
        }

        sleep(POLL_INTERVAL);
    }
}

fn wait_for_device() {
    if Path::new(GADGET_SERIAL).exists() {
        return;
    }

    println!("link: idle, waiting for {GADGET_SERIAL}");
    while !Path::new(GADGET_SERIAL).exists() {
        sleep(POLL_INTERVAL);
    }
}
