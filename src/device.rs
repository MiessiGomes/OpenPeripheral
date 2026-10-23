use crate::error::{Error, Result};
use crate::ids::{AUDIO_HID_INTERFACE, AUDIO_PID, CONTROLLER_PID, HP_VID, LIGHTING_INTERFACE};
use crate::protocol::{
    apply_packet, audio_command_packet, audio_ping_ok, audio_ping_packet, audio_reply_value,
    color_packet, eot_packet, post_save_packet, reset_lights_packet, save_begin_packet,
    save_stop_packet, PolarPattern, Rgb, AUDIO_GET_GAIN, AUDIO_GET_HIGHPASS, AUDIO_GET_MUTE,
    AUDIO_GET_POLAR, AUDIO_GET_SIDETONE, AUDIO_SET_GAIN, AUDIO_SET_HIGHPASS, AUDIO_SET_MUTE,
    AUDIO_SET_POLAR, AUDIO_SET_SIDETONE, PACKET_SIZE,
};
use hidapi::{DeviceInfo, HidApi, HidDevice};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

const GAP: Duration = Duration::from_millis(15);

pub struct Lighting {
    dev: HidDevice,
}

enum AudioTransport {
    Hidraw(File),
    Hidapi(HidDevice),
}

pub struct AudioHid {
    dev: AudioTransport,
}

#[derive(Clone, Debug)]
pub struct FoundDevice {
    pub vid: u16,
    pub pid: u16,
    pub interface: i32,
    pub product: String,
    pub path: String,
}

fn info_row(info: &DeviceInfo) -> FoundDevice {
    FoundDevice {
        vid: info.vendor_id(),
        pid: info.product_id(),
        interface: info.interface_number(),
        product: info.product_string().unwrap_or("").to_string(),
        path: info.path().to_string_lossy().into_owned(),
    }
}

pub fn enumerate(api: &HidApi) -> Vec<FoundDevice> {
    api.device_list()
        .filter(|d| {
            d.vendor_id() == HP_VID
                && (d.product_id() == CONTROLLER_PID || d.product_id() == AUDIO_PID)
        })
        .map(info_row)
        .collect()
}

fn open_iface(api: &HidApi, pid: u16, iface: i32) -> Result<HidDevice> {
    let info = api
        .device_list()
        .find(|d| d.vendor_id() == HP_VID && d.product_id() == pid && d.interface_number() == iface)
        .ok_or(if pid == CONTROLLER_PID {
            Error::LightingNotFound
        } else {
            Error::AudioHidNotFound
        })?;
    Ok(api.open_path(info.path())?)
}

impl Lighting {
    pub fn open(api: &HidApi) -> Result<Self> {
        Ok(Self {
            dev: open_iface(api, CONTROLLER_PID, LIGHTING_INTERFACE)?,
        })
    }

    fn send(&self, pkt: &[u8; PACKET_SIZE]) -> Result<()> {
        self.dev.send_feature_report(pkt)?;
        thread::sleep(GAP);
        Ok(())
    }

    pub fn set_direct(&self, top: Rgb, bottom: Rgb) -> Result<()> {
        self.send(&color_packet(top, bottom))?;
        self.send(&apply_packet())?;
        Ok(())
    }

    pub fn save_solid(&self, top: Rgb, bottom: Rgb) -> Result<()> {
        self.send(&save_begin_packet(1))?;
        self.send(&color_packet(top, bottom))?;
        self.send(&post_save_packet())?;
        self.send(&save_stop_packet())?;
        self.send(&eot_packet(1))?;
        self.send(&post_save_packet())?;
        self.set_direct(top, bottom)?;
        Ok(())
    }

    pub fn off(&self, persist: bool) -> Result<()> {
        if persist {
            self.save_solid(Rgb::OFF, Rgb::OFF)
        } else {
            self.set_direct(Rgb::OFF, Rgb::OFF)
        }
    }

    pub fn firmware_reset(&self) -> Result<()> {
        self.send(&reset_lights_packet())?;
        thread::sleep(Duration::from_millis(1000));
        Ok(())
    }
}

impl AudioHid {
    pub fn open() -> Result<Self> {
        match open_audio_hidraw() {
            Ok(file) => Ok(Self {
                dev: AudioTransport::Hidraw(file),
            }),
            Err(hidraw_err) => {
                let api = HidApi::new()?;
                match open_iface(&api, AUDIO_PID, AUDIO_HID_INTERFACE) {
                    Ok(dev) => Ok(Self {
                        dev: AudioTransport::Hidapi(dev),
                    }),
                    Err(libusb_err) => Err(Error::Other(format!("{hidraw_err}; {libusb_err}"))),
                }
            }
        }
    }

    fn write_bytes(&self, pkt: &[u8]) -> Result<()> {
        match &self.dev {
            AudioTransport::Hidraw(f) => {
                (&*f)
                    .write_all(pkt)
                    .map_err(|e| Error::Other(format!("hidraw write: {e}")))?;
            }
            AudioTransport::Hidapi(d) => {
                d.write(pkt)?;
            }
        }
        thread::sleep(Duration::from_millis(50));
        Ok(())
    }

    fn read_bytes(&self, timeout_ms: i32) -> Result<Vec<u8>> {
        let mut buf = [0u8; 64];
        let n = match &self.dev {
            AudioTransport::Hidraw(f) => poll_read(f, &mut buf, timeout_ms)?,
            AudioTransport::Hidapi(d) => d.read_timeout(&mut buf, timeout_ms)?,
        };
        Ok(buf[..n].to_vec())
    }

    fn write_cmd(&self, command: u8, payload: u8) -> Result<()> {
        self.write_bytes(&audio_command_packet(command, payload))
    }

    fn query(&self, command: u8) -> Result<u8> {
        self.write_cmd(command, 0)?;
        let reply = self.read_bytes(200)?;
        audio_reply_value(&reply, command).ok_or_else(|| {
            Error::Other(format!(
                "resposta HID inesperada para {command:#04x}: {}",
                hex_preview(&reply)
            ))
        })
    }

    /// Ping vendor 0x66 / op 0x01. Não envia 0x02 (reenumera o USB).
    pub fn ping(&self) -> Result<Vec<u8>> {
        self.write_bytes(&audio_ping_packet())?;
        let reply = self.read_bytes(200)?;
        if !audio_ping_ok(&reply) {
            return Err(Error::Other(format!(
                "ping inesperado: {}",
                hex_preview(&reply)
            )));
        }
        Ok(reply)
    }

    pub fn set_polar(&self, pattern: PolarPattern) -> Result<()> {
        self.write_cmd(AUDIO_SET_POLAR, pattern.wire())
    }

    pub fn polar(&self) -> Result<PolarPattern> {
        let v = self.query(AUDIO_GET_POLAR)?;
        PolarPattern::from_wire(v).ok_or_else(|| Error::Other(format!("polar desconhecido: {v}")))
    }

    pub fn set_muted(&self, muted: bool) -> Result<()> {
        self.write_cmd(AUDIO_SET_MUTE, u8::from(muted))
    }

    pub fn muted(&self) -> Result<bool> {
        Ok(self.query(AUDIO_GET_MUTE)? == 1)
    }

    pub fn set_highpass(&self, amount: u8) -> Result<()> {
        self.write_cmd(AUDIO_SET_HIGHPASS, amount)
    }

    pub fn highpass(&self) -> Result<u8> {
        self.query(AUDIO_GET_HIGHPASS)
    }

    pub fn set_gain_index(&self, index: u8) -> Result<()> {
        self.write_cmd(AUDIO_SET_GAIN, index)
    }

    pub fn gain_index(&self) -> Result<u8> {
        self.query(AUDIO_GET_GAIN)
    }

    pub fn set_sidetone_muted(&self, muted: bool) -> Result<()> {
        self.write_cmd(AUDIO_SET_SIDETONE, u8::from(muted))
    }

    pub fn sidetone_muted(&self) -> Result<bool> {
        Ok(self.query(AUDIO_GET_SIDETONE)? == 1)
    }
}

fn hex_preview(b: &[u8]) -> String {
    b.iter()
        .take(12)
        .map(|x| format!("{x:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn open_audio_hidraw() -> Result<File> {
    let path = find_audio_hidraw()?;
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                Error::Other(format!(
                    "sem permissão em {} (áudio HID). Instale udev/99-hyperx-duocast.rules, rode udevadm trigger e replugue o DuoCast",
                    path.display()
                ))
            } else {
                Error::Other(format!("{}: {e}", path.display()))
            }
        })
}

fn find_audio_hidraw() -> Result<PathBuf> {
    let root = Path::new("/sys/class/hidraw");
    let entries = fs::read_dir(root).map_err(|e| Error::Other(format!("hidraw: {e}")))?;
    for ent in entries.flatten() {
        let name = ent.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("hidraw") {
            continue;
        }
        let uevent = fs::read_to_string(ent.path().join("device/uevent")).unwrap_or_default();
        let hid_id = uevent
            .lines()
            .find_map(|l| l.strip_prefix("HID_ID="))
            .unwrap_or("");
        // 0003:000003F0:00000A8C
        let ok_id = hid_id.to_ascii_lowercase().contains("000003f0:00000a8c");
        if !ok_id {
            continue;
        }
        let iface = hidraw_interface(&ent.path()).unwrap_or(-1);
        if iface != AUDIO_HID_INTERFACE {
            continue;
        }
        return Ok(PathBuf::from(format!("/dev/{name}")));
    }
    Err(Error::AudioHidNotFound)
}

fn hidraw_interface(sys: &Path) -> Option<i32> {
    let device = sys.join("device");
    let resolved = fs::canonicalize(&device).ok()?;
    for parent in resolved.ancestors() {
        let n = parent.join("bInterfaceNumber");
        if let Ok(s) = fs::read_to_string(n) {
            return s.trim().parse().ok();
        }
    }
    None
}

fn poll_read(file: &File, buf: &mut [u8], timeout_ms: i32) -> Result<usize> {
    let mut pfd = libc::pollfd {
        fd: file.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    let n = unsafe { libc::poll(&mut pfd, 1, timeout_ms) };
    if n < 0 {
        return Err(Error::Other("poll hidraw falhou".into()));
    }
    if n == 0 {
        return Err(Error::Other("timeout na resposta HID 0x66".into()));
    }
    (&*file)
        .read(buf)
        .map_err(|e| Error::Other(format!("hidraw read: {e}")))
}
