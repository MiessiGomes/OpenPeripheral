//! Preferências do LED do AJ179: cores DPI gravadas à parte para o logo
//! poder apagar-se em todos os estágios sem perder os tons escolhidos.

use crate::rgb::Rgb;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Clone, Serialize, Deserialize)]
pub struct LedPrefs {
    /// `false`: iluminação 0x05 off + cores DPI a preto no rato.
    #[serde(default = "default_led_on")]
    pub led_on: bool,
    #[serde(default)]
    pub dpi_colors: Vec<String>,
    #[serde(default)]
    pub dpi_values: Vec<u16>,
    #[serde(default)]
    pub dpi_index: Option<u8>,
}

fn default_led_on() -> bool {
    true
}

impl Default for LedPrefs {
    fn default() -> Self {
        Self {
            led_on: true,
            dpi_colors: Vec::new(),
            dpi_values: Vec::new(),
            dpi_index: None,
        }
    }
}

pub fn path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".config/openperipheral/aj179-led.json")
}

pub fn load() -> LedPrefs {
    let Ok(raw) = fs::read_to_string(path()) else {
        return LedPrefs::default();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

pub fn save(prefs: &LedPrefs) {
    let p = path();
    if let Some(dir) = p.parent() {
        let _ = fs::create_dir_all(dir);
    }
    if let Ok(raw) = serde_json::to_string_pretty(prefs) {
        let _ = fs::write(p, raw);
    }
}

pub fn parse_colors(hexes: &[String]) -> Vec<Rgb> {
    hexes
        .iter()
        .filter_map(|h| Rgb::parse_hex(h).ok())
        .collect()
}

pub fn any_lit(colors: &[Rgb]) -> bool {
    colors.iter().any(|c| *c != Rgb::OFF)
}

pub fn remember_colors(prefs: &mut LedPrefs, colors: &[Rgb]) {
    if any_lit(colors) {
        prefs.dpi_colors = colors.iter().map(|c| c.to_hex()).collect();
    }
}

pub fn remember_values(prefs: &mut LedPrefs, values: &[u16]) {
    if values.iter().filter(|v| **v >= 50).count() >= 2 {
        prefs.dpi_values = values.to_vec();
    }
}
