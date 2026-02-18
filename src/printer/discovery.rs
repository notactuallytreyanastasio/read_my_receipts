use nusb::MaybeFuture;

use crate::printer::models::{find_known_model, is_epson_device};

#[derive(Debug, Clone)]
pub struct DiscoveredPrinter {
    pub vendor_id: u16,
    pub product_id: u16,
    pub model_name: String,
    pub serial: Option<String>,
}

pub fn scan_for_printers() -> Result<Vec<DiscoveredPrinter>, String> {
    let devices = nusb::list_devices()
        .wait()
        .map_err(|e| e.to_string())?;
    let mut printers = Vec::new();

    for dev in devices {
        let vid = dev.vendor_id();
        let pid = dev.product_id();

        if !is_epson_device(vid) {
            continue;
        }

        let model_name = if let Some(model) = find_known_model(vid, pid) {
            model.name.to_string()
        } else {
            dev.product_string()
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("Epson {:04x}", pid))
        };

        tracing::info!(
            "Found Epson device: {} (VID={:04x} PID={:04x})",
            model_name,
            vid,
            pid
        );

        printers.push(DiscoveredPrinter {
            vendor_id: vid,
            product_id: pid,
            model_name,
            serial: dev.serial_number().map(|s| s.to_string()),
        });
    }

    Ok(printers)
}
