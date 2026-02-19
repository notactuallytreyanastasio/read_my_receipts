use iced::widget::{
    button, column, container, rich_text, row, scrollable, span, text, text_editor, Space,
};
use iced::{font, time, Color, Element, Font, Length, Subscription, Task, Theme};

use crate::printer::discovery::{self, DiscoveredPrinter};
use crate::printer::models::{find_known_model, EPSON_VENDOR_ID};
use crate::receipt_markdown::{parse_receipt_markdown, Alignment, ReceiptBlock};
use crate::word_wrap::{wrap_document, WrappedLine};

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
    parsed_blocks: Vec<ReceiptBlock>,
    wrapped_lines: Vec<WrappedLine>,
    status: ConnectionStatus,
    discovered: Vec<DiscoveredPrinter>,
    selected_printer: Option<usize>,
    platform_warnings: Vec<String>,
    last_result: Option<Result<String, String>>,
    printing: bool,
    show_help: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    EditorAction(text_editor::Action),
    ScanPrinters,
    PrintersFound(Result<Vec<DiscoveredPrinter>, String>),
    SelectPrinter(usize),
    Print,
    PrintResult(Result<(), String>),
    DismissWarning(usize),
    HotplugEvent,
    HealthCheck,
    ToggleHelp,
}

fn current_max_chars(app: &App) -> u8 {
    app.selected_printer
        .and_then(|idx| app.discovered.get(idx))
        .and_then(|p| find_known_model(EPSON_VENDOR_ID, p.product_id))
        .map(|m| m.max_chars_per_line)
        .unwrap_or(42)
}

fn reparse(app: &mut App) {
    let input = app.content.text();
    app.parsed_blocks = parse_receipt_markdown(&input);
    let max_chars = current_max_chars(app);
    app.wrapped_lines = wrap_document(&app.parsed_blocks, max_chars);
}

impl Default for App {
    fn default() -> Self {
        Self {
            content: text_editor::Content::new(),
            parsed_blocks: Vec::new(),
            wrapped_lines: Vec::new(),
            status: ConnectionStatus::Scanning,
            discovered: Vec::new(),
            selected_printer: None,
            platform_warnings: crate::platform::check_prerequisites(),
            last_result: None,
            printing: false,
            show_help: false,
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
            reparse(app);
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
            reparse(app);
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
            reparse(app);
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
            if app.parsed_blocks.is_empty() || app.content.text().trim().is_empty() {
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

            let blocks = app.parsed_blocks.clone();
            let max_chars = current_max_chars(app);

            Task::perform(
                async move {
                    let mut conn = crate::printer::connection::PrinterConnection::open(
                        printer_info.product_id,
                        printer_info.model_name.clone(),
                    )?;
                    conn.print_rich(&blocks, max_chars)
                },
                Message::PrintResult,
            )
        }

        Message::PrintResult(result) => {
            app.printing = false;
            app.last_result = Some(result.map(|_| "Printed successfully".into()));
            Task::none()
        }

        Message::DismissWarning(idx) => {
            if idx < app.platform_warnings.len() {
                app.platform_warnings.remove(idx);
            }
            Task::none()
        }

        Message::ToggleHelp => {
            app.show_help = !app.show_help;
            Task::none()
        }

        Message::HealthCheck => {
            // Periodic health check — verify connected printer is still present
            if app.selected_printer.is_some() {
                Task::perform(
                    async { discovery::scan_for_printers() },
                    Message::PrintersFound,
                )
            } else {
                // No printer selected — try to find one
                Task::perform(
                    async { discovery::scan_for_printers() },
                    Message::PrintersFound,
                )
            }
        }
    }
}

pub fn view(app: &App) -> Element<'_, Message> {
    // Status bar
    let status_text = match &app.status {
        ConnectionStatus::Disconnected => String::from("No printer connected"),
        ConnectionStatus::Scanning => String::from("Scanning..."),
        ConnectionStatus::Connected { model, serial } => {
            let serial_str = serial
                .as_ref()
                .map(|s| format!(" ({s})"))
                .unwrap_or_default();
            format!("Connected: {model}{serial_str}")
        }
        ConnectionStatus::Error(e) => format!("Error: {e}"),
    };

    let status_color = match &app.status {
        ConnectionStatus::Connected { .. } => Color::from_rgb(0.20, 0.78, 0.35),
        ConnectionStatus::Error(_) => Color::from_rgb(1.0, 0.23, 0.19),
        ConnectionStatus::Scanning => Color::from_rgb(0.55, 0.55, 0.58),
        ConnectionStatus::Disconnected => Color::from_rgb(0.55, 0.55, 0.58),
    };

    let status_bar = row![
        text(status_text).size(13).color(status_color),
        Space::with_width(Length::Fill),
        button(text("Rescan").size(12))
            .on_press(Message::ScanPrinters)
            .padding([4, 12]),
    ]
    .spacing(10)
    .padding([8, 12])
    .align_y(iced::Alignment::Center);

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
                        text(w).size(11).color(Color::from_rgb(0.8, 0.5, 0.0)),
                        button(text("x").size(10))
                            .on_press(Message::DismissWarning(i))
                            .padding(2),
                    ]
                    .spacing(5)
                    .into()
                })
                .collect::<Vec<_>>(),
        )
        .spacing(2)
        .padding([0, 12])
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
                    button(text(label).size(12))
                        .on_press(Message::SelectPrinter(i))
                        .padding(4)
                        .into()
                })
                .collect::<Vec<_>>(),
        )
        .spacing(2)
        .padding([0, 12])
        .into()
    } else {
        Space::new(0, 0).into()
    };

    // Editor label
    let editor_label = text("Markdown")
        .size(11)
        .color(Color::from_rgb(0.55, 0.55, 0.58));

    // Text editor (left panel)
    let editor = text_editor(&app.content)
        .on_action(Message::EditorAction)
        .height(Length::Fill)
        .font(Font::MONOSPACE)
        .size(13)
        .placeholder(
            "Type receipt markdown...\n\n# Heading\n**bold** _underline_\nItem | $10.00\n---",
        );

    let editor_panel = column![editor_label, editor]
        .spacing(4)
        .width(Length::FillPortion(2));

    // Preview label
    let preview_label = text("Preview")
        .size(11)
        .color(Color::from_rgb(0.55, 0.55, 0.58));

    // Preview panel (right panel) — renders wrapped lines as rich_text
    let preview_lines: Vec<Element<'_, Message>> = if app.wrapped_lines.is_empty() {
        vec![text("Receipt preview will appear here...")
            .size(11)
            .color(Color::from_rgb(0.7, 0.7, 0.7))
            .into()]
    } else {
        app.wrapped_lines.iter().map(build_preview_line).collect()
    };

    let preview_content = column(preview_lines)
        .spacing(1)
        .padding(10)
        .width(Length::Fill);

    let preview = scrollable(
        container(preview_content)
            .width(Length::Fill)
            .style(|_: &Theme| container::Style {
                background: Some(Color::WHITE.into()),
                border: iced::Border {
                    color: Color::from_rgb(0.85, 0.85, 0.87),
                    width: 1.0,
                    radius: 6.0.into(),
                },
                ..Default::default()
            }),
    )
    .height(Length::Fill);

    let preview_panel = column![preview_label, preview]
        .spacing(4)
        .width(Length::FillPortion(3));

    // Side-by-side layout
    let editor_preview = row![editor_panel, preview_panel]
        .spacing(12)
        .height(Length::Fill)
        .padding([0, 12]);

    // Print button + result
    let print_btn: Element<'_, Message> = if app.printing {
        button(text("Printing...").size(13)).padding([6, 20]).into()
    } else {
        let can_print = app.selected_printer.is_some() && !app.content.text().trim().is_empty();
        if can_print {
            button(text("Print").size(13))
                .on_press(Message::Print)
                .padding([6, 20])
                .into()
        } else {
            button(text("Print").size(13)).padding([6, 20]).into()
        }
    };

    let result_display: Element<'_, Message> = match &app.last_result {
        Some(Ok(msg)) => text(msg)
            .size(12)
            .color(Color::from_rgb(0.20, 0.78, 0.35))
            .into(),
        Some(Err(msg)) => text(format!("Error: {msg}"))
            .size(12)
            .color(Color::from_rgb(1.0, 0.23, 0.19))
            .into(),
        None => Space::new(0, 0).into(),
    };

    let bottom_bar = row![print_btn, Space::with_width(10), result_display]
        .spacing(10)
        .padding([8, 12])
        .align_y(iced::Alignment::Center);

    // Layout
    let content = column![
        status_bar,
        warnings_section,
        printer_selector,
        editor_preview,
        bottom_bar,
    ]
    .spacing(4);

    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding([8, 0])
        .into()
}

/// Build a single preview line from a WrappedLine, using rich_text spans.
fn build_preview_line<'a>(line: &WrappedLine) -> Element<'a, Message> {
    let spans: Vec<_> = line
        .spans
        .iter()
        .map(|s| {
            let mut sp = span(s.text.clone());

            let weight = if s.format.bold {
                font::Weight::Bold
            } else {
                font::Weight::Normal
            };

            sp = sp.font(Font {
                family: font::Family::Monospace,
                weight,
                ..Font::default()
            });

            if s.format.underline {
                sp = sp.underline(true);
            }

            if s.format.double_size {
                sp = sp.size(22);
            } else {
                sp = sp.size(11);
            }

            sp = sp.color(Color::from_rgb(0.1, 0.1, 0.1));

            sp
        })
        .collect();

    let rt = rich_text(spans)
        .font(Font::MONOSPACE)
        .wrapping(iced::widget::text::Wrapping::Word);

    match line.alignment {
        Alignment::Center => container(rt)
            .width(Length::Fill)
            .center_x(Length::Fill)
            .into(),
        Alignment::Right => container(rt)
            .width(Length::Fill)
            .align_x(iced::Alignment::End)
            .into(),
        Alignment::Left => container(rt).width(Length::Fill).into(),
    }
}

pub fn theme(_app: &App) -> Theme {
    Theme::custom(
        "Receipts Light".to_string(),
        iced::theme::Palette {
            background: Color::from_rgb(0.96, 0.96, 0.97),
            text: Color::from_rgb(0.11, 0.11, 0.12),
            primary: Color::from_rgb(0.0, 0.48, 1.0),
            success: Color::from_rgb(0.20, 0.78, 0.35),
            danger: Color::from_rgb(1.0, 0.23, 0.19),
        },
    )
}

pub fn subscription(_app: &App) -> Subscription<Message> {
    let hotplug = Subscription::run(hotplug_watcher);
    // Poll every 5 seconds — catches macOS USB disconnects that hotplug misses
    let health = time::every(std::time::Duration::from_secs(5)).map(|_| Message::HealthCheck);
    Subscription::batch([hotplug, health])
}

fn hotplug_watcher() -> impl futures::Stream<Item = Message> {
    iced::stream::channel(10, |mut output| async move {
        let watcher = match nusb::watch_devices() {
            Ok(w) => w,
            Err(e) => {
                tracing::warn!("Hotplug not available: {e}");
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
                    tracing::info!("USB disconnected: {id:?}");
                }
            }

            if output.send(Message::HotplugEvent).await.is_err() {
                break;
            }
        }
    })
}
