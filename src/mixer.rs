//! Mixers ALSA do cartão `DuoCast` (`03F0:0A8C`).
//!
//! Observado em `amixer -c DuoCast`:
//! - `Mic` — mute de captura (UAC Feature Unit)
//! - `PCM` — volume/mute de playback (monitor / sidetone no host)
//! - `Extension Unit` — switch do Extension Unit Realtek `wExtensionCode=0x0BDA`
//!   (no Windows o NGENUITY usa APO: AINR, limiter, presence; no Linux só este
//!   bit USB aparece)

use crate::error::{Error, Result};
use std::process::Command;

const CARD: &str = "DuoCast";

fn amixer(args: &[&str]) -> Result<String> {
    let out = Command::new("amixer")
        .args(["-c", CARD])
        .args(args)
        .output()
        .map_err(|e| Error::Mixer(e.to_string()))?;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    if !out.status.success() {
        return Err(Error::Mixer(stderr.if_empty(stdout)));
    }
    Ok(stdout)
}

trait IfEmpty {
    fn if_empty(self, other: String) -> String;
}

impl IfEmpty for String {
    fn if_empty(self, other: String) -> String {
        if self.trim().is_empty() {
            other
        } else {
            self
        }
    }
}

pub fn contents() -> Result<String> {
    amixer(&["scontents"])
}

pub fn set_mute(muted: bool) -> Result<String> {
    amixer(&["sset", "Mic", if muted { "nocap" } else { "cap" }])
}

pub fn set_monitor(percent: u8, enabled: bool) -> Result<String> {
    let pct = format!("{}%", percent.min(100));
    let state = if enabled { "unmute" } else { "mute" };
    amixer(&["sset", "PCM", &pct, state])
}

pub fn set_extension(on: bool) -> Result<String> {
    amixer(&["sset", "Extension Unit", if on { "unmute" } else { "mute" }])
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct MixerState {
    pub mic_capture: bool,
    pub pcm_percent: u8,
    pub pcm_on: bool,
    pub extension: bool,
}

pub fn snapshot() -> Result<MixerState> {
    Ok(parse_snapshot(&contents()?))
}

fn parse_snapshot(s: &str) -> MixerState {
    let mut state = MixerState::default();
    let mut section = "";
    for line in s.lines() {
        if let Some(rest) = line.strip_prefix("Simple mixer control '") {
            section = rest.split('\'').next().unwrap_or("");
        }
        match section {
            "Mic" if line.contains("Capture [on]") => state.mic_capture = true,
            "Mic" if line.contains("Capture [off]") => state.mic_capture = false,
            "PCM" => {
                if let Some(i) = line.find('[') {
                    if let Some(j) = line[i..].find('%') {
                        if let Ok(p) = line[i + 1..i + j].trim().parse::<u8>() {
                            state.pcm_percent = p.min(100);
                        }
                    }
                }
                if line.contains("Playback") && line.contains("[on]") {
                    state.pcm_on = true;
                }
                if line.contains("Playback") && line.contains("[off]") {
                    state.pcm_on = false;
                }
            }
            "Extension Unit" => {
                if line.contains("Playback [on]") {
                    state.extension = true;
                }
                if line.contains("Playback [off]") {
                    state.extension = false;
                }
            }
            _ => {}
        }
    }
    state
}

#[cfg(test)]
mod tests {
    use super::parse_snapshot;

    #[test]
    fn parses_amixer_dump() {
        let dump = "\
Simple mixer control 'PCM',0
  Front Left: Playback 36 [40%] [-26.55dB] [on]
Simple mixer control 'Mic',0
  Mono: Capture [on]
Simple mixer control 'Extension Unit',0
  Mono: Playback [off]
";
        let s = parse_snapshot(dump);
        assert!(s.mic_capture);
        assert_eq!(s.pcm_percent, 40);
        assert!(s.pcm_on);
        assert!(!s.extension);
    }
}
