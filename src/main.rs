use receipts::app::{self, App, DisplayMode};

fn main() -> iced::Result {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("receipts=debug".parse().unwrap()),
        )
        .init();

    let mode = detect_display_mode();
    tracing::info!("Starting Receipts printer manager (mode: {:?})", mode);

    let mut builder = iced::application(app::title, app::update, app::view)
        .theme(app::theme)
        .subscription(app::subscription);

    builder = match mode {
        DisplayMode::Desktop => builder.window_size((1100.0, 700.0)).centered(),
        DisplayMode::Kiosk => builder.window_size((320.0, 240.0)).decorations(false),
    };

    builder.run_with(move || {
        let app = App::new(mode);
        let scan = iced::Task::perform(
            async { receipts::printer::discovery::scan_for_printers() },
            app::Message::PrintersFound,
        );
        (app, scan)
    })
}

fn detect_display_mode() -> DisplayMode {
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--kiosk" => return DisplayMode::Kiosk,
            "--desktop" => return DisplayMode::Desktop,
            _ => {}
        }
    }
    // Platform default: Linux -> Kiosk, everything else -> Desktop
    if cfg!(target_os = "linux") {
        DisplayMode::Kiosk
    } else {
        DisplayMode::Desktop
    }
}
