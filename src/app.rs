use iced::widget::{button, column, container, row, text, text_editor, Space};
use iced::{Element, Length, Subscription, Task, Theme};

use crate::printer::discovery::{self, DiscoveredPrinter};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionStatus {
    Disconnected,
    Scanning,
    Connected {
        model: String,
        serial: Option<String>,
    },
    Error(String),
}

pub struct App {
    content: text_editor::Content,
    status: ConnectionStatus,
    discovered: Vec<DiscoveredPrinter>,
    selected_printer: Option<usize>,
    platform_warnings: Vec<String>,
    last_result: Option<Result<String, String>>,
    printing: bool,
    bold: bool,
    underline: bool,
    double_size: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    EditorAction(text_editor::Action),
    ScanPrinters,
    PrintersFound(Result<Vec<DiscoveredPrinter>, String>),
    SelectPrinter(usize),
    Print,
    PrintResult(Result<(), String>),
    ToggleBold,
    ToggleUnderline,
    ToggleDoubleSize,
    DismissWarning(usize),
    HotplugEvent,
}

impl Default for App {
    fn default() -> Self {
        Self {
            content: text_editor::Content::new(),
            status: ConnectionStatus::Scanning,
            discovered: Vec::new(),
            selected_printer: None,
            platform_warnings: crate::platform::check_prerequisites(),
            last_result: None,
            printing: false,
            bold: false,
            underline: false,
            double_size: false,
        }
    }
}

pub fn title(_app: &App) -> String {
    String::from("Receipts")
}

pub fn update(app: &mut App, message: Message) -> Task<Message> {
    match message {
        Message::EditorAction(action) => {
            app.content.perform(action);
            Task::none()
        }

        Message::ScanPrinters => {
            app.status = ConnectionStatus::Scanning;
            Task::perform(
                async { discovery::scan_for_printers() },
                Message::PrintersFound,
            )
        }

        Message::PrintersFound(result) => {
            match result {
                Ok(printers) => {
                    if printers.is_empty() {
                        app.status = ConnectionStatus::Disconnected;
                        app.selected_printer = None;
                    } else {
                        let first = &printers[0];
                        app.status = ConnectionStatus::Connected {
                            model: first.model_name.clone(),
                            serial: first.serial.clone(),
                        };
                        app.selected_printer = Some(0);
                    }
                    app.discovered = printers;
                }
                Err(e) => {
                    app.status = ConnectionStatus::Error(e);
                    app.discovered.clear();
                    app.selected_printer = None;
                }
            }
            Task::none()
        }

        Message::SelectPrinter(idx) => {
            if let Some(printer) = app.discovered.get(idx) {
                app.selected_printer = Some(idx);
                app.status = ConnectionStatus::Connected {
                    model: printer.model_name.clone(),
                    serial: printer.serial.clone(),
                };
            }
            Task::none()
        }

        Message::HotplugEvent => {
            app.status = ConnectionStatus::Scanning;
            Task::perform(
                async { discovery::scan_for_printers() },
                Message::PrintersFound,
            )
        }

        Message::Print => {
            let content_text = app.content.text();
            if content_text.trim().is_empty() {
                app.last_result = Some(Err("Nothing to print".into()));
                return Task::none();
            }

            let Some(idx) = app.selected_printer else {
                app.last_result = Some(Err("No printer selected".into()));
                return Task::none();
            };
            let Some(printer_info) = app.discovered.get(idx).cloned() else {
                app.last_result = Some(Err("Printer not found".into()));
                return Task::none();
            };

            app.printing = true;
            app.last_result = None;

            let bold = app.bold;
            let underline = app.underline;
            let double_size = app.double_size;

            Task::perform(
                async move {
                    let mut conn =
                        crate::printer::connection::PrinterConnection::open(
                            printer_info.product_id,
                            printer_info.model_name.clone(),
                        )?;
                    conn.print_text(&content_text, bold, underline, double_size)
                },
                Message::PrintResult,
            )
        }

        Message::PrintResult(result) => {
            app.printing = false;
            app.last_result = Some(result.map(|_| "Printed successfully".into()));
            Task::none()
        }

        Message::ToggleBold => {
            app.bold = !app.bold;
            Task::none()
        }
        Message::ToggleUnderline => {
            app.underline = !app.underline;
            Task::none()
        }
        Message::ToggleDoubleSize => {
            app.double_size = !app.double_size;
            Task::none()
        }

        Message::DismissWarning(idx) => {
            if idx < app.platform_warnings.len() {
                app.platform_warnings.remove(idx);
            }
            Task::none()
        }
    }
}

pub fn view(app: &App) -> Element<'_, Message> {
    // Status bar
    let status_text = match &app.status {
        ConnectionStatus::Disconnected => String::from("[ ] No printer connected"),
        ConnectionStatus::Scanning => String::from("[~] Scanning..."),
        ConnectionStatus::Connected { model, serial } => {
            let serial_str = serial
                .as_ref()
                .map(|s| format!(" ({})", s))
                .unwrap_or_default();
            format!("[*] Connected: {}{}", model, serial_str)
        }
        ConnectionStatus::Error(e) => format!("[!] Error: {}", e),
    };

    let status_bar = row![
        text(status_text).size(14),
        Space::with_width(Length::Fill),
        button("Rescan").on_press(Message::ScanPrinters),
    ]
    .spacing(10)
    .padding(10);

    // Platform warnings
    let warnings_section: Element<'_, Message> = if app.platform_warnings.is_empty() {
        Space::new(0, 0).into()
    } else {
        column(
            app.platform_warnings
                .iter()
                .enumerate()
                .map(|(i, w)| {
                    row![
                        text(w).size(12),
                        button("x")
                            .on_press(Message::DismissWarning(i))
                            .padding(2),
                    ]
                    .spacing(5)
                    .into()
                })
                .collect::<Vec<_>>(),
        )
        .spacing(2)
        .padding(5)
        .into()
    };

    // Printer selector (if multiple printers found)
    let printer_selector: Element<'_, Message> = if app.discovered.len() > 1 {
        column(
            app.discovered
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let label = if app.selected_printer == Some(i) {
                        format!("(*) {}", p.model_name)
                    } else {
                        format!("( ) {}", p.model_name)
                    };
                    button(text(label).size(13))
                        .on_press(Message::SelectPrinter(i))
                        .padding(4)
                        .into()
                })
                .collect::<Vec<_>>(),
        )
        .spacing(2)
        .padding(5)
        .into()
    } else {
        Space::new(0, 0).into()
    };

    // Formatting toolbar
    let bold_label = if app.bold { "[B] Bold" } else { "B  Bold" };
    let underline_label = if app.underline {
        "[U] Underline"
    } else {
        "U  Underline"
    };
    let double_label = if app.double_size {
        "[2x] Double"
    } else {
        "2x  Double"
    };

    let toolbar = row![
        button(bold_label).on_press(Message::ToggleBold),
        button(underline_label).on_press(Message::ToggleUnderline),
        button(double_label).on_press(Message::ToggleDoubleSize),
    ]
    .spacing(5)
    .padding(5);

    // Text editor
    let editor = text_editor(&app.content)
        .on_action(Message::EditorAction)
        .height(400)
        .placeholder("Paste or type text to print...");

    // Print button + result
    let print_btn: Element<'_, Message> = if app.printing {
        button("Printing...").into()
    } else {
        let can_print =
            app.selected_printer.is_some() && !app.content.text().trim().is_empty();
        if can_print {
            button("Print").on_press(Message::Print).into()
        } else {
            button("Print").into()
        }
    };

    let result_display: Element<'_, Message> = match &app.last_result {
        Some(Ok(msg)) => text(msg).size(14).into(),
        Some(Err(msg)) => text(format!("Error: {}", msg)).size(14).into(),
        None => Space::new(0, 0).into(),
    };

    let bottom_bar = row![print_btn, Space::with_width(10), result_display]
        .spacing(10)
        .padding(10);

    // Layout
    let content = column![
        status_bar,
        warnings_section,
        printer_selector,
        toolbar,
        editor,
        bottom_bar,
    ]
    .spacing(0);

    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(10)
        .into()
}

pub fn theme(_app: &App) -> Theme {
    Theme::Dark
}

pub fn subscription(_app: &App) -> Subscription<Message> {
    Subscription::run(hotplug_watcher)
}

fn hotplug_watcher() -> impl futures::Stream<Item = Message> {
    iced::stream::channel(10, |mut output| async move {
        let watcher = match nusb::watch_devices() {
            Ok(w) => w,
            Err(e) => {
                tracing::warn!("Hotplug not available: {}", e);
                // Keep the future alive but do nothing
                futures::future::pending::<()>().await;
                return;
            }
        };

        use futures::SinkExt;
        use futures::StreamExt;
        let mut watcher = watcher;

        while let Some(event) = watcher.next().await {
            match &event {
                nusb::hotplug::HotplugEvent::Connected(info) => {
                    tracing::info!(
                        "USB connected: VID={:04x} PID={:04x}",
                        info.vendor_id(),
                        info.product_id()
                    );
                }
                nusb::hotplug::HotplugEvent::Disconnected(id) => {
                    tracing::info!("USB disconnected: {:?}", id);
                }
            }

            if let Err(_) = output.send(Message::HotplugEvent).await {
                break;
            }
        }
    })
}
