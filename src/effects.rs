use crate::protocol::Rgb;

/// Efeitos gerados no host (o firmware do DuoCast aplica frames RGB 0x81).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effect {
    Blink,
    Cycle,
    Wave,
    Lightning,
    Pulse,
}

impl Effect {
    pub const ALL: [Effect; 5] = [
        Effect::Blink,
        Effect::Cycle,
        Effect::Wave,
        Effect::Lightning,
        Effect::Pulse,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Effect::Blink => "Blink",
            Effect::Cycle => "Cycle",
            Effect::Wave => "Wave",
            Effect::Lightning => "Lightning",
            Effect::Pulse => "Pulse",
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "blink" => Some(Self::Blink),
            "cycle" => Some(Self::Cycle),
            "wave" => Some(Self::Wave),
            "lightning" => Some(Self::Lightning),
            "pulse" => Some(Self::Pulse),
            _ => None,
        }
    }
}

pub fn frame(effect: Effect, t: f32, color: Rgb, brightness: u8, speed: f32) -> (Rgb, Rgb) {
    let t = t * speed.clamp(0.2, 5.0);
    let c = color.scaled(brightness);
    match effect {
        Effect::Blink => {
            let on = (t * 4.0).fract() < 0.5;
            let v = if on { c } else { Rgb::OFF };
            (v, v)
        }
        Effect::Pulse => {
            let s =
                ((t * std::f32::consts::TAU * 0.6).sin() * 0.5 + 0.5) * brightness as f32 / 100.0;
            let p = Rgb::new(
                (c.r as f32 * s) as u8,
                (c.g as f32 * s) as u8,
                (c.b as f32 * s) as u8,
            );
            (p, p)
        }
        Effect::Cycle => {
            let hue = (t * 0.15).fract();
            let rgb = hsv(hue, 1.0, brightness as f32 / 100.0);
            (rgb, rgb)
        }
        Effect::Wave => {
            let hue_top = (t * 0.2).fract();
            let hue_bot = (hue_top + 0.5).fract();
            let v = brightness as f32 / 100.0;
            (hsv(hue_top, 1.0, v), hsv(hue_bot, 1.0, v))
        }
        Effect::Lightning => {
            let burst = (t * 3.0).fract() < 0.08;
            let v = if burst { c } else { Rgb::OFF };
            (v, if burst { Rgb::OFF } else { c })
        }
    }
}

fn hsv(h: f32, s: f32, v: f32) -> Rgb {
    let i = (h * 6.0).floor();
    let f = h * 6.0 - i;
    let p = v * (1.0 - s);
    let q = v * (1.0 - f * s);
    let t = v * (1.0 - (1.0 - f) * s);
    let (r, g, b) = match (i as i32).rem_euclid(6) {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    Rgb::new((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blink_toggles() {
        let c = Rgb::new(10, 20, 30);
        let (a, _) = frame(Effect::Blink, 0.0, c, 100, 1.0);
        let (b, _) = frame(Effect::Blink, 0.2, c, 100, 1.0);
        assert_ne!(a, b);
    }

    #[test]
    fn speed_advances_blink() {
        let c = Rgb::new(10, 20, 30);
        let (slow, _) = frame(Effect::Blink, 0.1, c, 100, 1.0);
        let (fast, _) = frame(Effect::Blink, 0.1, c, 100, 4.0);
        assert_ne!(slow, fast);
    }
}
