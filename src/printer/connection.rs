use std::sync::{Arc, Mutex};

use crate::printer::models::EPSON_VENDOR_ID;
use escpos::driver::NativeUsbDriver;
use escpos::printer::Printer;
use escpos::utils::Protocol;

/// A shared, persistent USB connection. Wraps an optional `PrinterConnection`
/// behind `Arc<Mutex<>>` so the iced async task pool can use it across prints
/// without reopening the USB interface each time.
///
/// On macOS, the kernel holds the USB interface for ~200ms after close, causing
/// `kIOReturnExclusiveAccess` on rapid reopen. Keeping the connection open
/// across prints avoids this entirely.
pub type SharedConnection = Arc<Mutex<Option<PrinterConnection>>>;

pub struct PrinterConnection {
    printer: Printer<NativeUsbDriver>,
    pub product_id: u16,
    pub model_name: String,
}

impl PrinterConnection {
    pub fn open(product_id: u16, model_name: String) -> Result<Self, String> {
        let driver = NativeUsbDriver::open(EPSON_VENDOR_ID, product_id).map_err(|e| {
            let err_str = e.to_string();
            #[cfg(target_os = "macos")]
            {
                crate::platform::macos::cups_conflict_hint(product_id, &err_str)
            }
            #[cfg(not(target_os = "macos"))]
            {
                format!(
                    "Failed to open USB device {EPSON_VENDOR_ID:04x}:{product_id:04x}: {err_str}"
                )
            }
        })?;

        let printer = Printer::new(driver, Protocol::default(), None);

        Ok(Self {
            printer,
            product_id,
            model_name,
        })
    }

    pub fn print_rich(
        &mut self,
        blocks: &[crate::receipt_markdown::ReceiptBlock],
        max_chars: u8,
    ) -> Result<(), String> {
        self.printer.init().map_err(|e| e.to_string())?;

        let commands = crate::printer::rich_print::generate_commands(blocks, max_chars);
        crate::printer::rich_print::execute_commands(&mut self.printer, &commands)?;

        self.printer
            .feeds(3)
            .map_err(|e| e.to_string())?
            .print_cut()
            .map_err(|e| e.to_string())?;

        Ok(())
    }

    /// Print text without cutting — for continuous log-style output.
    pub fn print_no_cut(
        &mut self,
        blocks: &[crate::receipt_markdown::ReceiptBlock],
        max_chars: u8,
    ) -> Result<(), String> {
        self.printer.init().map_err(|e| e.to_string())?;

        let commands = crate::printer::rich_print::generate_commands(blocks, max_chars);
        crate::printer::rich_print::execute_commands(&mut self.printer, &commands)?;

        self.printer
            .feeds(3)
            .map_err(|e| e.to_string())?
            .print()
            .map_err(|e| e.to_string())?;

        Ok(())
    }

    /// Print a website message: text content + optional image, then cut.
    ///
    /// Text and image are printed as separate init cycles to avoid
    /// escpos bit_image resetting printer state and clearing buffered text.
    pub fn print_website_message(
        &mut self,
        blocks: &[crate::receipt_markdown::ReceiptBlock],
        max_chars: u8,
        image_bytes: Option<&[u8]>,
    ) -> Result<(), String> {
        // Print text portion
        self.printer.init().map_err(|e| e.to_string())?;
        let commands = crate::printer::rich_print::generate_commands(blocks, max_chars);
        crate::printer::rich_print::execute_commands(&mut self.printer, &commands)?;

        // Print image if present — re-init to isolate from text
        if let Some(bytes) = image_bytes {
            if !bytes.is_empty() {
                self.printer.feeds(2).map_err(|e| e.to_string())?;
                self.printer.init().map_err(|e| e.to_string())?;
                if let Err(e) = self.print_image(bytes) {
                    tracing::warn!("Image print failed (non-fatal): {e}");
                }
            }
        }

        // Feed and cut
        self.printer
            .feeds(3)
            .map_err(|e| e.to_string())?
            .print_cut()
            .map_err(|e| e.to_string())?;

        Ok(())
    }

    /// Print an image without cutting — for photo strip sequences.
    /// Sends the image + a small feed, but no cut command.
    /// If `bright` is true, applies indoor brightness boost before dithering.
    pub fn print_image_no_cut(&mut self, image_bytes: &[u8], extra_feed: u8, bright: bool) -> Result<(), String> {
        self.printer.init().map_err(|e| e.to_string())?;
        self.print_image_inner(image_bytes, bright)?;
        self.printer
            .feeds(extra_feed)
            .map_err(|e| e.to_string())?
            .print()
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Print an image using ESC/POS bit image commands.
    /// Resizes, applies gamma + Floyd-Steinberg dithering, then sends to printer.
    fn print_image(&mut self, image_bytes: &[u8]) -> Result<(), String> {
        self.print_image_inner(image_bytes, false)
    }

    fn print_image_inner(&mut self, image_bytes: &[u8], bright: bool) -> Result<(), String> {
        let img = image::load_from_memory(image_bytes)
            .map_err(|e| format!("Image decode failed: {e}"))?;

        // Rotate 90° clockwise so portrait photos print upright on receipt paper
        let img = img.rotate90();

        let resized = img.resize(512, u32::MAX, image::imageops::FilterType::Lanczos3);

        // Grayscale → gamma correction → Floyd-Steinberg dither
        let mut gray = resized.to_luma8();
        if bright {
            crate::printer::image_proc::dither_for_thermal_bright(&mut gray);
        } else {
            crate::printer::image_proc::dither_for_thermal(&mut gray);
        }

        let width = gray.width() as usize;
        let height = gray.height() as usize;
        let width_bytes = (width + 7) / 8;
        tracing::info!("Dithered image: {width}x{height}px, sending as banded raster");

        // Convert 8-bit dithered (0 or 255) to 1-bit packed raster
        let mut raster = vec![0u8; width_bytes * height];
        for y in 0..height {
            for x in 0..width {
                if gray.get_pixel(x as u32, y as u32)[0] == 0 {
                    raster[y * width_bytes + x / 8] |= 0x80 >> (x % 8);
                }
            }
        }

        // Send in bands of 24 rows with small delays to avoid overflowing
        // the printer's ~16KB receive buffer, which causes USB bus resets.
        const BAND_HEIGHT: usize = 24;

        for band_start in (0..height).step_by(BAND_HEIGHT) {
            let band_end = (band_start + BAND_HEIGHT).min(height);
            let band_h = band_end - band_start;

            // GS v 0: print raster bit image
            let cmd: [u8; 8] = [
                0x1d, 0x76, 0x30, 0x00,
                (width_bytes & 0xFF) as u8,
                ((width_bytes >> 8) & 0xFF) as u8,
                (band_h & 0xFF) as u8,
                ((band_h >> 8) & 0xFF) as u8,
            ];
            self.printer.custom(&cmd).map_err(|e| format!("Raster cmd failed: {e}"))?;

            let data_start = band_start * width_bytes;
            let data_end = band_end * width_bytes;
            self.printer
                .custom(&raster[data_start..data_end])
                .map_err(|e| format!("Raster data failed: {e}"))?;

            // Flush each band to USB immediately. The escpos library buffers
            // all commands until print() — without this, the entire image is
            // sent in one burst which overflows the printer's receive buffer
            // and causes a USB bus reset (kernel 6.12+).
            self.printer.print().map_err(|e| format!("Band flush failed: {e}"))?;
            std::thread::sleep(std::time::Duration::from_millis(100));
        }

        Ok(())
    }
}

/// Create a new empty shared connection slot.
pub fn new_shared() -> SharedConnection {
    Arc::new(Mutex::new(None))
}

/// Open a USB connection and store it in the shared slot.
/// If a connection is already open to the same printer, reuses it.
/// If open to a different printer, closes the old one first.
pub fn open_shared(
    shared: &SharedConnection,
    product_id: u16,
    model_name: String,
) -> Result<(), String> {
    let mut guard = shared.lock().map_err(|e| format!("Lock poisoned: {e}"))?;

    // Already connected to this printer? Keep it.
    if let Some(ref conn) = *guard {
        if conn.product_id == product_id {
            tracing::debug!("Reusing existing USB connection to {model_name}");
            return Ok(());
        }
        tracing::info!("Switching printer — closing old connection");
    }

    tracing::info!("Opening persistent USB connection to {model_name}");
    let conn = PrinterConnection::open(product_id, model_name)?;
    *guard = Some(conn);
    Ok(())
}

/// Close the shared connection (e.g., on disconnect or error).
pub fn close_shared(shared: &SharedConnection) {
    if let Ok(mut guard) = shared.lock() {
        if guard.is_some() {
            tracing::info!("Closing persistent USB connection");
            *guard = None;
        }
    }
}

/// Print using the shared connection. Opens a new connection if needed.
/// On USB error, clears the connection so the next call will reopen.
pub fn print_with_shared(
    shared: &SharedConnection,
    product_id: u16,
    model_name: String,
    f: impl FnOnce(&mut PrinterConnection) -> Result<(), String>,
) -> Result<(), String> {
    let mut guard = shared.lock().map_err(|e| format!("Lock poisoned: {e}"))?;

    // Open connection if not already open (or if it was cleared after an error)
    if guard.is_none() {
        tracing::info!("No active connection — opening USB to {model_name}");
        let conn = PrinterConnection::open(product_id, model_name.clone())?;
        *guard = Some(conn);
    }

    // Liveness check: send an init command to verify the USB pipe is functional.
    // If it fails, the connection is stale — close and reopen before printing.
    {
        let conn = guard.as_mut().unwrap();
        if let Err(e) = conn.printer.init() {
            tracing::warn!(
                "USB liveness check failed, reconnecting: {e}"
            );
            *guard = None;
            let conn = PrinterConnection::open(product_id, model_name.clone())?;
            *guard = Some(conn);
        }
    }

    let conn = guard.as_mut().unwrap();

    match f(conn) {
        Ok(()) => {
            tracing::info!("Print job completed successfully");
            Ok(())
        }
        Err(e) => {
            // USB error — connection is likely broken. Close it so next
            // print attempt will reopen fresh.
            tracing::warn!("Print failed, closing connection for recovery: {e}");
            *guard = None;
            Err(e)
        }
    }
}
