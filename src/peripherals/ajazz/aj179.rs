//! AJAZZ AJ179 — HID vendor na interface 2 (32 bytes, sem report ID no payload).
//!
//! Origem: AJAZZ Driver (X) 1.0.6.0 (`config.xml`, strings HID, recurso BIN Telink)
//! e confirmação ao vivo no recetor `249a:5c2f` (cmd `0x10`…`0x20`). Não envia OTA.

use crate::error::{Error, Result};
use crate::hid::FoundDevice;
use crate::rgb::Rgb;
use hidapi::{HidApi, HidDevice};
use serde::Serialize;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const TELINK_VID: u16 = 0x248A;
pub const DONGLE_VID: u16 = 0x249A;
pub const WIRED_PIDS: [u16; 4] = [0x5C2E, 0x5D2E, 0x5E2E, 0x5B2E];
pub const DONGLE_PIDS: [u16; 2] = [0x5C2F, 0x5B2F];
pub const CONFIG_INTERFACE: i32 = 2;

pub const REPORT_LEN: usize = 32;
const WRITE_LEN: usize = 33;
const PAYLOAD_MAX: usize = 27;

pub const CMD_SET_RATE: u8 = 0x02;
pub const CMD_SET_DPI: u8 = 0x03;
pub const CMD_SET_DPI_COLOR: u8 = 0x04;
pub const CMD_SET_LIGHT: u8 = 0x05;
pub const CMD_SET_SENSOR: u8 = 0x06;
pub const CMD_SET_POWER: u8 = 0x07;
pub const CMD_SET_BUTTONS: u8 = 0x09;
pub const CMD_INFO: u8 = 0x10;
pub const CMD_GET_KEYS: u8 = 0x11;
pub const CMD_GET_RATE: u8 = 0x12;
pub const CMD_GET_DPI: u8 = 0x13;
pub const CMD_GET_DPI_COLOR: u8 = 0x14;
pub const CMD_GET_LIGHT: u8 = 0x15;
pub const CMD_GET_SENSOR: u8 = 0x16;
pub const CMD_GET_POWER: u8 = 0x17;
pub const CMD_RX_FW: u8 = 0x20;
pub const CMD_STATUS: u8 = 0xC0;

pub const ST_OK: u8 = 0x00;
pub const ST_IDLE: u8 = 0x01;
pub const ST_ASLEEP: u8 = 0x02;

pub const LIGHT_OFF: u8 = 0;
pub const LIGHT_BREATHING: u8 = 2;
pub const LIGHT_CONSTANT: u8 = 3;
pub const SENSOR_PAW3395: u8 = 0x95;

#[derive(Clone, Debug, Serialize)]
pub struct Info {
    pub device_id: String,
    pub sensor_id: u8,
    pub charging: bool,
    pub battery: u8,
    pub connected: bool,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct DpiStage {
    pub value: u16,
    pub color: Rgb,
}

#[derive(Clone, Debug, Serialize)]
pub struct LightingState {
    pub mode: u8,
    pub brightness: u8,
    pub speed: u8,
    pub color: Rgb,
}

#[derive(Clone, Debug, Serialize)]
pub struct SensorState {
    pub lod: u8,
    pub motion_sync: bool,
    pub angle_snap: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct PowerState {
    pub sleep: u8,
    pub wake_on_move: bool,
    pub lights_off_on_move: bool,
    pub debounce: u8,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Snapshot {
    pub connected: bool,
    pub link: String,
    pub vid: u16,
    pub pid: u16,
    pub path: String,
    pub info: Option<Info>,
    pub rate_hz: Option<u16>,
    pub dpi_index: Option<u8>,
    pub dpi: Vec<DpiStage>,
    pub lighting: Option<LightingState>,
    pub sensor: Option<SensorState>,
    pub power: Option<PowerState>,
    pub buttons: Vec<[u8; 3]>,
    pub rx_fw: Option<String>,
    pub error: Option<String>,
}

pub fn is_aj179_ids(vid: u16, pid: u16) -> bool {
    match vid {
        TELINK_VID => WIRED_PIDS.contains(&pid) || DONGLE_PIDS.contains(&pid),
        DONGLE_VID => DONGLE_PIDS.contains(&pid),
        _ => false,
    }
}

pub fn enumerate(api: &HidApi) -> Vec<FoundDevice> {
    api.device_list()
        .filter(|d| is_aj179_ids(d.vendor_id(), d.product_id()))
        .map(crate::hid::found_from_info)
        .collect()
}

pub fn checksum(report: &[u8; REPORT_LEN]) -> u8 {
    report[4..=30].iter().fold(0u8, |a, b| a.wrapping_add(*b))
}

pub fn build_report(cmd: u8, chunk: u8, payload: &[u8]) -> [u8; REPORT_LEN] {
    let mut r = [0u8; REPORT_LEN];
    r[0] = cmd;
    r[1] = 0;
    r[2] = chunk;
    let n = payload.len().min(PAYLOAD_MAX);
    r[3] = n as u8;
    r[4..4 + n].copy_from_slice(&payload[..n]);
    r[31] = checksum(&r);
    r
}

pub fn read_cmd(cmd: u8) -> [u8; REPORT_LEN] {
    build_report(cmd, 0x00, &[])
}

fn wire_packet(report: &[u8; REPORT_LEN]) -> [u8; WRITE_LEN] {
    let mut w = [0u8; WRITE_LEN];
    w[1..].copy_from_slice(report);
    w
}

pub fn parse_info(report: &[u8; REPORT_LEN]) -> Option<Info> {
    if report[0] != CMD_INFO || report[1] != ST_OK {
        return None;
    }
    let id = String::from_utf8_lossy(&report[4..8])
        .trim_end_matches('\0')
        .to_string();
    Some(Info {
        device_id: id,
        sensor_id: report[10],
        charging: report[12] != 0,
        battery: report[13],
        connected: report[14] != 0,
    })
}

pub fn clamp_dpi(sensor: u8, dpi: u16) -> u16 {
    let (lo, hi) = match sensor {
        SENSOR_PAW3395 => (50, 26_000),
        _ => (50, 26_000),
    };
    let stepped = dpi.max(lo) / 50 * 50;
    stepped.clamp(lo, hi)
}

pub fn encode_dpi(sensor: u8, dpi: u16) -> u16 {
    let dpi = clamp_dpi(sensor, dpi);
    match sensor {
        SENSOR_PAW3395 | 0x96 => dpi / 50 - 1,
        _ => dpi / 50 - 1,
    }
}

pub fn decode_dpi(sensor: u8, raw: u16) -> u16 {
    match sensor {
        SENSOR_PAW3395 | 0x96 => (raw + 1) * 50,
        _ => (raw + 1) * 50,
    }
}

pub fn rate_to_flag(hz: u16) -> Option<u8> {
    Some(match hz {
        125 => 0x08,
        250 => 0x04,
        500 => 0x02,
        1000 => 0x01,
        2000 => 0x10,
        4000 => 0x20,
        8000 => 0x40,
        _ => return None,
    })
}

pub fn flag_to_rate(flag: u8) -> Option<u16> {
    Some(match flag {
        0x08 => 125,
        0x04 => 250,
        0x02 => 500,
        0x01 => 1000,
        0x10 => 2000,
        0x20 => 4000,
        0x40 => 8000,
        _ => return None,
    })
}

pub fn parse_dpi(report: &[u8; REPORT_LEN], sensor: u8) -> Option<(u8, Vec<u16>)> {
    if report[0] != CMD_GET_DPI || report[1] != ST_OK {
        return None;
    }
    let index = report[4] >> 4;
    let count = report[4] & 0x0f;
    let mut stages = Vec::new();
    for i in 0..count {
        let o = 5 + (i as usize) * 4;
        if o + 1 >= REPORT_LEN {
            break;
        }
        let raw = u16::from_le_bytes([report[o], report[o + 1]]);
        stages.push(decode_dpi(sensor, raw));
    }
    Some((index, stages))
}

pub fn parse_dpi_colors(report: &[u8; REPORT_LEN], n: usize) -> Vec<Rgb> {
    (0..n)
        .map(|i| {
            let o = 4 + i * 3;
            Rgb::new(report[o], report[o + 1], report[o + 2])
        })
        .collect()
}

pub fn parse_lighting(report: &[u8; REPORT_LEN]) -> Option<LightingState> {
    if report[0] != CMD_GET_LIGHT || report[1] != ST_OK {
        return None;
    }
    let len = report[3] as usize;
    if len == 0 {
        return None;
    }
    let mode = report[4];
    let packed = if len >= 2 { report[5] } else { 0 };
    let color = if mode == LIGHT_BREATHING && len >= 9 {
        Rgb::new(report[7], report[8], report[9])
    } else if len >= 5 {
        Rgb::new(report[6], report[7], report[8])
    } else {
        Rgb::OFF
    };
    Some(LightingState {
        mode,
        brightness: packed >> 4,
        speed: packed & 0x0f,
        color,
    })
}

pub fn parse_sensor(report: &[u8; REPORT_LEN]) -> Option<SensorState> {
    if report[0] != CMD_GET_SENSOR || report[1] != ST_OK {
        return None;
    }
    Some(SensorState {
        lod: report[4],
        motion_sync: report[5] != 0,
        angle_snap: report[6] != 0,
    })
}

pub fn parse_power(report: &[u8; REPORT_LEN]) -> Option<PowerState> {
    if report[0] != CMD_GET_POWER || report[1] != ST_OK {
        return None;
    }
    Some(PowerState {
        sleep: report[4],
        wake_on_move: report[5] != 0,
        lights_off_on_move: report[6] != 0,
        debounce: report[7],
    })
}

pub const BUTTON_COUNT: usize = 6;
pub const BUTTON_NAMES: [&str; BUTTON_COUNT] = [
    "esquerdo",
    "direito",
    "meio",
    "traseiro",
    "dianteiro",
    "dpi",
];

/// Fábrica do Driver (X) / `0x11` ao vivo: L R M back fwd DPI-loop.
pub const FACTORY_BUTTONS: [[u8; 3]; BUTTON_COUNT] = [
    [0x10, 0x01, 0x00],
    [0x10, 0x02, 0x00],
    [0x10, 0x04, 0x00],
    [0x10, 0x08, 0x00],
    [0x10, 0x10, 0x00],
    [0x40, 0x01, 0x00],
];

pub fn parse_buttons(report: &[u8; REPORT_LEN]) -> Vec<[u8; 3]> {
    if report[0] != CMD_GET_KEYS || report[1] != ST_OK {
        return Vec::new();
    }
    (0..BUTTON_COUNT)
        .map(|i| {
            let o = 4 + i * 3;
            [report[o], report[o + 1], report[o + 2]]
        })
        .collect()
}

pub fn button_action(code: [u8; 3]) -> String {
    match code {
        [0x10, 0x01, 0x00] => "left".into(),
        [0x10, 0x02, 0x00] => "right".into(),
        [0x10, 0x04, 0x00] => "middle".into(),
        [0x10, 0x08, 0x00] => "back".into(),
        [0x10, 0x10, 0x00] => "forward".into(),
        [0x20, 0x01, 0x00] => "scroll-up".into(),
        [0x20, 0xff, 0x00] => "scroll-down".into(),
        [0x40, 0x01, 0x00] => "dpi-loop".into(),
        [0x40, 0x02, 0x00] => "dpi-up".into(),
        [0x40, 0x03, 0x00] => "dpi-down".into(),
        [0x80, 0xcd, 0x00] => "play-pause".into(),
        [0x80, 0xb5, 0x00] => "next".into(),
        [0x80, 0xb6, 0x00] => "previous".into(),
        [0x80, 0xb7, 0x00] => "stop".into(),
        [0x80, 0xe9, 0x00] => "volume-up".into(),
        [0x80, 0xea, 0x00] => "volume-down".into(),
        [0x80, 0xe2, 0x00] => "mute".into(),
        [0x60, 0x00, 0x00] => "disabled".into(),
        _ => format!("{:02x}{:02x}{:02x}", code[0], code[1], code[2]),
    }
}

pub fn encode_button(action: &str) -> Result<[u8; 3]> {
    Ok(match action.trim().to_ascii_lowercase().as_str() {
        "left" | "esquerdo" => [0x10, 0x01, 0x00],
        "right" | "direito" => [0x10, 0x02, 0x00],
        "middle" | "meio" => [0x10, 0x04, 0x00],
        "back" | "traseiro" => [0x10, 0x08, 0x00],
        "forward" | "dianteiro" => [0x10, 0x10, 0x00],
        "scroll-up" => [0x20, 0x01, 0x00],
        "scroll-down" => [0x20, 0xff, 0x00],
        "dpi-loop" => [0x40, 0x01, 0x00],
        "dpi-up" => [0x40, 0x02, 0x00],
        "dpi-down" => [0x40, 0x03, 0x00],
        "play-pause" => [0x80, 0xcd, 0x00],
        "next" => [0x80, 0xb5, 0x00],
        "previous" => [0x80, 0xb6, 0x00],
        "stop" => [0x80, 0xb7, 0x00],
        "volume-up" => [0x80, 0xe9, 0x00],
        "volume-down" => [0x80, 0xea, 0x00],
        "mute" => [0x80, 0xe2, 0x00],
        "disabled" => [0x60, 0x00, 0x00],
        other => return Err(Error::Other(format!("ação de botão desconhecida: {other}"))),
    })
}

/// `0x09`: o driver declara `len=0x0F` mesmo com 18 bytes de mapeamento.
pub fn buttons_packet(keys: &[[u8; 3]]) -> [u8; REPORT_LEN] {
    let mut payload = [0u8; 18];
    for (i, k) in keys.iter().take(BUTTON_COUNT).enumerate() {
        payload[i * 3..i * 3 + 3].copy_from_slice(k);
    }
    let mut r = build_report(CMD_SET_BUTTONS, 0x01, &payload);
    r[3] = 0x0F;
    r[31] = checksum(&r);
    r
}

pub fn lighting_payload(mode: u8, brightness: u8, speed: u8, color: Rgb) -> Vec<u8> {
    let bri = if mode == LIGHT_OFF {
        brightness.min(4)
    } else {
        brightness.clamp(1, 4)
    };
    let packed = (bri << 4) | speed.min(4);
    match mode {
        LIGHT_CONSTANT => vec![mode, packed, color.r, color.g, color.b],
        LIGHT_BREATHING => {
            let mut p = vec![mode, packed, 0x07];
            for _ in 0..7 {
                p.extend_from_slice(&[color.r, color.g, color.b]);
            }
            p
        }
        _ => vec![LIGHT_OFF, packed],
    }
}

pub fn dpi_payload(sensor: u8, index: u8, stages: &[u16]) -> Vec<u8> {
    let count = stages.len().min(8) as u8;
    let mut p = vec![(index << 4) | count];
    for &dpi in stages.iter().take(8) {
        let raw = encode_dpi(sensor, dpi).to_le_bytes();
        p.extend_from_slice(&raw);
        p.extend_from_slice(&raw);
    }
    p
}

enum Transport {
    Hidraw(File),
    Hidapi(HidDevice),
}

pub struct Mouse {
    dev: Transport,
    pub vid: u16,
    pub pid: u16,
    pub path: String,
}

impl Mouse {
    pub fn open() -> Result<Self> {
        match open_hidraw() {
            Ok((file, vid, pid, path)) => Ok(Self {
                dev: Transport::Hidraw(file),
                vid,
                pid,
                path,
            }),
            Err(hidraw_err) => {
                let api = HidApi::new()?;
                match open_hidapi(&api) {
                    Ok((dev, vid, pid, path)) => Ok(Self {
                        dev: Transport::Hidapi(dev),
                        vid,
                        pid,
                        path,
                    }),
                    Err(e) => Err(Error::Other(format!("{hidraw_err}; {e}"))),
                }
            }
        }
    }

    fn write_report(&self, report: &[u8; REPORT_LEN]) -> Result<()> {
        let pkt = wire_packet(report);
        match &self.dev {
            Transport::Hidraw(f) => {
                (&*f)
                    .write_all(&pkt)
                    .map_err(|e| Error::Other(format!("aj179 hidraw write: {e}")))?;
            }
            Transport::Hidapi(d) => {
                d.write(&pkt)?;
            }
        }
        Ok(())
    }

    fn read_some(&self, timeout_ms: i32) -> Result<Option<[u8; REPORT_LEN]>> {
        let mut buf = [0u8; 64];
        let n = match &self.dev {
            Transport::Hidraw(f) => poll_read(f, &mut buf, timeout_ms)?,
            Transport::Hidapi(d) => {
                d.set_blocking_mode(false)?;
                d.read_timeout(&mut buf, timeout_ms).unwrap_or_default()
            }
        };
        if n == 0 {
            return Ok(None);
        }
        let mut r = [0u8; REPORT_LEN];
        let start = if n == WRITE_LEN { 1 } else { 0 };
        let take = REPORT_LEN.min(n.saturating_sub(start));
        r[..take].copy_from_slice(&buf[start..start + take]);
        Ok(Some(r))
    }

    pub fn transact(&self, report: &[u8; REPORT_LEN]) -> Result<[u8; REPORT_LEN]> {
        let cmd = report[0];
        self.write_report(report)?;
        let deadline = Instant::now() + Duration::from_millis(3200);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(Error::Other(
                    "AJ179: timeout HID (mova o mouse se estiver a dormir)".into(),
                ));
            }
            let wait = left.as_millis().min(200) as i32;
            match self.read_some(wait)? {
                Some(r) if r[0] == CMD_STATUS => continue,
                Some(r) if r[0] == cmd && r[1] == ST_OK => return Ok(r),
                Some(r) if r[0] == cmd && r[1] == ST_ASLEEP => {
                    return Err(Error::Other(
                        "AJ179 a dormir — mexa no mouse e tente de novo".into(),
                    ));
                }
                Some(r) if r[0] == cmd && r[1] == ST_IDLE => {
                    return Err(Error::Other(
                        "AJ179 não respondeu (idle). Mexa no mouse.".into(),
                    ));
                }
                Some(_) => continue,
                None => {}
            }
        }
    }

    pub fn get(&self, cmd: u8) -> Result<[u8; REPORT_LEN]> {
        self.transact(&read_cmd(cmd))
    }

    pub fn set(&self, cmd: u8, payload: &[u8]) -> Result<[u8; REPORT_LEN]> {
        self.transact(&build_report(cmd, 0x01, payload))
    }

    pub fn snapshot(&self) -> Snapshot {
        let link = if DONGLE_PIDS.contains(&self.pid) {
            "2.4G"
        } else {
            "USB"
        };
        let mut snap = Snapshot {
            connected: true,
            link: link.into(),
            vid: self.vid,
            pid: self.pid,
            path: self.path.clone(),
            ..Snapshot::default()
        };
        match self.get(CMD_INFO) {
            Ok(r) => snap.info = parse_info(&r),
            Err(e) => {
                snap.error = Some(e.to_string());
                return snap;
            }
        }
        let sensor = snap
            .info
            .as_ref()
            .map(|i| i.sensor_id)
            .unwrap_or(SENSOR_PAW3395);
        let awake = snap.info.as_ref().map(|i| i.connected).unwrap_or(false);
        if !awake {
            snap.error = Some("mouse a dormir — mexa para sincronizar DPI/LED".into());
            return snap;
        }
        if let Ok(r) = self.get(CMD_GET_RATE) {
            snap.rate_hz = flag_to_rate(r[4]);
        }
        let mut values = Vec::new();
        if let Ok(r) = self.get(CMD_GET_DPI) {
            if let Some((idx, v)) = parse_dpi(&r, sensor) {
                snap.dpi_index = Some(idx);
                values = v;
            }
        }
        let colors = self
            .get(CMD_GET_DPI_COLOR)
            .map(|r| parse_dpi_colors(&r, values.len().max(6)))
            .unwrap_or_default();
        snap.dpi = values
            .into_iter()
            .enumerate()
            .map(|(i, value)| DpiStage {
                value,
                color: colors.get(i).copied().unwrap_or(Rgb::OFF),
            })
            .collect();
        if let Ok(r) = self.get(CMD_GET_LIGHT) {
            snap.lighting = parse_lighting(&r);
        }
        if let Ok(r) = self.get(CMD_GET_SENSOR) {
            snap.sensor = parse_sensor(&r);
        }
        if let Ok(r) = self.get(CMD_GET_POWER) {
            snap.power = parse_power(&r);
        }
        if let Ok(r) = self.get(CMD_GET_KEYS) {
            snap.buttons = parse_buttons(&r);
        }
        if DONGLE_PIDS.contains(&self.pid) {
            if let Ok(r) = self.get(CMD_RX_FW) {
                snap.rx_fw = Some(format!("{:x}.{:02x}", r[4], r[5]));
            }
        }
        snap
    }

    pub fn set_rate(&self, hz: u16) -> Result<()> {
        let flag = rate_to_flag(hz).ok_or_else(|| Error::Other(format!("taxa inválida {hz}")))?;
        self.set(CMD_SET_RATE, &[flag])?;
        Ok(())
    }

    pub fn set_dpi_stages(&self, sensor: u8, index: u8, stages: &[u16]) -> Result<()> {
        self.set(CMD_SET_DPI, &dpi_payload(sensor, index, stages))?;
        Ok(())
    }

    pub fn set_buttons(&self, keys: &[[u8; 3]]) -> Result<()> {
        self.transact(&buttons_packet(keys))?;
        Ok(())
    }

    pub fn set_dpi_colors(&self, colors: &[Rgb]) -> Result<()> {
        let mut p = Vec::new();
        for c in colors.iter().take(6) {
            p.extend_from_slice(&[c.r, c.g, c.b]);
        }
        while p.len() < 18 {
            p.extend_from_slice(&[0, 0, 0]);
        }
        self.set(CMD_SET_DPI_COLOR, &p)?;
        Ok(())
    }

    pub fn set_lighting(&self, mode: u8, brightness: u8, speed: u8, color: Rgb) -> Result<()> {
        let payload = lighting_payload(mode, brightness, speed, color);
        self.set(CMD_SET_LIGHT, &payload)?;
        self.set(CMD_SET_LIGHT, &payload)?;
        Ok(())
    }

    pub fn set_sensor(&self, lod: u8, motion_sync: bool, angle_snap: bool) -> Result<()> {
        self.set(
            CMD_SET_SENSOR,
            &[lod, u8::from(motion_sync), u8::from(angle_snap), 0, 0],
        )?;
        Ok(())
    }

    pub fn set_power(&self, p: &PowerState) -> Result<()> {
        self.set(
            CMD_SET_POWER,
            &[
                p.sleep,
                u8::from(p.wake_on_move),
                u8::from(p.lights_off_on_move),
                p.debounce,
            ],
        )?;
        Ok(())
    }
}

fn open_hidapi(api: &HidApi) -> Result<(HidDevice, u16, u16, String)> {
    let info = api
        .device_list()
        .find(|d| {
            is_aj179_ids(d.vendor_id(), d.product_id()) && d.interface_number() == CONFIG_INTERFACE
        })
        .ok_or(Error::Aj179NotFound)?;
    let vid = info.vendor_id();
    let pid = info.product_id();
    let path = info.path().to_string_lossy().into_owned();
    Ok((api.open_path(info.path())?, vid, pid, path))
}

fn open_hidraw() -> Result<(File, u16, u16, String)> {
    let (path, vid, pid) = find_hidraw()?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                Error::Other(format!(
                    "sem permissão em {} (AJ179). Instale src/peripherals/ajazz/udev/99-ajazz-aj179.rules e replugue o dongle/cabo",
                    path.display()
                ))
            } else {
                Error::Other(format!("{}: {e}", path.display()))
            }
        })?;
    Ok((file, vid, pid, path.display().to_string()))
}

fn find_hidraw() -> Result<(PathBuf, u16, u16)> {
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
        let Some((vid, pid)) = parse_hid_id(hid_id) else {
            continue;
        };
        if !is_aj179_ids(vid, pid) {
            continue;
        }
        let iface = hidraw_interface(&ent.path()).unwrap_or(-1);
        if iface != CONFIG_INTERFACE {
            continue;
        }
        return Ok((PathBuf::from(format!("/dev/{name}")), vid, pid));
    }
    Err(Error::Aj179NotFound)
}

fn parse_hid_id(s: &str) -> Option<(u16, u16)> {
    // 0003:0000249A:00005C2F
    let mut parts = s.split(':');
    let _bus = parts.next()?;
    let vid = u16::from_str_radix(parts.next()?, 16).ok()?;
    let pid = u16::from_str_radix(parts.next()?, 16).ok()?;
    Some((vid, pid))
}

fn hidraw_interface(sys: &Path) -> Option<i32> {
    let resolved = fs::canonicalize(sys.join("device")).ok()?;
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
        return Err(Error::Other("poll aj179 falhou".into()));
    }
    if n == 0 {
        return Ok(0);
    }
    (&*file)
        .read(buf)
        .map_err(|e| Error::Other(format!("hidraw read: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex32(s: &str) -> [u8; REPORT_LEN] {
        let clean: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect();
        let mut out = [0u8; REPORT_LEN];
        for i in 0..REPORT_LEN {
            out[i] = u8::from_str_radix(&clean[i * 2..i * 2 + 2], 16).unwrap();
        }
        out
    }

    #[test]
    fn checksum_matches_live_info() {
        let r = hex32("1000010b4d31373900009501002f0100000000000000000000000000000000b4");
        assert_eq!(checksum(&r), r[31]);
        let info = parse_info(&r).unwrap();
        assert_eq!(info.device_id, "M179");
        assert_eq!(info.sensor_id, SENSOR_PAW3395);
        assert_eq!(info.battery, 0x2f);
        assert!(info.connected);
        assert!(!info.charging);
    }

    #[test]
    fn dpi_decode_paw3395_live() {
        let r = hex32("1300011936070007000f000f00170017001f001f002f002f003f003f000000aa");
        let (idx, stages) = parse_dpi(&r, SENSOR_PAW3395).unwrap();
        assert_eq!(idx, 3);
        assert_eq!(stages, vec![400, 800, 1200, 1600, 2400, 3200]);
        assert_eq!(encode_dpi(SENSOR_PAW3395, 800), 15);
    }

    #[test]
    fn rate_and_power_live() {
        let rate = hex32("1200010101000000000000000000000000000000000000000000000000000001");
        assert_eq!(flag_to_rate(rate[4]), Some(1000));
        let pwr = hex32("170001051e000008000000000000000000000000000000000000000000000026");
        let p = parse_power(&pwr).unwrap();
        assert_eq!(p.sleep, 30);
        assert_eq!(p.debounce, 8);
        let fw = hex32("2000010241010000000000000000000000000000000000000000000000000042");
        assert_eq!(fw[4], 0x41);
        assert_eq!(fw[5], 0x01);
    }

    #[test]
    fn buttons_are_get_keys() {
        let r = hex32("11000112100100100200100400100800101000400100000000000000000000b0");
        let keys = parse_buttons(&r);
        assert_eq!(keys[0], [0x10, 0x01, 0x00]);
        assert_eq!(keys[5], [0x40, 0x01, 0x00]);
        assert_eq!(button_action(keys[0]), "left");
        assert_eq!(button_action(keys[5]), "dpi-loop");
        let pkt = buttons_packet(&FACTORY_BUTTONS);
        assert_eq!(pkt[0], CMD_SET_BUTTONS);
        assert_eq!(pkt[2], 0x01);
        assert_eq!(pkt[3], 0x0F);
        assert_eq!(&pkt[4..7], &[0x10, 0x01, 0x00]);
        assert_eq!(&pkt[19..22], &[0x40, 0x01, 0x00]);
        assert_eq!(pkt[31], checksum(&pkt));
        assert_eq!(encode_button("scroll-up").unwrap(), [0x20, 0x01, 0x00]);
        assert_eq!(clamp_dpi(SENSOR_PAW3395, 812), 800);
        assert_eq!(clamp_dpi(SENSOR_PAW3395, 10), 50);
    }

    #[test]
    fn read_cmd_checksum_zero_payload() {
        let r = read_cmd(CMD_INFO);
        assert_eq!(r[0], 0x10);
        assert_eq!(r[2], 0);
        assert_eq!(r[3], 0);
        assert_eq!(r[31], 0);
        assert_eq!(wire_packet(&r)[0], 0);
        assert_eq!(wire_packet(&r)[1], 0x10);
    }

    #[test]
    fn lighting_off_payload() {
        let p = lighting_payload(LIGHT_OFF, 0, 0, Rgb::OFF);
        assert_eq!(p, vec![0, 0]);
        let c = lighting_payload(LIGHT_CONSTANT, 4, 0, Rgb::new(0, 0xff, 0xff));
        assert_eq!(c, vec![3, 0x40, 0, 0xff, 0xff]);
        // brilho 0 em modo ligado vira 1 — senão o firmware fica “constante apagado”
        let dim = lighting_payload(LIGHT_CONSTANT, 0, 0, Rgb::new(0xff, 0, 0));
        assert_eq!(dim, vec![3, 0x10, 0xff, 0, 0]);
        let breath = lighting_payload(LIGHT_BREATHING, 2, 3, Rgb::new(0, 0xff, 0));
        assert_eq!(breath[0], 2);
        assert_eq!(breath[1], 0x23);
        assert_eq!(breath[2], 0x07);
        assert_eq!(&breath[3..6], &[0, 0xff, 0]);
        assert_eq!(breath.len(), 3 + 7 * 3);
        let mut get = [0u8; REPORT_LEN];
        get[0] = CMD_GET_LIGHT;
        get[3] = 5;
        get[4] = LIGHT_CONSTANT;
        get[5] = 0x40;
        get[6] = 0xff;
        get[7] = 0x00;
        get[8] = 0xfe;
        let parsed = parse_lighting(&get).unwrap();
        assert_eq!(parsed.mode, LIGHT_CONSTANT);
        assert_eq!(parsed.brightness, 4);
        assert_eq!(parsed.color, Rgb::new(0xff, 0, 0xfe));
    }
}
