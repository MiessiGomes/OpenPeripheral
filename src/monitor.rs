//! Loopback DuoCast → saída do PC via `arecord` / `aplay` (teste de modos).
//!
//! O jack 3.5 mm do microfone é outro caminho (mixer ALSA `PCM`).

use crate::error::{Error, Result};
use serde::Serialize;
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const RATE: u32 = 48_000;
const CHANNELS: usize = 2;
const LIVE_CAP: usize = 48_000;
const TAKE_SECS: u32 = 4;

pub struct Monitor {
    inner: Arc<Inner>,
    procs: Mutex<Option<Procs>>,
}

struct Procs {
    rec: Child,
    play: Child,
}

struct Inner {
    listening: AtomicBool,
    recording: AtomicBool,
    playing: AtomicBool,
    peak: AtomicU32,
    hold: AtomicU32,
    volume: AtomicU32,
    mic_gain: AtomicU32,
    hp: AtomicU32,
    hp_state: Mutex<HpState>,
    live: Mutex<VecDeque<i16>>,
    take: Mutex<Vec<i16>>,
    play_pos: AtomicUsize,
    error: Mutex<Option<String>>,
    input_name: Mutex<String>,
    output_name: Mutex<String>,
    stop: AtomicBool,
}

struct HpState {
    prev_x: [f32; CHANNELS],
    prev_y: [f32; CHANNELS],
}

#[derive(Clone, Serialize)]
pub struct MonitorView {
    pub listening: bool,
    pub recording: bool,
    pub playing: bool,
    pub peak: f32,
    pub hold: f32,
    pub volume: u8,
    pub has_take: bool,
    pub take_secs: u32,
    pub input: String,
    pub output: String,
    pub error: Option<String>,
}

impl Default for Monitor {
    fn default() -> Self {
        Self::new()
    }
}

impl Monitor {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                listening: AtomicBool::new(false),
                recording: AtomicBool::new(false),
                playing: AtomicBool::new(false),
                peak: AtomicU32::new(0),
                hold: AtomicU32::new(0),
                volume: AtomicU32::new(80),
                mic_gain: AtomicU32::new(70),
                hp: AtomicU32::new(0),
                hp_state: Mutex::new(HpState {
                    prev_x: [0.0; CHANNELS],
                    prev_y: [0.0; CHANNELS],
                }),
                live: Mutex::new(VecDeque::with_capacity(LIVE_CAP)),
                take: Mutex::new(Vec::new()),
                play_pos: AtomicUsize::new(0),
                error: Mutex::new(None),
                input_name: Mutex::new(String::new()),
                output_name: Mutex::new("default".into()),
                stop: AtomicBool::new(false),
            }),
            procs: Mutex::new(None),
        }
    }

    pub fn snapshot(&self) -> MonitorView {
        MonitorView {
            listening: self.inner.listening.load(Ordering::Relaxed),
            recording: self.inner.recording.load(Ordering::Relaxed),
            playing: self.inner.playing.load(Ordering::Relaxed),
            peak: f32::from_bits(self.inner.peak.load(Ordering::Relaxed)),
            hold: f32::from_bits(self.inner.hold.load(Ordering::Relaxed)),
            volume: self.inner.volume.load(Ordering::Relaxed) as u8,
            has_take: !self.inner.take.lock().unwrap().is_empty(),
            take_secs: TAKE_SECS,
            input: self.inner.input_name.lock().unwrap().clone(),
            output: self.inner.output_name.lock().unwrap().clone(),
            error: self.inner.error.lock().unwrap().clone(),
        }
    }

    pub fn decay_meters(&self) {
        let peak = f32::from_bits(self.inner.peak.load(Ordering::Relaxed)) * 0.72;
        self.inner.peak.store(peak.to_bits(), Ordering::Relaxed);
        let hold = f32::from_bits(self.inner.hold.load(Ordering::Relaxed)) * 0.97;
        self.inner
            .hold
            .store(hold.max(peak).to_bits(), Ordering::Relaxed);
    }

    pub fn set_volume(&self, pct: u8) {
        self.inner
            .volume
            .store(u32::from(pct.min(150)), Ordering::Relaxed);
    }

    pub fn set_mic_gain(&self, pct: u8) {
        self.inner
            .mic_gain
            .store(u32::from(pct.min(100)), Ordering::Relaxed);
    }

    pub fn set_highpass(&self, pct: u8) {
        self.inner
            .hp
            .store(u32::from(pct.min(100)), Ordering::Relaxed);
    }

    pub fn set_listening(&self, on: bool) -> Result<()> {
        self.inner.listening.store(on, Ordering::Relaxed);
        if on {
            self.ensure_procs()?;
        } else if !self.busy() {
            self.stop_procs();
        }
        Ok(())
    }

    pub fn record_and_play(&self) -> Result<()> {
        self.inner.listening.store(true, Ordering::Relaxed);
        self.ensure_procs()?;
        self.inner.take.lock().unwrap().clear();
        self.inner.recording.store(true, Ordering::Relaxed);
        self.inner.playing.store(false, Ordering::Relaxed);
        let inner = self.inner.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_secs(TAKE_SECS as u64));
            inner.recording.store(false, Ordering::Relaxed);
            inner.play_pos.store(0, Ordering::Relaxed);
            if !inner.take.lock().unwrap().is_empty() {
                inner.playing.store(true, Ordering::Relaxed);
            }
        });
        Ok(())
    }

    pub fn play_take(&self) -> Result<()> {
        if self.inner.take.lock().unwrap().is_empty() {
            return Err(Error::Other("grave um trecho primeiro".into()));
        }
        self.ensure_procs()?;
        self.inner.play_pos.store(0, Ordering::Relaxed);
        self.inner.playing.store(true, Ordering::Relaxed);
        Ok(())
    }

    fn busy(&self) -> bool {
        self.inner.listening.load(Ordering::Relaxed)
            || self.inner.recording.load(Ordering::Relaxed)
            || self.inner.playing.load(Ordering::Relaxed)
    }

    fn stop_procs(&self) {
        self.inner.stop.store(true, Ordering::Relaxed);
        if let Some(mut p) = self.procs.lock().unwrap().take() {
            let _ = p.rec.kill();
            let _ = p.play.kill();
            let _ = p.rec.wait();
            let _ = p.play.wait();
        }
    }

    fn ensure_procs(&self) -> Result<()> {
        if self.procs.lock().unwrap().is_some() {
            return Ok(());
        }
        *self.inner.error.lock().unwrap() = None;
        self.inner.stop.store(false, Ordering::Relaxed);
        let device = capture_device();
        *self.inner.input_name.lock().unwrap() = device.clone();

        let mut rec = spawn_arecord(&device)?;
        let mut play = spawn_aplay()?;
        let mut stdout = rec
            .stdout
            .take()
            .ok_or_else(|| Error::Other("arecord sem stdout".into()))?;
        let mut stdin = play
            .stdin
            .take()
            .ok_or_else(|| Error::Other("aplay sem stdin".into()))?;
        let inner = self.inner.clone();
        thread::Builder::new()
            .name("duocast-cap".into())
            .spawn(move || capture_loop(&mut stdout, &inner))
            .map_err(|e| Error::Other(e.to_string()))?;
        let inner_out = self.inner.clone();
        thread::Builder::new()
            .name("duocast-play".into())
            .spawn(move || playback_loop(&mut stdin, &inner_out))
            .map_err(|e| Error::Other(e.to_string()))?;
        *self.procs.lock().unwrap() = Some(Procs { rec, play });
        Ok(())
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        self.stop_procs();
    }
}

fn capture_device() -> String {
    if let Ok(s) = std::fs::read_to_string("/proc/asound/cards") {
        for line in s.lines() {
            if line.to_ascii_lowercase().contains("duocast") {
                if let Some(num) = line.split_whitespace().next() {
                    return format!("plughw:{num},0");
                }
            }
        }
    }
    let out = Command::new("arecord").args(["-l"]).output().ok();
    if let Some(o) = out {
        let s = String::from_utf8_lossy(&o.stdout);
        for line in s.lines() {
            let low = line.to_ascii_lowercase();
            if !low.contains("duocast") {
                continue;
            }
            for (prefix, _) in [("card ", ()), ("placa ", ())] {
                if let Some(rest) = low.strip_prefix(prefix) {
                    if let Some(num) = rest
                        .split([':', ' '])
                        .find(|t| t.chars().all(|c| c.is_ascii_digit()))
                    {
                        return format!("plughw:{num},0");
                    }
                }
            }
        }
    }
    "plughw:DuoCast,0".into()
}

fn spawn_arecord(dev: &str) -> Result<Child> {
    Command::new("arecord")
        .args([
            "-q", "-D", dev, "-f", "S16_LE", "-c", "2", "-r", "48000", "-t", "raw",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Error::Other(format!("arecord: {e}")))
}

fn spawn_aplay() -> Result<Child> {
    Command::new("aplay")
        .args([
            "-q", "-D", "default", "-f", "S16_LE", "-c", "2", "-r", "48000", "-t", "raw",
        ])
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Error::Other(format!("aplay: {e}")))
}

fn capture_loop(stdout: &mut impl Read, inner: &Inner) {
    let mut buf = [0u8; 4096];
    while !inner.stop.load(Ordering::Relaxed) {
        let n = match stdout.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => break,
        };
        let mut samples = s16_from_bytes(&buf[..n]);
        process_capture(&mut samples, inner);
        let mut peak = 0.0f32;
        for frame in samples.chunks(CHANNELS) {
            let m = mix_frame(frame);
            peak = peak.max((m as f32).abs() / 32768.0);
        }
        let prev = f32::from_bits(inner.peak.load(Ordering::Relaxed));
        if peak > prev {
            inner.peak.store(peak.to_bits(), Ordering::Relaxed);
        }
        let hold = f32::from_bits(inner.hold.load(Ordering::Relaxed));
        if peak > hold {
            inner.hold.store(peak.to_bits(), Ordering::Relaxed);
        }

        if inner.recording.load(Ordering::Relaxed) {
            let mut take = inner.take.lock().unwrap();
            take.extend_from_slice(&samples);
            let max = RATE as usize * CHANNELS * TAKE_SECS as usize;
            if take.len() > max {
                take.truncate(max);
                inner.recording.store(false, Ordering::Relaxed);
                inner.play_pos.store(0, Ordering::Relaxed);
                inner.playing.store(true, Ordering::Relaxed);
            }
        }

        if inner.listening.load(Ordering::Relaxed) && !inner.playing.load(Ordering::Relaxed) {
            let mut live = inner.live.lock().unwrap();
            for s in samples {
                if live.len() >= LIVE_CAP * CHANNELS {
                    live.pop_front();
                }
                live.push_back(s);
            }
        }
    }
}

fn playback_loop(stdin: &mut impl Write, inner: &Inner) {
    let silence = vec![0u8; 1024];
    while !inner.stop.load(Ordering::Relaxed) {
        let vol = inner.volume.load(Ordering::Relaxed) as f32 / 100.0;
        let mut pcm: Vec<i16> = Vec::new();
        if inner.playing.load(Ordering::Relaxed) {
            let take = inner.take.lock().unwrap();
            let mut pos = inner.play_pos.load(Ordering::Relaxed);
            let n = 1024 / 2;
            if pos >= take.len() {
                inner.playing.store(false, Ordering::Relaxed);
            } else {
                let end = (pos + n).min(take.len());
                pcm.extend_from_slice(&take[pos..end]);
                pos = end;
                inner.play_pos.store(pos, Ordering::Relaxed);
                if pos >= take.len() {
                    inner.playing.store(false, Ordering::Relaxed);
                }
            }
        } else if inner.listening.load(Ordering::Relaxed) {
            let mut live = inner.live.lock().unwrap();
            let n = 512.min(live.len());
            for _ in 0..n {
                if let Some(s) = live.pop_front() {
                    pcm.push(s);
                }
            }
        }
        let bytes = if pcm.is_empty() {
            silence.clone()
        } else {
            let scaled: Vec<i16> = pcm
                .into_iter()
                .map(|s| ((s as f32) * vol).clamp(-32768.0, 32767.0) as i16)
                .collect();
            s16_to_bytes(&scaled)
        };
        if stdin.write_all(&bytes).is_err() {
            *inner.error.lock().unwrap() = Some("saída de áudio encerrada".into());
            break;
        }
    }
}

fn process_capture(samples: &mut [i16], inner: &Inner) {
    let g = inner.mic_gain.load(Ordering::Relaxed) as f32 / 100.0;
    let gain = 0.05 + g * g * 5.95;
    let hp = inner.hp.load(Ordering::Relaxed) as f32 / 100.0;
    let cutoff = 40.0 + hp * 900.0;
    let dt = 1.0 / RATE as f32;
    let rc = 1.0 / (2.0 * std::f32::consts::PI * cutoff.max(20.0));
    let a = rc / (rc + dt);
    let mut st = inner.hp_state.lock().unwrap();
    for frame in samples.chunks_mut(CHANNELS) {
        for (ch, s) in frame.iter_mut().enumerate() {
            let mut x = *s as f32 / 32768.0;
            if hp > 0.02 && ch < CHANNELS {
                let y = a * (st.prev_y[ch] + x - st.prev_x[ch]);
                st.prev_x[ch] = x;
                st.prev_y[ch] = y;
                x = y;
            }
            *s = (x * gain * 32767.0).clamp(-32767.0, 32767.0) as i16;
        }
    }
}

fn mix_frame(frame: &[i16]) -> i16 {
    if frame.is_empty() {
        0
    } else {
        (frame.iter().map(|s| *s as i32).sum::<i32>() / frame.len() as i32) as i16
    }
}

fn s16_from_bytes(b: &[u8]) -> Vec<i16> {
    b.as_chunks::<2>()
        .0
        .iter()
        .map(|c| i16::from_le_bytes(*c))
        .collect()
}

fn s16_to_bytes(s: &[i16]) -> Vec<u8> {
    let mut o = Vec::with_capacity(s.len() * 2);
    for v in s {
        o.extend_from_slice(&v.to_le_bytes());
    }
    o
}

#[cfg(test)]
mod tests {
    use super::{mix_frame, s16_from_bytes};

    #[test]
    fn roundtrip_s16() {
        let v = s16_from_bytes(&[0x00, 0x10, 0xff, 0x7f]);
        assert_eq!(v, vec![0x1000, 0x7fff]);
        assert_eq!(mix_frame(&[100, -100]), 0);
    }
}
