//! Photo booth: countdown → 3 photos → print strip (no cuts).
//!
//! Designed to be spawned by the web server. Manages the display,
//! camera, and print pipeline as a standalone process.

use std::path::PathBuf;
use std::process::Command;

fn main() {
    let pics_dir = pics_dir();
    std::fs::create_dir_all(&pics_dir).unwrap_or_else(|e| {
        eprintln!("Cannot create {}: {e}", pics_dir.display());
        std::process::exit(1);
    });

    let port = std::env::var("UPLOAD_PORT").unwrap_or_else(|_| "80".to_string());

    // Step 1: Kill any existing camera preview
    eprintln!("[booth] Stopping camera preview...");
    let _ = Command::new("pkill").args(["-f", "rpicam-hello"]).status();
    std::thread::sleep(std::time::Duration::from_millis(500));

    // Step 2: Countdown + 3 photos
    let mut photos: Vec<PathBuf> = Vec::new();

    for i in 0..3 {
        // Countdown
        for text in ["3", "2", "1", "Cheese!"] {
            show_text(text, 900);
        }

        // Capture
        let now = chrono::Local::now();
        let filename = now.format(&format!("%Y%m%d_%H%M%S_{}.jpg", i + 1)).to_string();
        let output_path = pics_dir.join(&filename);

        eprintln!("[booth] Capturing photo {}...", i + 1);
        let status = Command::new("rpicam-still")
            .args([
                "-o", output_path.to_str().unwrap(),
                "--width", "4056",
                "--height", "3040",
                "-t", "500",
                "--nopreview",
                "--ev", "0.5",
            ])
            .status();

        match status {
            Ok(s) if s.success() => {
                eprintln!("[booth] Photo {}: {}", i + 1, output_path.display());
                photos.push(output_path);
            }
            Ok(s) => {
                eprintln!("[booth] rpicam-still failed: {s}");
                show_text("Camera error!", 2000);
                std::process::exit(1);
            }
            Err(e) => {
                eprintln!("[booth] Failed to run rpicam-still: {e}");
                std::process::exit(1);
            }
        }

        // Prompt between rounds
        match i {
            0 => show_text("Again!", 800),
            1 => show_text("Last one!", 800),
            2 => show_text("Printing!", 1500),
            _ => {}
        }
    }

    // Step 3: Print all 3 via /print/strip (no cuts)
    eprintln!("[booth] Printing {} photos as strip (no cuts)...", photos.len());
    for (i, photo) in photos.iter().enumerate() {
        let bytes = match std::fs::read(photo) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("[booth] Failed to read {}: {e}", photo.display());
                continue;
            }
        };

        // Extra feed after last photo so the tear doesn't cut into the image
        let feed = if i == photos.len() - 1 { 10 } else { 3 };
        let url = format!("http://localhost:{port}/print/strip?feed={feed}");
        let boundary = "----boothboundary";
        let mut body = Vec::new();
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"image\"; filename=\"photo.jpg\"\r\nContent-Type: image/jpeg\r\n\r\n"
            ).as_bytes(),
        );
        body.extend_from_slice(&bytes);
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

        let result = Command::new("curl")
            .args([
                "-s",
                "-X", "POST",
                "-H", &format!("Content-Type: multipart/form-data; boundary={boundary}"),
                "--data-binary", "@-",
                &url,
            ])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                child.stdin.take().unwrap().write_all(&body)?;
                child.wait_with_output()
            });

        match result {
            Ok(out) => {
                let resp = String::from_utf8_lossy(&out.stdout);
                eprintln!("[booth] Photo {} upload: {}", i + 1, resp.trim());
            }
            Err(e) => eprintln!("[booth] Upload failed: {e}"),
        }

        // Wait between prints so they don't overlap
        if i < photos.len() - 1 {
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
    }

    eprintln!("[booth] Complete!");
}

fn pics_dir() -> PathBuf {
    let mut dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    if dir.ends_with("target/release") || dir.ends_with("target/debug") {
        dir = dir.parent().unwrap().parent().unwrap().to_path_buf();
    }
    dir.join("pics")
}

/// Show text on the display using zenity, auto-closing after `ms` milliseconds.
fn show_text(text: &str, ms: u32) {
    let timeout_secs = (ms / 1000).max(1);
    let font_size = if text.len() <= 2 { "280" } else { "140" };
    let markup = format!(
        "<span font=\"{}\" weight=\"bold\">{}</span>",
        font_size, text
    );
    let child = Command::new("zenity")
        .args([
            "--info",
            "--text", &markup,
            "--no-wrap",
            "--timeout", &timeout_secs.to_string(),
        ])
        .env("DISPLAY", ":0")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();

    if let Ok(mut child) = child {
        std::thread::sleep(std::time::Duration::from_millis(100));
        let _ = Command::new("xdotool")
            .args(["key", "super+Up"])
            .env("DISPLAY", ":0")
            .status();
        std::thread::sleep(std::time::Duration::from_millis(ms as u64 - 100));
        let _ = child.kill();
        let _ = child.wait();
    } else {
        std::thread::sleep(std::time::Duration::from_millis(ms as u64));
    }
}
