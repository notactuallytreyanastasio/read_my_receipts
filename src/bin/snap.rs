use std::path::PathBuf;
use std::process::Command;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // Default to pics/ directory relative to the binary's location,
    // falling back to ./pics/ if the binary isn't in the expected place.
    let pics_dir = args.get(1).map(PathBuf::from).unwrap_or_else(|| {
        let mut dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));
        // If we're in target/release, go up to project root
        if dir.ends_with("target/release") || dir.ends_with("target/debug") {
            dir = dir.parent().unwrap().parent().unwrap().to_path_buf();
        }
        dir.join("pics")
    });
    std::fs::create_dir_all(&pics_dir).unwrap_or_else(|e| {
        eprintln!("Cannot create {}: {e}", pics_dir.display());
        std::process::exit(1);
    });

    let now = chrono::Local::now();
    let filename = now.format("%Y%m%d_%H%M%S.jpg").to_string();
    let output_path = pics_dir.join(filename);

    let width = env_or("SNAP_WIDTH", "4056");
    let height = env_or("SNAP_HEIGHT", "3040");
    let timeout = env_or("SNAP_TIMEOUT", "1500");

    eprintln!("Capturing {width}x{height} → {}", output_path.display());

    let status = Command::new("rpicam-still")
        .args([
            "-o", output_path.to_str().unwrap(),
            "--width", &width,
            "--height", &height,
            "-t", &timeout,
            "--nopreview",
        ])
        .status();

    match status {
        Ok(s) if s.success() => {
            let size = std::fs::metadata(&output_path)
                .map(|m| m.len())
                .unwrap_or(0);
            println!("{}", output_path.display());
            eprintln!("Done ({} KB)", size / 1024);
        }
        Ok(s) => {
            eprintln!("rpicam-still exited with {s}");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("Failed to run rpicam-still: {e}");
            std::process::exit(1);
        }
    }
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}
