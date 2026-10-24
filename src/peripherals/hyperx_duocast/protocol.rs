//! Pacotes HID do controlador DuoCast (`03F0:098C`, interface 0).
//!
//! Relatório de feature de 65 bytes (byte 0 = report id 0). Usage page `0xFF90`,
//! 64 bytes de payload. Confirmado no descritor USB ao vivo e no NGENUITY
//! (`CMDMICDuoCastShowLighting` / `SetLightsInfo`).

pub const PACKET_SIZE: usize = 65;
pub const PAYLOAD_SIZE: usize = 64;

pub const RGB_MARKER: u8 = 0x81;
pub const REGISTER_MARKER: u8 = 0x04;
pub const EOT_MARKER: u8 = 0x08;

pub const REG_SAVE: u8 = 0x53;
pub const REG_APPLY: u8 = 0xF2;
pub const REG_POST_SAVE: u8 = 0x02;
pub const REG_SAVE_STOP: u8 = 0x23;
/// `CMDMICDuoCastResetLights` — `[1]=0x04`, `[2]=0xFD`, `[9]=0`
pub const REG_RESET_LIGHTS: u8 = 0xFD;

pub const EOT_MAGIC_0: u8 = 0xAA;
pub const EOT_MAGIC_1: u8 = 0x55;
pub const EOT_FLAG: u8 = 0x28;

pub use crate::rgb::Rgb;

pub fn empty_packet() -> [u8; PACKET_SIZE] {
    [0u8; PACKET_SIZE]
}

/// Cor direta: anel superior + inferior.
pub fn color_packet(top: Rgb, bottom: Rgb) -> [u8; PACKET_SIZE] {
    let mut p = empty_packet();
    p[1] = RGB_MARKER;
    p[2] = top.r;
    p[3] = top.g;
    p[4] = top.b;
    p[5] = RGB_MARKER;
    p[6] = bottom.r;
    p[7] = bottom.g;
    p[8] = bottom.b;
    p
}

pub fn register_packet(reg: u8, param1: u8, param2: u8) -> [u8; PACKET_SIZE] {
    let mut p = empty_packet();
    p[1] = REGISTER_MARKER;
    p[2] = reg;
    p[8] = param1;
    p[9] = param2;
    p
}

pub fn apply_packet() -> [u8; PACKET_SIZE] {
    register_packet(REG_APPLY, 0, 1)
}

pub fn save_begin_packet(color_packets: u8) -> [u8; PACKET_SIZE] {
    register_packet(REG_SAVE, color_packets, 0)
}

pub fn post_save_packet() -> [u8; PACKET_SIZE] {
    register_packet(REG_POST_SAVE, 0, 0)
}

pub fn save_stop_packet() -> [u8; PACKET_SIZE] {
    register_packet(REG_SAVE_STOP, 1, 0)
}

pub fn reset_lights_packet() -> [u8; PACKET_SIZE] {
    register_packet(REG_RESET_LIGHTS, 0, 0)
}

pub fn eot_packet(frame_count: u8) -> [u8; PACKET_SIZE] {
    let mut p = empty_packet();
    p[1] = EOT_MARKER;
    p[0x3C] = EOT_FLAG;
    p[0x3D] = frame_count;
    p[0x3F] = EOT_MAGIC_0;
    p[0x40] = EOT_MAGIC_1;
    p
}

/// Report 0x66 no dispositivo de áudio (`03F0:0A8C`, interface 3).
/// `CMDMICDuoCastAudioBase.InitCommand`: `[0]=0x66`, `[1]=cmd`, `[2]=payload`.
/// Opcode `0x01` devolve `66 01 55` no hardware. Opcode `0x02` reenumera o USB — não enviar.
pub const AUDIO_REPORT_ID: u8 = 0x66;
pub const AUDIO_PACKET_SIZE: usize = 64;
pub const AUDIO_PING_OP: u8 = 0x01;
pub const AUDIO_ACK: u8 = 0x55;

pub const AUDIO_SET_POLAR: u8 = 0x05;
pub const AUDIO_GET_POLAR: u8 = 0x85;
pub const AUDIO_SET_MUTE: u8 = 0x06;
pub const AUDIO_GET_MUTE: u8 = 0x86;
pub const AUDIO_SET_HIGHPASS: u8 = 0x07;
pub const AUDIO_GET_HIGHPASS: u8 = 0x87;
pub const AUDIO_SET_GAIN: u8 = 0x08;
pub const AUDIO_GET_GAIN: u8 = 0x88;
pub const AUDIO_SET_SIDETONE: u8 = 0x09;
pub const AUDIO_GET_SIDETONE: u8 = 0x89;

/// Enum do helper (`Bidirectional=0`, `Cardioid=1`, `Omnidirectional=2`, `Stereo=3`).
/// O DuoCast só escreve 1 e 2.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PolarPattern {
    Cardioid = 1,
    #[serde(rename = "omni", alias = "omnidirectional")]
    Omnidirectional = 2,
}

impl PolarPattern {
    pub fn from_wire(b: u8) -> Option<Self> {
        match b {
            1 => Some(Self::Cardioid),
            2 => Some(Self::Omnidirectional),
            _ => None,
        }
    }

    pub fn wire(self) -> u8 {
        self as u8
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Cardioid => "cardioid",
            Self::Omnidirectional => "omni",
        }
    }
}

pub fn audio_command_packet(command: u8, payload: u8) -> [u8; AUDIO_PACKET_SIZE] {
    let mut p = [0u8; AUDIO_PACKET_SIZE];
    p[0] = AUDIO_REPORT_ID;
    p[1] = command;
    p[2] = payload;
    p
}

pub fn audio_ping_packet() -> [u8; AUDIO_PACKET_SIZE] {
    audio_command_packet(AUDIO_PING_OP, 0)
}

pub fn audio_ping_ok(reply: &[u8]) -> bool {
    reply.len() >= 3
        && reply[0] == AUDIO_REPORT_ID
        && reply[1] == AUDIO_PING_OP
        && reply[2] == AUDIO_ACK
}

pub fn audio_reply_value(reply: &[u8], expected_cmd: u8) -> Option<u8> {
    if reply.len() >= 3 && reply[0] == AUDIO_REPORT_ID && reply[1] == expected_cmd {
        Some(reply[2])
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_layout() {
        let p = color_packet(Rgb::new(0xf2, 0, 0), Rgb::new(0, 0xff, 0x10));
        assert_eq!(p[0], 0);
        assert_eq!(&p[1..9], &[0x81, 0xf2, 0, 0, 0x81, 0, 0xff, 0x10]);
        assert!(p[9..].iter().all(|b| *b == 0));
    }

    #[test]
    fn register_layout() {
        let p = register_packet(REG_APPLY, 0, 1);
        assert_eq!(p[1], 0x04);
        assert_eq!(p[2], 0xF2);
        assert_eq!(p[8], 0);
        assert_eq!(p[9], 1);
    }

    #[test]
    fn eot_layout() {
        let p = eot_packet(1);
        assert_eq!(p[1], 0x08);
        assert_eq!(p[0x3C], 0x28);
        assert_eq!(p[0x3D], 1);
        assert_eq!(p[0x3F], 0xAA);
        assert_eq!(p[0x40], 0x55);
    }

    #[test]
    fn brightness_scales() {
        let c = Rgb::new(200, 100, 50).scaled(50);
        assert_eq!(c, Rgb::new(100, 50, 25));
    }

    #[test]
    fn parse_hex_color() {
        assert_eq!(Rgb::parse_hex("#ff8800").unwrap(), Rgb::new(0xff, 0x88, 0));
        assert!(Rgb::parse_hex("xyz").is_err());
    }

    #[test]
    fn ping_packet_and_ack() {
        let p = audio_ping_packet();
        assert_eq!(p[0], 0x66);
        assert_eq!(p[1], 0x01);
        assert!(audio_ping_ok(&[0x66, 0x01, 0x55]));
        assert!(!audio_ping_ok(&[0x66, 0x02, 0x55]));
    }

    #[test]
    fn audio_set_polar_cardioid() {
        let p = audio_command_packet(AUDIO_SET_POLAR, PolarPattern::Cardioid.wire());
        assert_eq!(&p[..3], &[0x66, 0x05, 0x01]);
    }

    #[test]
    fn audio_get_highpass() {
        let p = audio_command_packet(AUDIO_GET_HIGHPASS, 0);
        assert_eq!(&p[..3], &[0x66, 0x87, 0x00]);
        assert_eq!(
            audio_reply_value(&[0x66, 0x87, 0x01], AUDIO_GET_HIGHPASS),
            Some(1)
        );
    }

    #[test]
    fn reset_lights_register() {
        let p = reset_lights_packet();
        assert_eq!(p[1], 0x04);
        assert_eq!(p[2], 0xFD);
        assert_eq!(p[9], 0);
    }
}
