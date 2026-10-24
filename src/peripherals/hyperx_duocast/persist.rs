use crate::rgb::Rgb;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Clone, Serialize, Deserialize)]
pub struct SavedState {
    pub top: String,
    pub bottom: String,
    pub brightness: u8,
    pub link: bool,
}

impl Default for SavedState {
    fn default() -> Self {
        Self {
            top: "#f20000".into(),
            bottom: "#f20000".into(),
            brightness: 80,
            link: true,
        }
    }
}

pub fn path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".config/openperipheral/duocast.json")
}

pub fn load() -> SavedState {
    let p = path();
    let Ok(raw) = fs::read_to_string(&p) else {
        return SavedState::default();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

pub fn save(state: &SavedState) {
    let p = path();
    if let Some(dir) = p.parent() {
        let _ = fs::create_dir_all(dir);
    }
    if let Ok(raw) = serde_json::to_string_pretty(state) {
        let _ = fs::write(p, raw);
    }
}

pub fn parse_rgb(hex: &str, fallback: Rgb) -> Rgb {
    Rgb::parse_hex(hex).unwrap_or(fallback)
}
