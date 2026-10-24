pub mod error;
pub mod hid;
pub mod peripherals;
pub mod rgb;
pub mod update;

pub use error::{Error, Result};
pub use hid::FoundDevice;
pub use rgb::Rgb;
