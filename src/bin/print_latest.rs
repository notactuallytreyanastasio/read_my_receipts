use std::path::PathBuf;

fn main() {
    let pics_dir = pics_dir();
    let latest = find_latest_jpg(&pics_dir);

    match latest {
        Some(path) => {
            eprintln!("Printing: {}", path.display());
            let bytes = std::fs::read(&path).unwrap_or_else(|e| {
                eprintln!("Failed to read {}: {e}", path.display());
                std::process::exit(1);
            });
            send_to_printer(&bytes);
            println!("{}", path.display());
        }
        None => {
            eprintln!("No .jpg files found in {}", pics_dir.display());
            std::process::exit(1);
        }
    }
}

fn pics_dir() -> PathBuf {
    let args: Vec<String> = std::env::args().collect();
    if let Some(dir) = args.get(1) {
        return PathBuf::from(dir);
    }
    let mut dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    if dir.ends_with("target/release") || dir.ends_with("target/debug") {
        dir = dir.parent().unwrap().parent().unwrap().to_path_buf();
    }
    dir.join("pics")
}

fn find_latest_jpg(dir: &PathBuf) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("jpg"))
        })
        .max_by_key(|e| e.metadata().ok().and_then(|m| m.modified().ok()))
        .map(|e| e.path())
}

fn send_to_printer(bytes: &[u8]) {
    let port = std::env::var("UPLOAD_PORT").unwrap_or_else(|_| "80".to_string());
    let url = format!("http://localhost:{port}/print/upload");

    let boundary = "----snapboundary";
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"image\"; filename=\"photo.jpg\"\r\nContent-Type: image/jpeg\r\n\r\n").as_bytes());
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    let output = std::process::Command::new("curl")
        .args([
            "-s", "-w", "%{http_code}",
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

    match output {
        Ok(out) => {
            let resp = String::from_utf8_lossy(&out.stdout);
            if resp.ends_with("200") {
                eprintln!("Sent to printer ({} KB)", bytes.len() / 1024);
            } else {
                eprintln!("Upload failed: {resp}");
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("Failed to send: {e}");
            std::process::exit(1);
        }
    }
}
