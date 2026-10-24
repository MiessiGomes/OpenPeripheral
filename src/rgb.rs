//! Cor RGB partilhada entre periféricos (LED DuoCast, DPI/luz AJ179, …).

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const OFF: Self = Self { r: 0, g: 0, b: 0 };

    pub fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    pub fn scaled(self, brightness: u8) -> Self {
        let b = brightness.min(100) as u16;
        Self {
            r: ((self.r as u16 * b) / 100) as u8,
            g: ((self.g as u16 * b) / 100) as u8,
            b: ((self.b as u16 * b) / 100) as u8,
        }
    }

    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    pub fn parse_hex(s: &str) -> Result<Self, String> {
        let t = s.trim().trim_start_matches('#');
        if t.len() != 6 {
            return Err(format!("cor hex inválida `{s}` (use RRGGBB)"));
        }
        let n = u32::from_str_radix(t, 16).map_err(|_| format!("cor hex inválida `{s}`"))?;
        Ok(Self {
            r: ((n >> 16) & 0xff) as u8,
            g: ((n >> 8) & 0xff) as u8,
            b: (n & 0xff) as u8,
        })
    }
}
