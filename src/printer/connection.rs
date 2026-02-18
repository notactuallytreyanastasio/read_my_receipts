use escpos::driver::NativeUsbDriver;
use escpos::printer::Printer;
use escpos::utils::Protocol;
use crate::printer::models::EPSON_VENDOR_ID;

pub struct PrinterConnection {
    printer: Printer<NativeUsbDriver>,
    pub product_id: u16,
    pub model_name: String,
}

impl PrinterConnection {
    pub fn open(product_id: u16, model_name: String) -> Result<Self, String> {
        let driver = NativeUsbDriver::open(EPSON_VENDOR_ID, product_id)
            .map_err(|e| format!("Failed to open USB device {:04x}:{:04x}: {}", EPSON_VENDOR_ID, product_id, e))?;

        let printer = Printer::new(driver, Protocol::default(), None);

        Ok(Self {
            printer,
            product_id,
            model_name,
        })
    }

    pub fn print_text(&mut self, text: &str, bold: bool, underline: bool, double_size: bool) -> Result<(), String> {
        self.printer.init().map_err(|e| e.to_string())?;

        if bold {
            self.printer.bold(true).map_err(|e| e.to_string())?;
        }
        if underline {
            self.printer
                .underline(escpos::utils::UnderlineMode::Single)
                .map_err(|e| e.to_string())?;
        }
        if double_size {
            self.printer.size(2, 2).map_err(|e| e.to_string())?;
        }

        self.printer
            .writeln(text)
            .map_err(|e| e.to_string())?
            .feeds(3)
            .map_err(|e| e.to_string())?
            .print_cut()
            .map_err(|e| e.to_string())?;

        Ok(())
    }
}
