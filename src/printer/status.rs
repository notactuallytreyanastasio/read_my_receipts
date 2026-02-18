#[derive(Debug, Clone, Default)]
pub struct PrinterStatus {
    pub online: bool,
    pub cover_open: bool,
    pub paper_near_end: bool,
    pub paper_out: bool,
    pub error: bool,
}

impl PrinterStatus {
    pub fn from_status_bytes(printer_byte: u8, offline_byte: u8, paper_byte: u8) -> Self {
        Self {
            online: (printer_byte & 0x08) == 0,
            cover_open: (offline_byte & 0x04) != 0,
            error: (offline_byte & 0x20) != 0,
            paper_near_end: (paper_byte & 0x0C) != 0,
            paper_out: (paper_byte & 0x60) != 0,
        }
    }

    pub fn summary(&self) -> &'static str {
        if !self.online {
            return "Offline";
        }
        if self.paper_out {
            return "Paper Out";
        }
        if self.cover_open {
            return "Cover Open";
        }
        if self.error {
            return "Error";
        }
        if self.paper_near_end {
            return "Paper Low";
        }
        "Ready"
    }
}
