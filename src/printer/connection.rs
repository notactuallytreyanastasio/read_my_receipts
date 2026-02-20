use crate::printer::models::EPSON_VENDOR_ID;
use escpos::driver::NativeUsbDriver;
use escpos::printer::Printer;
use escpos::utils::Protocol;

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

    /// Print an image using ESC/POS bit image commands.
    /// Resizes to printer width first, then sends to escpos.
    fn print_image(&mut self, image_bytes: &[u8]) -> Result<(), String> {
        use escpos::utils::BitImageOption;

        // Resize to 512px wide before sending — raw web images can be multi-MB
        // which chokes the printer's limited memory. 512px leaves margin for
        // TM-T88VI's physical non-printable edges on 80mm paper.
        let img = image::load_from_memory(image_bytes)
            .map_err(|e| format!("Image decode failed: {e}"))?;
        let resized = img.resize(512, u32::MAX, image::imageops::FilterType::Lanczos3);
        let mut buf = std::io::Cursor::new(Vec::new());
        resized
            .write_to(&mut buf, image::ImageFormat::Png)
            .map_err(|e| format!("PNG encode failed: {e}"))?;
        let resized_bytes = buf.into_inner();
        tracing::info!(
            "Resized image: {}x{}px, {} bytes",
            resized.width(),
            resized.height(),
            resized_bytes.len()
        );

        let option = BitImageOption::new(Some(512), None, Default::default())
            .map_err(|e| format!("Image option error: {e}"))?;

        self.printer
            .bit_image_from_bytes_option(&resized_bytes, option)
            .map_err(|e| format!("Image print failed: {e}"))?;

        Ok(())
    }
}
