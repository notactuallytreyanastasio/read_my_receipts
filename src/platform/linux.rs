use std::path::Path;

const UDEV_RULES_PATH: &str = "/etc/udev/rules.d/99-epson-receipt.rules";

pub fn check_usb_access() -> Vec<String> {
    let mut warnings = Vec::new();

    if !Path::new(UDEV_RULES_PATH).exists() {
        warnings.push(format!(
            "Linux: udev rules not found at {}. \
             Install them for non-root USB access. \
             See assets/udev/99-epson-receipt.rules.",
            UDEV_RULES_PATH
        ));
    }

    if let Ok(output) = std::process::Command::new("groups").output() {
        let groups = String::from_utf8_lossy(&output.stdout);
        if !groups.contains("plugdev") && !groups.contains("lp") {
            warnings.push(
                "Linux: Current user not in 'plugdev' or 'lp' group. \
                 USB printer access may require group membership."
                    .to_string(),
            );
        }
    }

    warnings
}
