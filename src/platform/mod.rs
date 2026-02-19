#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
pub mod macos;

pub fn check_prerequisites() -> Vec<String> {
    let mut warnings = Vec::new();

    #[cfg(target_os = "macos")]
    {
        warnings.extend(macos::check_usb_access());
    }

    #[cfg(target_os = "linux")]
    {
        warnings.extend(linux::check_usb_access());
    }

    warnings
}
