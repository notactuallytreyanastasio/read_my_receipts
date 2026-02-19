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
}
