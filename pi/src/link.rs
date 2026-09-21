use std::collections::HashMap;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::mpsc::TryRecvError;
use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use base64::Engine;
use plugin::{FileManifest, SyncPlan, safe_join, sha256_hex};
use protocol::frame::{FrameError, read_frame, write_frame};
use protocol::{FileEntry, HostToPi, PROTOCOL_VERSION, PiToHost};
use serialport::{ClearBuffer, SerialPort};

use crate::Pi;

pub type ActionUpdate = (String, String, serde_json::Value);
pub type ActionSender = std::sync::mpsc::Sender<ActionUpdate>;
pub type ActionReceiver = std::sync::mpsc::Receiver<ActionUpdate>;


pub const GADGET_SERIAL: &str = "/dev/ttyGS0";
const READ_TIMEOUT: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_secs(1);
const ANNOUNCE_INTERVAL: Duration = Duration::from_secs(10);
const MAX_SYNC_ROUNDS: u32 = 3;

struct Transfer {
    device_path: String,
    dest: PathBuf,
    expected_sha256: String,
    total: u32,
    next_seq: u32,
    bytes: Vec<u8>,
}

struct Session<'a> {
    pi: &'a mut Pi,
    port: Box<dyn SerialPort>,
    greeted: bool,
    last_announce: Instant,
    last_heartbeat: Instant,
    wanted: HashMap<String, FileEntry>,
    transfer: Option<Transfer>,
    sync_rounds: u32,
}

impl<'a> Session<'a> {
    fn open(pi: &'a mut Pi) -> Result<Self> {
        let port = serialport::new(GADGET_SERIAL, protocol::BAUD_RATE)
            .timeout(READ_TIMEOUT)
            .open()
            .with_context(|| format!("opening {GADGET_SERIAL}"))?;

        port.clear(ClearBuffer::Input)
            .context("clearing stale input")?;

        println!("link: {GADGET_SERIAL} open");

        Ok(Self {
            pi,
            port,
            greeted: false,
            last_announce: Instant::now(),
            last_heartbeat: Instant::now(),
            wanted: HashMap::new(),
            transfer: None,
            sync_rounds: 0,
        })
    }

    fn run(&mut self) -> Result<&'static str> {
        self.announce().context("sending hello")?;

        loop {
            match self.pi.action_rx.try_recv() {
                Ok((module_id, name, payload)) => self.send_action(module_id, name, payload)?,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }

        loop {
            if self.greeted && self.last_heartbeat.elapsed() >= ANNOUNCE_INTERVAL {
                self.send_heartbeat()?;
                self.last_heartbeat = Instant::now();
            }

            match read_frame::<_, HostToPi>(&mut self.port) {
                Ok(message) => self.handle(message)?,

                Err(FrameError::Io(e)) if e.kind() == ErrorKind::TimedOut => {
                    if !self.greeted && self.last_announce.elapsed() >= ANNOUNCE_INTERVAL {
                        self.announce().context("resending hello")?;
                        self.last_announce = Instant::now();
                    }
                }

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

    fn handle(&mut self, message: HostToPi) -> Result<()> {
        match message {
            HostToPi::Hello {
                protocol_version,
                host_version,
            } => {
                self.greeted = true;
                println!("link: host v{host_version} speaking protocol v{protocol_version}");

                if protocol_version != PROTOCOL_VERSION {
                    eprintln!(
                        "link: protocol mismatch, device speaks v{PROTOCOL_VERSION} \
                         and the host speaks v{protocol_version}"
                    );
                }
            }

            HostToPi::SyncManifest { files } => {
                let (plan, local) = self.pi.sync_plan(&files)?;

                self.transfer = None;
                self.sync_rounds = 0;
                self.wanted = files
                    .into_iter()
                    .map(|entry| (entry.path.clone(), entry))
                    .collect();

                remove_extra(&plan.extra, &local);
                self.request_or_ready(plan)?;
            }

            HostToPi::FilePush { path, seq, total, data, } => self.receive_chunk(path, seq, total, &data),

            HostToPi::SyncComplete => {
                if let Some(t) = self.transfer.take() {
                    eprintln!(
                        "link: sync ended with {} incomplete at chunk {}/{}",
                        t.device_path, t.next_seq, t.total
                    );
                }

                let desired: Vec<FileEntry> = self.wanted.values().cloned().collect();
                let (plan, _) = self.pi.sync_plan(&desired)?;
                self.request_or_ready(plan)?;
            }

            HostToPi::Ready => {
                println!("Host Ready starting all host!");
                self.pi.start_all_host();
            }

            HostToPi::State { module_id, name, payload } => {
                let plugin = match self.pi.running.get_plugin(&module_id) {
                    Some(plugin) => plugin,
                    None => {
                        return Ok(());
                    }
                };

                let call = plugin.call("onState".to_owned(), name.clone(), payload.clone());
                if !call {
                    eprintln!("Unable to call onState {name} for {module_id} with payload {payload}")
                }
            }

            other => println!("link: {other:?}"),
        }



        Ok(())
    }

    fn receive_chunk(&mut self, path: String, seq: u32, total: u32, data: &str) {
        let Some(entry) = self.wanted.get(&path) else {
            eprintln!("link: host pushed {path}, which was never in the manifest");
            self.transfer = None;
            return;
        };

        let Some(dest) = safe_join(&self.pi.dir, &path) else {
            eprintln!("link: refusing to write {path}, it escapes the plugin directory");
            self.transfer = None;
            return;
        };

        if total == 0 {
            eprintln!("link: host pushed {path} with no chunks");
            self.transfer = None;
            return;
        }

        let bytes = match base64::engine::general_purpose::STANDARD.decode(data) {
            Ok(bytes) => bytes,
            Err(e) => {
                eprintln!("link: could not decode {path} chunk {seq} ({e})");
                self.transfer = None;
                return;
            }
        };
        println!("{}", data.to_string());

        let starting = seq == 0;
        let continuing = self
            .transfer
            .as_ref()
            .is_some_and(|t| t.device_path == path && t.next_seq == seq && t.total == total);

        if !starting && !continuing {
            eprintln!("link: out of order chunk {seq} for {path}, dropping the transfer");
            self.transfer = None;
            return;
        }

        if starting {
            self.transfer = Some(Transfer {
                device_path: path.clone(),
                dest,
                expected_sha256: entry.sha256.clone(),
                total,
                next_seq: 0,
                bytes: Vec::with_capacity(entry.size_bytes as usize),
            });
        }

        let Some(transfer) = self.transfer.as_mut() else {
            return;
        };

        transfer.bytes.extend_from_slice(&bytes);
        transfer.next_seq += 1;

        if transfer.next_seq < transfer.total {
            return;
        }

        let transfer = self.transfer.take().expect("just checked");
        let actual = sha256_hex(&transfer.bytes);

        if actual != transfer.expected_sha256 {
            eprintln!(
                "link: {} hashed to {actual}, expected {}",
                transfer.device_path, transfer.expected_sha256
            );
            return;
        }

        match install(&transfer.dest, &transfer.bytes) {
            Ok(()) => println!(
                "link: wrote {} ({} bytes)",
                transfer.device_path,
                transfer.bytes.len()
            ),
            Err(e) => eprintln!("link: could not write {} ({e:#})", transfer.device_path),
        }
    }

    fn request_or_ready(&mut self, plan: SyncPlan) -> Result<()> {
        if plan.needed.is_empty() {
            println!("link: in sync with {} file(s)", self.wanted.len());

            self.pi.load()?;
            self.pi.start_all_host();

            return write_frame(&mut self.port, &PiToHost::Ready).context("sending ready");
        }

        if self.sync_rounds >= MAX_SYNC_ROUNDS {
            eprintln!(
                "link: giving up after {MAX_SYNC_ROUNDS} round(s), still missing {:?}",
                plan.needed
            );
            return Ok(());
        }

        self.sync_rounds += 1;
        println!(
            "link: asking for {} file(s), round {}/{MAX_SYNC_ROUNDS}",
            plan.needed.len(),
            self.sync_rounds
        );

        write_frame(&mut self.port, &PiToHost::SyncNeed { paths: plan.needed })
            .context("sending the sync need")
    }

    fn announce(&mut self) -> Result<()> {
        let init = PiToHost::Hello {
            protocol_version: PROTOCOL_VERSION,
            device_version: env!("CARGO_PKG_VERSION").to_string(),
        };
        println!("link: sending hello");
        Ok(write_frame(&mut self.port, &init)?)
    }

    fn send_heartbeat(&mut self) -> Result<()> {
        Ok(write_frame(&mut self.port, &PiToHost::Heartbeat)?)
    }

    fn send_action(&mut self, module_id: String, name: String, payload: serde_json::Value) -> Result<()> {
        Ok(write_frame(&mut self.port, &PiToHost::Action { id: rand::random(), module_id, name, payload })?)
    }

}

fn install(dest: &Path, bytes: &[u8]) -> Result<()> {
    let parent = dest
        .parent()
        .with_context(|| format!("{} has no parent", dest.display()))?;
    let name = dest
        .file_name()
        .with_context(|| format!("{} has no file name", dest.display()))?;

    fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;

    let mut tmp = parent.join(name);
    tmp.as_mut_os_string().push(".part");

    fs::write(&tmp, bytes).with_context(|| format!("writing {}", tmp.display()))?;

    if let Err(e) = fs::rename(&tmp, dest) {
        let _ = fs::remove_file(&tmp);
        return Err(e).with_context(|| format!("renaming into {}", dest.display()));
    }

    Ok(())
}

fn remove_extra(extra: &[String], local: &FileManifest) {
    for device_path in extra {
        let Some(path) = local.source(device_path) else {
            eprintln!("link: no local source for extra file {device_path}, skipping");
            continue;
        };

        match fs::remove_file(path) {
            Ok(()) => println!("link: removed {device_path}"),
            Err(e) if e.kind() == ErrorKind::NotFound => {}
            Err(e) => eprintln!("link: could not remove {device_path} ({e})"),
        }
    }
}

pub fn listen(pi: &mut Pi) -> ! {
    loop {
        match Session::open(pi).and_then(|mut s| s.run()) {
            Ok(reason) => println!("link: {reason}"),
            Err(e) => eprintln!("link: {e:#}"),
        }

        pi.stop_all_host();
        sleep(POLL_INTERVAL);
    }
}
