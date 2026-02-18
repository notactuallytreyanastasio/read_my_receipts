use thiserror::Error;

#[derive(Debug, Error, Clone)]
pub enum AppError {
    #[error("No Epson printer found on USB")]
    NoPrinterFound,

    #[error("USB error: {0}")]
    Usb(String),

    #[error("Printer error: {0}")]
    Printer(String),

    #[error("Printer offline")]
    PrinterOffline,

    #[error("Paper out")]
    PaperOut,

    #[error("Cover open")]
    CoverOpen,

    #[error("Platform error: {0}")]
    Platform(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),
}
