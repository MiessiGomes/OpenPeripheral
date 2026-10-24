/// Identidades USB observadas no NGENUITY e no hardware ao vivo.
pub const HP_VID: u16 = 0x03F0;

/// Controlador HID RGB (produto `HyperX DuoCast Controller`).
/// Interface 0: usage page 0xFF90, feature 64 bytes, sem interrupt IN
/// (hidraw do Linux não associa — usar backend libusb).
pub const CONTROLLER_PID: u16 = 0x098C;
pub const LIGHTING_INTERFACE: i32 = 0;

/// Dispositivo de áudio USB (`HyperX DuoCast`) + HID vendor report 0x66.
/// Também o PID que o INF Realtek e o NGENUITY chamam de DuoCast.
pub const AUDIO_PID: u16 = 0x0A8C;
pub const AUDIO_HID_INTERFACE: i32 = 3;
