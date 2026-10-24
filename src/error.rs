use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("hidapi: {0}")]
    Hid(#[from] hidapi::HidError),
    #[error("DuoCast RGB controller 03F0:098C interface 0 não encontrado (libusb)")]
    LightingNotFound,
    #[error("DuoCast áudio HID 03F0:0A8C interface 3 não encontrado")]
    AudioHidNotFound,
    #[error("AJAZZ AJ179 (interface HID 2) não encontrado")]
    Aj179NotFound,
    #[error("amixer falhou: {0}")]
    Mixer(String),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
