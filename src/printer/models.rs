pub const EPSON_VENDOR_ID: u16 = 0x04b8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrinterModel {
    pub name: &'static str,
    pub product_ids: &'static [u16],
    pub max_chars_per_line: u8,
    pub supports_partial_cut: bool,
}

pub static KNOWN_MODELS: &[PrinterModel] = &[
    PrinterModel {
        name: "TM-T88VI",
        product_ids: &[0x0e15, 0x0e28],
        max_chars_per_line: 48,
        supports_partial_cut: true,
    },
    PrinterModel {
        name: "TM-M50",
        product_ids: &[0x0e36],
        max_chars_per_line: 48,
        supports_partial_cut: true,
    },
];

pub fn find_known_model(vendor_id: u16, product_id: u16) -> Option<&'static PrinterModel> {
    if vendor_id != EPSON_VENDOR_ID {
        return None;
    }
    KNOWN_MODELS
        .iter()
        .find(|m| m.product_ids.contains(&product_id))
}

pub fn is_epson_device(vendor_id: u16) -> bool {
    vendor_id == EPSON_VENDOR_ID
}
