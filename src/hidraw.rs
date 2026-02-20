use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};

const FRAME_BYTES: usize = 640;
const PACKET_BYTES: usize = 642;

pub struct HidSender {
    vid: u16,
    pid: u16,
    interface: String,
    file: Option<File>,
    packet: [u8; PACKET_BYTES],
}

impl HidSender {
    pub fn new(vid: u16, pid: u16, interface: String) -> Self {
        Self {
            vid,
            pid,
            interface,
            file: None,
            packet: [0; PACKET_BYTES],
        }
    }

    pub fn send_frame(&mut self, frame: &[u8]) -> Result<()> {
        if frame.len() != FRAME_BYTES {
            bail!("invalid frame size: got {}, expected {}", frame.len(), FRAME_BYTES);
        }

        self.ensure_open()?;

        self.packet.fill(0);
        self.packet[0] = 0x61;
        self.packet[1..1 + FRAME_BYTES].copy_from_slice(frame);

        let Some(file) = self.file.as_mut() else {
            bail!("device file unavailable");
        };

        if let Err(err) = file.write_all(&self.packet) {
            self.file = None;
            self.ensure_open()?;
            let retry = self
                .file
                .as_mut()
                .ok_or_else(|| anyhow!("device reopen failed"))?;
            retry
                .write_all(&self.packet)
                .with_context(|| format!("failed to write packet after reconnect: {err}"))?;
        }

        Ok(())
    }

    fn ensure_open(&mut self) -> Result<()> {
        if self.file.is_some() {
            return Ok(());
        }

        let device_path = discover_hidraw(self.vid, self.pid, &self.interface)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&device_path)
            .with_context(|| format!("failed opening {}", device_path))?;
        self.file = Some(file);
        Ok(())
    }
}

fn discover_hidraw(vid: u16, pid: u16, interface: &str) -> Result<String> {
    let root = Path::new("/sys/class/hidraw");
    let entries = fs::read_dir(root).context("cannot read /sys/class/hidraw")?;
    let mut preferred: Option<String> = None;
    let mut fallback: Option<String> = None;

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with("hidraw") {
            continue;
        }

        let hidraw_sys_path = entry.path();
        let uevent_path = hidraw_sys_path.join("device/uevent");
        let uevent = match fs::read_to_string(&uevent_path) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let (dev_vid, dev_pid) = match parse_hid_id(&uevent) {
            Some(v) => v,
            None => continue,
        };

        if dev_vid != vid || dev_pid != pid {
            continue;
        }

        let candidate = format!("/dev/{name}");
        fallback.get_or_insert_with(|| candidate.clone());

        let iface = interface_from_path(&hidraw_sys_path);
        if iface.as_deref() == Some(interface) {
            preferred = Some(candidate);
            break;
        }
    }

    if let Some(path) = preferred {
        return Ok(path);
    }

    if let Some(path) = fallback {
        return Ok(path);
    }

    bail!(
        "Apex5 hidraw device not found (VID {:04X}, PID {:04X}, interface {})",
        vid,
        pid,
        interface
    )
}

fn parse_hid_id(uevent: &str) -> Option<(u16, u16)> {
    for line in uevent.lines() {
        let Some(id) = line.strip_prefix("HID_ID=") else {
            continue;
        };
        let parts: Vec<&str> = id.split(':').collect();
        if parts.len() < 3 {
            continue;
        }
        let vid = u16::from_str_radix(parts[1], 16).ok()?;
        let pid = u16::from_str_radix(parts[2], 16).ok()?;
        return Some((vid, pid));
    }
    None
}

fn interface_from_path(hidraw_sys_path: &Path) -> Option<String> {
    let device_link = fs::canonicalize(hidraw_sys_path.join("device")).ok()?;
    let full = device_link.to_string_lossy();

    for segment in full.split('/') {
        let Some((left, right)) = segment.split_once(':') else {
            continue;
        };
        if !left.contains('-') {
            continue;
        }
        let Some((_, iface_num)) = right.split_once('.') else {
            continue;
        };
        let iface = iface_num.parse::<u8>().ok()?;
        return Some(format!("mi_{iface:02}"));
    }

    None
}
