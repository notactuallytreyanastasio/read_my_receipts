use iced::keyboard::{self, key};
use iced::widget::text_editor::{Binding, KeyPress, Motion};
use iced::widget::{
    button, column, container, image as iced_image, rich_text, row, scrollable, span, text,
    text_editor, Space,
};
use iced::{font, time, Color, Element, Font, Length, Subscription, Task, Theme};

use crate::poller::{self, PollEvent, PollerConfig, ReceiptMessage};
use crate::printer::discovery::{self, DiscoveredPrinter};
use crate::printer::models::{find_known_model, EPSON_VENDOR_ID};
use crate::receipt_markdown::{Alignment, ReceiptBlock};
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PollerStatus {
    Disabled,
    Connecting,
    Polling,
    Error(String),
}

#[derive(Debug, Clone)]
pub struct ReceivedMessage {
    pub id: i64,
    pub sender: String,
    pub content_preview: String,
    pub content_full: String,
    pub time: String,
    pub image_bytes: Option<Vec<u8>>,
    pub status: MessagePrintStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessagePrintStatus {
    Printing,
    Printed,
    Failed(String),
}

#[derive(Debug, Clone)]
struct QueuedPrint {
    message_id: i64,
    blocks: Vec<ReceiptBlock>,
    image_bytes: Option<Vec<u8>>,
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
    show_messages_panel: bool,
    // Poller state
    poller_config: Option<PollerConfig>,
    poller_enabled: bool,
    poller_status: PollerStatus,
    received_messages: Vec<ReceivedMessage>,
    print_queue: Vec<QueuedPrint>,
    messages_printed_count: u32,
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
    // Poller messages
    PollEvent(PollEvent),
    TogglePoller,
    PrintMessageResult {
        message_id: i64,
        result: Result<(), String>,
    },
    MarkResult(Result<(), String>),
    ImageDownloaded {
        message_id: i64,
        result: Result<Vec<u8>, String>,
    },
    ToggleMessagesPanel,
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
    app.parsed_blocks = crate::receipt_markdown::parse_receipt_markdown(&input);
    let max_chars = current_max_chars(app);
    app.wrapped_lines = wrap_document(&app.parsed_blocks, max_chars);
}

impl Default for App {
    fn default() -> Self {
        let poller_config = poller::config::load_config().ok();
        let poller_enabled = poller_config.is_some();
        let poller_status = if poller_config.is_some() {
            PollerStatus::Connecting
        } else {
            PollerStatus::Disabled
        };

        if poller_config.is_some() {
            tracing::info!("Poller config loaded from .hermes_env");
        } else {
            tracing::info!("No .hermes_env found — poller disabled");
        }

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
            show_messages_panel: false,
            poller_config,
            poller_enabled,
            poller_status,
            received_messages: Vec::new(),
            print_queue: Vec::new(),
            messages_printed_count: 0,
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
            // Check print queue for pending polled messages
            try_print_next_queued(app)
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

        Message::HealthCheck => Task::perform(
            async { discovery::scan_for_printers() },
            Message::PrintersFound,
        ),

        // --- Poller messages ---
        Message::PollEvent(event) => match event {
            PollEvent::Connected => {
                app.poller_status = PollerStatus::Polling;
                tracing::info!("Poller connected");
                Task::none()
            }
            PollEvent::Error(e) => {
                app.poller_status = PollerStatus::Error(e.clone());
                tracing::warn!("Poll error: {e}");
                Task::none()
            }
            PollEvent::MessagesReceived(messages) => {
                app.poller_status = PollerStatus::Polling;
                handle_received_messages(app, messages)
            }
        },

        Message::TogglePoller => {
            app.poller_enabled = !app.poller_enabled;
            if app.poller_enabled {
                app.poller_status = PollerStatus::Connecting;
            } else {
                app.poller_status = PollerStatus::Disabled;
            }
            Task::none()
        }

        Message::PrintMessageResult { message_id, result } => {
            app.printing = false;

            // Update received message status
            if let Some(rm) = app
                .received_messages
                .iter_mut()
                .find(|m| m.id == message_id)
            {
                match &result {
                    Ok(()) => {
                        rm.status = MessagePrintStatus::Printed;
                        app.messages_printed_count += 1;
                    }
                    Err(e) => {
                        rm.status = MessagePrintStatus::Failed(e.clone());
                    }
                }
            }

            // Fire-and-forget: mark on the blog API
            let mark_task = if let Some(config) = app.poller_config.clone() {
                let is_ok = result.is_ok();
                Task::perform(
                    async move {
                        let client = reqwest::Client::new();
                        if is_ok {
                            poller::client::mark_printed(&client, &config, message_id).await
                        } else {
                            poller::client::mark_failed(&client, &config, message_id).await
                        }
                    },
                    Message::MarkResult,
                )
            } else {
                Task::none()
            };

            // Try to print next queued message
            let next_task = try_print_next_queued(app);

            Task::batch([mark_task, next_task])
        }

        Message::MarkResult(result) => {
            if let Err(e) = result {
                tracing::warn!("Failed to update message status on API: {e}");
            }
            Task::none()
        }

        Message::ImageDownloaded { message_id, result } => {
            match result {
                Ok(bytes) => {
                    tracing::info!(
                        "Downloaded image for message {}: {} bytes",
                        message_id,
                        bytes.len()
                    );
                    // Update the display entry
                    if let Some(rm) = app
                        .received_messages
                        .iter_mut()
                        .find(|m| m.id == message_id)
                    {
                        rm.image_bytes = Some(bytes.clone());
                    }
                    // Update the queued print job
                    if let Some(job) = app
                        .print_queue
                        .iter_mut()
                        .find(|j| j.message_id == message_id)
                    {
                        job.image_bytes = Some(bytes);
                    }
                }
                Err(e) => {
                    tracing::warn!("Image download failed for message {}: {e}", message_id);
                }
            }
            // Start printing if not busy
            if !app.printing {
                try_print_next_queued(app)
            } else {
                Task::none()
            }
        }

        Message::ToggleMessagesPanel => {
            app.show_messages_panel = !app.show_messages_panel;
            Task::none()
        }
    }
}

/// Handle a batch of received messages: add to display list, start image downloads.
fn handle_received_messages(app: &mut App, messages: Vec<ReceiptMessage>) -> Task<Message> {
    let mut download_tasks: Vec<Task<Message>> = Vec::new();

    for msg in messages {
        // Skip duplicates — same message can arrive before mark_printed completes
        if app.received_messages.iter().any(|rm| rm.id == msg.id) {
            tracing::debug!("Skipping duplicate message id={}", msg.id);
            continue;
        }

        let sender = msg
            .sender_name
            .as_deref()
            .or(msg.sender_ip.as_deref())
            .unwrap_or("anonymous")
            .to_string();

        let preview = if msg.content.len() > 50 {
            format!("{}...", &msg.content[..47])
        } else {
            msg.content.clone()
        };

        let time = format_time_short(&msg.created_at);
        let has_image = msg.image_url.is_some();

        // Format text blocks
        let blocks = poller::format::format_message(&msg);

        app.received_messages.push(ReceivedMessage {
            id: msg.id,
            sender,
            content_preview: preview,
            content_full: msg.content.clone(),
            time,
            image_bytes: None, // filled in when download completes
            status: MessagePrintStatus::Printing,
        });
        if app.received_messages.len() > 50 {
            app.received_messages.remove(0);
        }

        // Queue print job (image_bytes filled later if needed)
        app.print_queue.push(QueuedPrint {
            message_id: msg.id,
            blocks,
            image_bytes: None,
        });

        // Start image download if URL present
        if let (Some(image_url), Some(config)) = (msg.image_url.clone(), app.poller_config.clone())
        {
            let message_id = msg.id;
            download_tasks.push(Task::perform(
                async move {
                    let client = reqwest::Client::new();
                    poller::client::download_image(&client, &config, &image_url).await
                },
                move |result| Message::ImageDownloaded { message_id, result },
            ));
        }

        // If no image, start printing immediately
        if !has_image && !app.printing {
            let task = try_print_next_queued(app);
            download_tasks.push(task);
        }
    }

    // If there are only image messages and none are printing yet, downloads will trigger printing
    Task::batch(download_tasks)
}

/// Pop the next queued print job and start it.
fn try_print_next_queued(app: &mut App) -> Task<Message> {
    if app.printing {
        return Task::none();
    }

    let Some(job) = app.print_queue.first().cloned() else {
        return Task::none();
    };
    app.print_queue.remove(0);

    let Some(idx) = app.selected_printer else {
        // No printer — mark as failed
        if let Some(rm) = app
            .received_messages
            .iter_mut()
            .find(|m| m.id == job.message_id)
        {
            rm.status = MessagePrintStatus::Failed("No printer".into());
        }
        return Task::none();
    };
    let Some(printer_info) = app.discovered.get(idx).cloned() else {
        return Task::none();
    };

    app.printing = true;
    let max_chars = current_max_chars(app);
    let message_id = job.message_id;
    let blocks = job.blocks;
    let image_bytes = job.image_bytes;

    Task::perform(
        async move {
            let mut conn = crate::printer::connection::PrinterConnection::open(
                printer_info.product_id,
                printer_info.model_name.clone(),
            )?;
            conn.print_website_message(&blocks, max_chars, image_bytes.as_deref())
        },
        move |result| Message::PrintMessageResult { message_id, result },
    )
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
        .key_binding(macos_key_binding)
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

    // Messages section (poller status + recent messages)
    let messages_section: Element<'_, Message> = if app.poller_config.is_some() {
        build_messages_section(app)
    } else {
        Space::new(0, 0).into()
    };

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
        messages_section,
        bottom_bar,
    ]
    .spacing(4);

    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding([8, 0])
        .into()
}

fn build_messages_section(app: &App) -> Element<'_, Message> {
    let (poller_text, poller_color) = match &app.poller_status {
        PollerStatus::Polling => (
            "Polling bobbby.online".to_string(),
            Color::from_rgb(0.20, 0.78, 0.35),
        ),
        PollerStatus::Connecting => (
            "Connecting...".to_string(),
            Color::from_rgb(0.55, 0.55, 0.58),
        ),
        PollerStatus::Error(e) => {
            let display = if e.len() > 40 {
                format!("Poll error: {}...", &e[..37])
            } else {
                format!("Poll error: {e}")
            };
            (display, Color::from_rgb(1.0, 0.23, 0.19))
        }
        PollerStatus::Disabled => (
            "Polling paused".to_string(),
            Color::from_rgb(0.55, 0.55, 0.58),
        ),
    };

    let toggle_label = if app.poller_enabled {
        "Pause"
    } else {
        "Resume"
    };

    let queue_count = app.print_queue.len();
    let queue_text = if queue_count > 0 {
        format!("{queue_count} queued")
    } else {
        String::new()
    };

    let panel_label = if app.show_messages_panel {
        "Hide Messages"
    } else {
        "Messages"
    };

    let msg_count = app.received_messages.len();
    let panel_button_label = if msg_count > 0 && !app.show_messages_panel {
        format!("{panel_label} ({msg_count})")
    } else {
        panel_label.to_string()
    };

    let header = row![
        text(poller_text).size(11).color(poller_color),
        Space::with_width(Length::Fill),
        text(queue_text)
            .size(11)
            .color(Color::from_rgb(0.85, 0.55, 0.0)),
        text(format!("{} printed", app.messages_printed_count))
            .size(11)
            .color(Color::from_rgb(0.55, 0.55, 0.58)),
        button(text(panel_button_label).size(11))
            .on_press(Message::ToggleMessagesPanel)
            .padding([3, 10]),
        button(text(toggle_label).size(11))
            .on_press(Message::TogglePoller)
            .padding([3, 10]),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center);

    let mut items: Vec<Element<'_, Message>> = vec![header.into()];

    // Expanded messages panel
    if app.show_messages_panel {
        let panel = build_messages_panel(app);
        items.push(panel);
    }

    container(column(items).spacing(4))
        .padding([6, 12])
        .width(Length::Fill)
        .style(|_: &Theme| container::Style {
            background: Some(Color::from_rgb(0.93, 0.93, 0.95).into()),
            border: iced::Border {
                color: Color::from_rgb(0.85, 0.85, 0.87),
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        })
        .into()
}

fn build_messages_panel(app: &App) -> Element<'_, Message> {
    let gray = Color::from_rgb(0.55, 0.55, 0.58);
    let light_gray = Color::from_rgb(0.7, 0.7, 0.7);

    if app.received_messages.is_empty() {
        return text("No messages received yet")
            .size(11)
            .color(light_gray)
            .into();
    }

    let mut rows: Vec<Element<'_, Message>> = Vec::new();

    // Header row
    rows.push(
        row![
            text("Status")
                .size(10)
                .color(gray)
                .width(Length::Fixed(40.0)),
            text("Time").size(10).color(gray).width(Length::Fixed(45.0)),
            text("From")
                .size(10)
                .color(gray)
                .width(Length::Fixed(120.0)),
            text("Message").size(10).color(gray).width(Length::Fill),
        ]
        .spacing(6)
        .padding([2, 4])
        .into(),
    );

    rows.push(
        container(Space::new(Length::Fill, 1))
            .style(|_: &Theme| container::Style {
                background: Some(Color::from_rgb(0.82, 0.82, 0.84).into()),
                ..Default::default()
            })
            .into(),
    );

    // Show last 10 messages, most recent first
    for msg in app.received_messages.iter().rev().take(10) {
        let (status_text, status_color) = match &msg.status {
            MessagePrintStatus::Printed => ("OK", Color::from_rgb(0.20, 0.78, 0.35)),
            MessagePrintStatus::Printing => ("..", Color::from_rgb(0.55, 0.55, 0.58)),
            MessagePrintStatus::Failed(e) => {
                let _ = e;
                ("FAIL", Color::from_rgb(1.0, 0.23, 0.19))
            }
        };

        // Truncate sender for display
        let sender_display = if msg.sender.len() > 16 {
            format!("{}...", &msg.sender[..13])
        } else {
            msg.sender.clone()
        };

        // Message row with aligned columns
        let info_row = row![
            text(status_text)
                .size(10)
                .color(status_color)
                .width(Length::Fixed(40.0)),
            text(&msg.time)
                .size(10)
                .color(gray)
                .width(Length::Fixed(45.0)),
            text(sender_display).size(10).width(Length::Fixed(120.0)),
            text(&msg.content_preview)
                .size(10)
                .color(Color::from_rgb(0.2, 0.2, 0.2))
                .width(Length::Fill),
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center);

        // Build row: text info + thumbnail (if image)
        let msg_element: Element<'_, Message> = if let Some(bytes) = &msg.image_bytes {
            let handle = iced_image::Handle::from_bytes(bytes.clone());
            let thumbnail = iced_image(handle)
                .height(80)
                .content_fit(iced::ContentFit::ScaleDown);
            let content = column![info_row, thumbnail].spacing(4);
            content.into()
        } else {
            info_row.into()
        };

        // Wrap in a container
        let msg_container = container(msg_element)
            .padding([4, 4])
            .width(Length::Fill)
            .style(|_: &Theme| container::Style {
                border: iced::Border {
                    color: Color::from_rgb(0.88, 0.88, 0.90),
                    width: 0.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            });

        rows.push(msg_container.into());

        // Thin separator between messages
        rows.push(
            container(Space::new(Length::Fill, 1))
                .style(|_: &Theme| container::Style {
                    background: Some(Color::from_rgb(0.88, 0.88, 0.90).into()),
                    ..Default::default()
                })
                .into(),
        );
    }

    // Remove trailing separator
    if !rows.is_empty() {
        rows.pop();
    }

    let list = column(rows).spacing(0);

    scrollable(list).height(Length::Fixed(300.0)).into()
}

/// Format ISO timestamp to short display: "14:30" or "Feb 19 14:30"
fn format_time_short(iso: &str) -> String {
    if iso.len() >= 16 {
        // "2025-02-19T14:30:00Z" → "14:30"
        iso[11..16].to_string()
    } else {
        iso.to_string()
    }
}

/// Custom key bindings for macOS text editing shortcuts.
///
/// On macOS, Option+Arrow produces dead-key text characters, which causes
/// iced's default handler to insert garbage instead of doing word navigation.
/// This function intercepts those combos and produces the correct bindings.
fn macos_key_binding(key_press: KeyPress) -> Option<Binding<Message>> {
    let KeyPress {
        key,
        modifiers,
        status,
        ..
    } = &key_press;

    if *status != text_editor::Status::Focused {
        return None;
    }

    // Only intercept when Option or Cmd is held with a named key
    match key.as_ref() {
        keyboard::Key::Named(named) => {
            let shift = modifiers.shift();

            // Cmd+Arrow: line/document navigation
            if modifiers.macos_command() {
                let motion = match named {
                    key::Named::ArrowLeft => Some(Motion::Home),
                    key::Named::ArrowRight => Some(Motion::End),
                    key::Named::ArrowUp => Some(Motion::DocumentStart),
                    key::Named::ArrowDown => Some(Motion::DocumentEnd),
                    _ => None,
                };
                if let Some(m) = motion {
                    return Some(if shift {
                        Binding::Select(m)
                    } else {
                        Binding::Move(m)
                    });
                }
            }

            // Option+Arrow: word navigation
            if modifiers.alt() {
                let motion = match named {
                    key::Named::ArrowLeft => Some(Motion::WordLeft),
                    key::Named::ArrowRight => Some(Motion::WordRight),
                    _ => None,
                };
                if let Some(m) = motion {
                    return Some(if shift {
                        Binding::Select(m)
                    } else {
                        Binding::Move(m)
                    });
                }
            }

            // Option+Backspace: delete word backward
            if modifiers.alt() && matches!(named, key::Named::Backspace) {
                return Some(Binding::Sequence(vec![
                    Binding::Select(Motion::WordLeft),
                    Binding::Backspace,
                ]));
            }

            // Cmd+Backspace: delete to start of line
            if modifiers.macos_command() && matches!(named, key::Named::Backspace) {
                return Some(Binding::Sequence(vec![
                    Binding::Select(Motion::Home),
                    Binding::Backspace,
                ]));
            }

            // Option+Delete: delete word forward
            if modifiers.alt() && matches!(named, key::Named::Delete) {
                return Some(Binding::Sequence(vec![
                    Binding::Select(Motion::WordRight),
                    Binding::Delete,
                ]));
            }

            // Fall through to default
            Binding::from_key_press(key_press)
        }
        // For character keys (Cmd+A, Cmd+C, etc.) and plain typing, use default
        _ => Binding::from_key_press(key_press),
    }
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

pub fn subscription(app: &App) -> Subscription<Message> {
    let hotplug = Subscription::run(hotplug_watcher);
    let health = time::every(std::time::Duration::from_secs(5)).map(|_| Message::HealthCheck);

    let mut subs = vec![hotplug, health];

    if app.poller_enabled {
        if let Some(config) = app.poller_config.clone() {
            subs.push(
                Subscription::run_with_id(
                    "website-poller",
                    poller::subscription::poll_watcher(config),
                )
                .map(Message::PollEvent),
            );
        }
    }

    Subscription::batch(subs)
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
