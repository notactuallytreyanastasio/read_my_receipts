//! Headless web server for receipt printing.
//!
//! Replaces the iced GUI app for the Pi use case — no X11, no DISPLAY needed.
//! Runs the axum upload server on port 80 and processes print jobs directly.

use receipts::printer::connection::{self, SharedConnection};
use receipts::printer::discovery;
use receipts::printer::models::find_known_model;
use receipts::receipt_markdown;
use receipts::upload_server::handler::{self, PrintPayload};

use tokio::sync::mpsc;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("receipts=info".parse().unwrap())
                .add_directive("server=info".parse().unwrap()),
        )
        .init();

    tracing::info!("Starting headless receipt server");

    // Discover printer
    let printer = match discovery::scan_for_printers() {
        Ok(printers) => {
            if let Some(p) = printers.into_iter().next() {
                tracing::info!("Found printer: {} (PID={:04x})", p.model_name, p.product_id);
                Some(p)
            } else {
                tracing::warn!("No Epson printer found — will retry on each print job");
                None
            }
        }
        Err(e) => {
            tracing::warn!("USB scan failed: {e} — will retry on each print job");
            None
        }
    };

    // Open shared USB connection
    let shared = connection::new_shared();
    if let Some(ref p) = printer {
        if let Err(e) = connection::open_shared(&shared, p.product_id, p.model_name.clone()) {
            tracing::warn!("Initial USB connection failed: {e}");
        }
    }

    // Channel for print payloads from the web handler
    let (tx, rx) = mpsc::channel::<PrintPayload>(32);

    // Spawn print worker
    let worker_shared = shared.clone();
    let worker_printer = printer.clone();
    tokio::task::spawn_blocking(move || {
        print_worker(rx, worker_shared, worker_printer);
    });

    // Build and serve the axum router
    let router = handler::build_router(tx);
    let bind_addr = "0.0.0.0:80";

    let listener = match tokio::net::TcpListener::bind(bind_addr).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("Failed to bind {bind_addr}: {e}");
            std::process::exit(1);
        }
    };

    // Disable display sleep so the screen stays on for the photo booth
    let _ = std::process::Command::new("xset")
        .args(["s", "off"])
        .env("DISPLAY", ":0")
        .status();
    let _ = std::process::Command::new("xset")
        .args(["-dpms"])
        .env("DISPLAY", ":0")
        .status();
    let _ = std::process::Command::new("xset")
        .args(["s", "noblank"])
        .env("DISPLAY", ":0")
        .status();

    tracing::info!("Listening on {bind_addr}");

    if let Err(e) = axum::serve(listener, router).await {
        tracing::error!("Server error: {e}");
        std::process::exit(1);
    }
}

/// Blocking print worker — receives payloads from the channel and prints them.
/// Runs in a blocking thread since USB I/O is synchronous.
fn print_worker(
    mut rx: mpsc::Receiver<PrintPayload>,
    shared: SharedConnection,
    initial_printer: Option<discovery::DiscoveredPrinter>,
) {
    // Cache printer info; re-scan if we didn't find one at startup
    let mut cached_printer = initial_printer;

    while let Some(payload) = rx.blocking_recv() {
        // If we don't have a printer, try scanning again
        if cached_printer.is_none() {
            if let Ok(printers) = discovery::scan_for_printers() {
                if let Some(p) = printers.into_iter().next() {
                    tracing::info!(
                        "Discovered printer on retry: {} (PID={:04x})",
                        p.model_name,
                        p.product_id
                    );
                    cached_printer = Some(p);
                }
            }
        }

        let Some(ref printer) = cached_printer else {
            tracing::error!("No printer available — dropping print job");
            continue;
        };

        let product_id = printer.product_id;
        let model_name = printer.model_name.clone();
        let max_chars = find_known_model(0x04b8, product_id)
            .map(|m| m.max_chars_per_line)
            .unwrap_or(42);

        let result = match payload {
            PrintPayload::Image(bytes) => {
                tracing::info!("Printing image: {} bytes", bytes.len());
                connection::print_with_shared(&shared, product_id, model_name, |conn| {
                    conn.print_website_message(&[], max_chars, Some(&bytes))
                })
            }
            PrintPayload::ImageNoCut(bytes, feed, bright) => {
                tracing::info!("Printing strip image (no cut, feed={}, bright={}): {} bytes", feed, bright, bytes.len());
                connection::print_with_shared(&shared, product_id, model_name, |conn| {
                    conn.print_image_no_cut(&bytes, feed, bright)
                })
            }
            PrintPayload::Text { text, source } => {
                tracing::info!("Printing text: {} bytes (source={})", text.len(), source);
                let mut blocks = receipt_markdown::parse_receipt_markdown(&text);
                blocks.push(receipt_markdown::ReceiptBlock::BlankLine);
                blocks.push(receipt_markdown::ReceiptBlock::BlankLine);
                blocks.push(receipt_markdown::ReceiptBlock::BlankLine);
                connection::print_with_shared(&shared, product_id, model_name, |conn| {
                    conn.print_no_cut(&blocks, max_chars)
                })
            }
        };

        match &result {
            Ok(()) => {
                // Give the printer time to finish physically printing before
                // sending the next job — prevents USB disconnects under load.
                std::thread::sleep(std::time::Duration::from_secs(3));
            }
            Err(e) => {
                tracing::error!("Print failed: {e}");
                // Brief pause before retrying to let USB recover
                std::thread::sleep(std::time::Duration::from_secs(2));
            }
        }
    }

    tracing::info!("Print worker shutting down");
}
