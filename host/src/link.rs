use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::mpsc::TryRecvError;
use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use base64::Engine;
use plugin::{FileManifest};
use protocol::frame::{FrameError, read_frame, write_frame};
use protocol::{HostToPi, PROTOCOL_VERSION, PiToHost};
use serialport::{ClearBuffer, SerialPort};

use crate::Host;

pub type StateUpdate = (String, String, serde_json::Value);
pub type StateSender = std::sync::mpsc::Sender<StateUpdate>;
pub type StateReceiver = std::sync::mpsc::Receiver<StateUpdate>;

pub type AuthDoneSender = std::sync::mpsc::Sender<String>;
pub type AuthDoneReceiver = std::sync::mpsc::Receiver<String>;

pub const GADGET_SERIAL: &str = "/dev/ttyACM0";
const READ_TIMEOUT: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_secs(1);
const NOT_ALIVE_LIMIT: Duration = Duration::from_secs(30);
const IDLE_POLL: Duration = Duration::from_millis(20);

pub struct Session<'a> {
    host: &'a mut Host,
    port: Box<dyn SerialPort>,
    last_alive: Instant,
    offered: Option<FileManifest>,
    pending_auth_states: bool,
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
            pending_auth_states: false,
        })
    }

    fn run(&mut self) -> Result<&'static str> {
        loop {
            if self.last_alive.elapsed() >= NOT_ALIVE_LIMIT {
                eprintln!("link: nothing from the device in {NOT_ALIVE_LIMIT:?}");
                return Ok("device silent");
            }

            loop {
                match self.host.state_rx.try_recv() {
                    Ok((module_id, name, payload)) => self.send_state(module_id, name, payload)?,
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => break,
                }
            }

            loop {
                match self.host.event_rx.try_recv() {
                    Ok((module_id, event, payload)) => {

                        let Some(running) = self.host.running.as_ref() else {
                            continue;
                        };

                        for plugin in running.get_all() {
                            if plugin.manifest.subscribes.iter().any(|s| s.module_id == module_id && s.event == event) {
                                plugin.call("onEvent".to_owned(), event.clone(), payload.clone());
                            }
                        }

                    },
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => break,
                }
            }

            if self.pending_auth_states {
                self.pending_auth_states = false;
                self.send_auth_states()?;
            }

            loop {
                match self.host.auth_done_rx.try_recv() {
                    Ok(module_id) => {
                        match self.host.reload(&module_id) {
                            Ok(()) => println!("link: reloaded {module_id} with its new credential"),
                            Err(e) => eprintln!("link: could not reload {module_id} ({e:#})"),
                        }
                        self.send_auth_states()?;
                    }
                    Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
                }
            }

            if self.port.bytes_to_read().unwrap_or(1) == 0 {
                sleep(IDLE_POLL);
                continue;
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
                self.pending_auth_states = true;
                println!("link: device reports it is in sync");
                self.host.start_all();
                let message = HostToPi::Ready;
                println!("Pi Ready starting all host!");
                let _ = write_frame(&mut self.port, &message);
            }

            PiToHost::Heartbeat => {
                println!("link: heartbeat");
            }

            PiToHost::Action { id: _, module_id, name, payload } => {
                let Some(plugin) = self.host.running.as_ref().and_then(|r| r.get_plugin(&module_id)) else {
                    eprintln!("action {name}: {module_id} is not running");
                    return Ok(());
                };

                let call = plugin.call("onAction".to_owned(), name.clone(), payload.clone());
                if !call {
                    eprintln!("Unable to call onAction {name} for {module_id} with payload {payload}")
                }

            }

            PiToHost::Authorize { module_id } => {
                println!("link: device asked to authorize {module_id}");
                self.host.authorize(&module_id);
            }

            other => println!("link: {other:?}"),
        }

        Ok(())
    }

    fn send_auth_states(&mut self) -> Result<()> {
        for (module_id, needs_auth) in self.host.auth_states() {
            write_frame(
                &mut self.port,
                &HostToPi::Auth { module_id, needs_auth },
            )
            .context("sending an auth state")?;
        }

        Ok(())
    }

    pub fn send_state(&mut self, module_id: String, name: String, payload: serde_json::Value) -> Result<()> {
        Ok(write_frame(&mut self.port, &HostToPi::State { module_id, name, payload })?)
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

        host.stop_all();

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
