pub fn check_usb_access() -> Vec<String> {
    vec![
        "macOS: If the printer cannot be opened, remove it from \
         System Settings > Printers & Scanners to release the USB interface."
            .to_string(),
    ]
}
