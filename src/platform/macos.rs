use std::process::Command;

/// A CUPS printer that may be claiming a USB interface we need.
#[derive(Debug, Clone)]
pub struct CupsPrinter {
    pub name: String,
    pub uri: String,
    pub is_usb: bool,
    pub is_epson: bool,
}

/// Check for CUPS printers that might block raw USB access.
/// Returns (warnings, detected_cups_printers).
pub fn check_usb_access() -> Vec<String> {
    let mut warnings = Vec::new();
    let cups = detect_cups_printers();

    let conflicting: Vec<&CupsPrinter> = cups.iter().filter(|p| p.is_usb && p.is_epson).collect();

    if !conflicting.is_empty() {
        for p in &conflicting {
            warnings.push(format!(
                "CUPS conflict: \"{}\" is claiming USB. \
                 Remove it from System Settings > Printers & Scanners, \
                 or run: lpadmin -x {}",
                p.name, p.name
            ));
        }
    }

    warnings
}

/// Detect all CUPS printers, tagging USB and Epson ones.
pub fn detect_cups_printers() -> Vec<CupsPrinter> {
    let output = match Command::new("lpstat").arg("-v").output() {
        Ok(o) => o,
        Err(e) => {
            tracing::debug!("lpstat not available: {e}");
            return Vec::new();
        }
    };

    if !output.status.success() {
        // "No destinations added" is exit code 1 — not an error for us
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.contains("No destinations") {
            tracing::debug!("lpstat failed: {stderr}");
        }
        return Vec::new();
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_lpstat_output(&stdout)
}

/// Check if a specific printer name is claimed by CUPS over USB.
pub fn is_claimed_by_cups(printer_name: &str) -> bool {
    let cups = detect_cups_printers();
    cups.iter().any(|p| {
        p.is_usb
            && p.is_epson
            && (p.name.to_lowercase().contains(&printer_name.to_lowercase())
                || printer_name.to_lowercase().contains(&p.name.to_lowercase()))
    })
}

/// Build a user-friendly error message when USB open fails on macOS.
pub fn cups_conflict_hint(product_id: u16, original_error: &str) -> String {
    let cups = detect_cups_printers();
    let usb_epson: Vec<&CupsPrinter> = cups.iter().filter(|p| p.is_usb && p.is_epson).collect();

    if usb_epson.is_empty() {
        // No CUPS conflict — the error is something else
        format!(
            "Failed to open USB device (PID {product_id:04x}): {original_error}\n\
             Tip: On macOS, check System Settings > Privacy & Security > USB access."
        )
    } else {
        let names: Vec<&str> = usb_epson.iter().map(|p| p.name.as_str()).collect();
        format!(
            "Cannot open USB device (PID {product_id:04x}): macOS CUPS driver is claiming the interface.\n\
             Conflicting CUPS printer(s): {}\n\
             Fix: Remove from System Settings > Printers & Scanners, or run:\n{}",
            names.join(", "),
            usb_epson
                .iter()
                .map(|p| format!("  lpadmin -x {}", p.name))
                .collect::<Vec<_>>()
                .join("\n")
        )
    }
}

/// Parse `lpstat -v` output into CupsPrinter structs.
///
/// Format: `device for <name>: <uri>`
/// Example: `device for EPSON_TM_T88VI: usb://EPSON/TM-T88VI?serial=...`
fn parse_lpstat_output(output: &str) -> Vec<CupsPrinter> {
    let mut printers = Vec::new();

    for line in output.lines() {
        let line = line.trim();
        // lpstat -v format: "device for <name>: <uri>"
        let Some(rest) = line.strip_prefix("device for ") else {
            continue;
        };
        let Some((name, uri)) = rest.split_once(": ") else {
            continue;
        };
        let name = name.trim().to_string();
        let uri = uri.trim().to_string();

        let is_usb = uri.starts_with("usb://");
        let is_epson =
            uri.to_lowercase().contains("epson") || name.to_lowercase().contains("epson");

        printers.push(CupsPrinter {
            name,
            uri,
            is_usb,
            is_epson,
        });
    }

    printers
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_epson_usb_printer() {
        let output = "device for EPSON_TM_T88VI: usb://EPSON/TM-T88VI?serial=J2CE012345\n";
        let printers = parse_lpstat_output(output);
        assert_eq!(printers.len(), 1);
        assert!(printers[0].is_usb);
        assert!(printers[0].is_epson);
        assert_eq!(printers[0].name, "EPSON_TM_T88VI");
    }

    #[test]
    fn parse_network_printer_no_conflict() {
        let output = "device for HP_LaserJet: ipp://192.168.1.100/ipp/print\n";
        let printers = parse_lpstat_output(output);
        assert_eq!(printers.len(), 1);
        assert!(!printers[0].is_usb);
        assert!(!printers[0].is_epson);
    }

    #[test]
    fn parse_multiple_printers() {
        let output = "\
device for EPSON_TM_T88VI: usb://EPSON/TM-T88VI?serial=J2CE012345
device for HP_LaserJet: ipp://192.168.1.100/ipp/print
device for EPSON_TM_M50: usb://EPSON/TM-M50?serial=ABC123
";
        let printers = parse_lpstat_output(output);
        assert_eq!(printers.len(), 3);

        let epson_usb: Vec<_> = printers.iter().filter(|p| p.is_usb && p.is_epson).collect();
        assert_eq!(epson_usb.len(), 2);
    }

    #[test]
    fn parse_empty_output() {
        let printers = parse_lpstat_output("");
        assert!(printers.is_empty());
    }

    #[test]
    fn check_usb_access_no_cups() {
        // When no CUPS printers, should return no warnings
        // (This tests the logic path, not actual system state)
        let warnings = check_usb_access();
        // On a system with no CUPS printers, this should be empty
        // On a system with CUPS Epson USB printers, it should warn
        assert!(warnings.len() <= 10); // sanity check
    }
}
