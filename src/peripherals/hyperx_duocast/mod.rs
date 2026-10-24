//! HyperX DuoCast: LED HID `03F0:098C` + áudio HID `03F0:0A8C` (0x66) + ALSA.

pub mod device;
pub mod effects;
pub mod ids;
pub mod mixer;
pub mod monitor;
pub mod persist;
pub mod protocol;

pub use device::{enumerate, AudioHid, Lighting};
pub use effects::Effect;
pub use protocol::{PolarPattern, Rgb};
