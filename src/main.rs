mod app;
mod error;
mod platform;
mod printer;

use app::App;

fn main() -> iced::Result {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("receipts=debug".parse().unwrap()),
        )
        .init();

    tracing::info!("Starting Receipts printer manager");

    iced::application(app::title, app::update, app::view)
        .theme(app::theme)
        .subscription(app::subscription)
        .window_size((600.0, 700.0))
        .centered()
        .run_with(|| {
            let app = App::default();
            let scan = iced::Task::perform(
                async { crate::printer::discovery::scan_for_printers() },
                app::Message::PrintersFound,
            );
            (app, scan)
        })
}
