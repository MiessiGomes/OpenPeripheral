//! Identidade USB/HID comum à enumeração de todos os periféricos.

use hidapi::DeviceInfo;

#[derive(Clone, Debug)]
pub struct FoundDevice {
    pub vid: u16,
    pub pid: u16,
    pub interface: i32,
    pub product: String,
    pub path: String,
}

pub fn found_from_info(info: &DeviceInfo) -> FoundDevice {
    FoundDevice {
        vid: info.vendor_id(),
        pid: info.product_id(),
        interface: info.interface_number(),
        product: info.product_string().unwrap_or("").to_string(),
        path: info.path().to_string_lossy().into_owned(),
    }
}
