use escpos::driver::Driver;
use escpos::printer::Printer;
use escpos::utils::{JustifyMode, UnderlineMode};

use crate::receipt_markdown::{Alignment, ReceiptBlock};
use crate::word_wrap::{wrap_document, WrappedLine};

/// A pure, testable representation of an ESC/POS command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrintCommand {
    SetBold(bool),
    SetUnderline(bool),
    SetDoubleSize(bool),
    SetAlignment(Alignment),
    Write(String),
    Feed,
}

/// Generate a sequence of print commands from receipt blocks.
/// This is a pure function — no side effects, fully testable.
pub fn generate_commands(blocks: &[ReceiptBlock], max_chars: u8) -> Vec<PrintCommand> {
    let lines = wrap_document(blocks, max_chars);
    generate_commands_from_lines(&lines)
}

/// Generate print commands from pre-wrapped lines.
pub fn generate_commands_from_lines(lines: &[WrappedLine]) -> Vec<PrintCommand> {
    let mut commands = Vec::new();
    let mut current_alignment = Alignment::Left;

    for line in lines {
        // Set alignment if changed
        if line.alignment != current_alignment {
            commands.push(PrintCommand::SetAlignment(line.alignment));
            current_alignment = line.alignment;
        }

        // Track format state to avoid redundant commands
        let mut bold_on = false;
        let mut underline_on = false;
        let mut double_on = false;

        for span in &line.spans {
            // Only emit format changes when state actually changes
            if span.format.bold != bold_on {
                commands.push(PrintCommand::SetBold(span.format.bold));
                bold_on = span.format.bold;
            }
            if span.format.underline != underline_on {
                commands.push(PrintCommand::SetUnderline(span.format.underline));
                underline_on = span.format.underline;
            }
            if span.format.double_size != double_on {
                commands.push(PrintCommand::SetDoubleSize(span.format.double_size));
                double_on = span.format.double_size;
            }

            if !span.text.is_empty() {
                commands.push(PrintCommand::Write(span.text.clone()));
            }
        }

        // Reset any active formatting before line feed
        if bold_on {
            commands.push(PrintCommand::SetBold(false));
        }
        if underline_on {
            commands.push(PrintCommand::SetUnderline(false));
        }
        if double_on {
            commands.push(PrintCommand::SetDoubleSize(false));
        }

        commands.push(PrintCommand::Feed);
    }

    // Reset alignment at end
    if current_alignment != Alignment::Left {
        commands.push(PrintCommand::SetAlignment(Alignment::Left));
    }

    commands
}

/// Execute print commands against a real printer.
/// This is the imperative shell — the only function that touches hardware.
pub fn execute_commands<D: Driver>(
    printer: &mut Printer<D>,
    commands: &[PrintCommand],
) -> Result<(), String> {
    for cmd in commands {
        match cmd {
            PrintCommand::SetBold(on) => {
                printer.bold(*on).map_err(|e| e.to_string())?;
            }
            PrintCommand::SetUnderline(on) => {
                let mode = if *on {
                    UnderlineMode::Single
                } else {
                    UnderlineMode::None
                };
                printer.underline(mode).map_err(|e| e.to_string())?;
            }
            PrintCommand::SetDoubleSize(on) => {
                if *on {
                    printer.size(2, 2).map_err(|e| e.to_string())?;
                } else {
                    printer.size(1, 1).map_err(|e| e.to_string())?;
                }
            }
            PrintCommand::SetAlignment(align) => {
                let mode = match align {
                    Alignment::Left => JustifyMode::LEFT,
                    Alignment::Center => JustifyMode::CENTER,
                    Alignment::Right => JustifyMode::RIGHT,
                };
                printer.justify(mode).map_err(|e| e.to_string())?;
            }
            PrintCommand::Write(text) => {
                printer.write(text).map_err(|e| e.to_string())?;
            }
            PrintCommand::Feed => {
                printer.feed().map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::receipt_markdown::{parse_receipt_markdown, ReceiptSpan, SpanFormat};
    use crate::word_wrap::WrappedLine;

    #[test]
    fn bold_span_generates_correct_commands() {
        let lines = vec![WrappedLine {
            spans: vec![ReceiptSpan::bold("TOTAL")],
            alignment: Alignment::Left,
        }];
        let cmds = generate_commands_from_lines(&lines);

        assert_eq!(cmds[0], PrintCommand::SetBold(true));
        assert_eq!(cmds[1], PrintCommand::Write("TOTAL".into()));
        assert_eq!(cmds[2], PrintCommand::SetBold(false));
        assert_eq!(cmds[3], PrintCommand::Feed);
    }

    #[test]
    fn no_redundant_format_changes() {
        // Two adjacent bold spans — should only set bold once
        let lines = vec![WrappedLine {
            spans: vec![ReceiptSpan::bold("ACME"), ReceiptSpan::bold(" STORE")],
            alignment: Alignment::Left,
        }];
        let cmds = generate_commands_from_lines(&lines);

        let bold_count = cmds
            .iter()
            .filter(|c| matches!(c, PrintCommand::SetBold(true)))
            .count();
        assert_eq!(bold_count, 1, "Bold should only be set once");
    }

    #[test]
    fn alignment_changes_emitted() {
        let lines = vec![
            WrappedLine {
                spans: vec![ReceiptSpan::plain("left")],
                alignment: Alignment::Left,
            },
            WrappedLine {
                spans: vec![ReceiptSpan::plain("center")],
                alignment: Alignment::Center,
            },
        ];
        let cmds = generate_commands_from_lines(&lines);

        // Should have alignment change before "center" line
        assert!(cmds.contains(&PrintCommand::SetAlignment(Alignment::Center)));
        // Should NOT have explicit Left alignment at start (it's the default)
        assert_eq!(cmds[0], PrintCommand::Write("left".into()));
    }

    #[test]
    fn divider_generates_dashes() {
        let blocks = parse_receipt_markdown("---");
        let cmds = generate_commands(&blocks, 42);

        // Should contain a Write with 42 dashes
        let has_divider = cmds.iter().any(|c| match c {
            PrintCommand::Write(s) => s.len() == 42 && s.chars().all(|c| c == '-'),
            _ => false,
        });
        assert!(has_divider, "Should have 42-dash divider: {cmds:?}");
    }

    #[test]
    fn mixed_format_line() {
        let lines = vec![WrappedLine {
            spans: vec![
                ReceiptSpan::bold("Total"),
                ReceiptSpan::plain(" "),
                ReceiptSpan {
                    text: "$10.25".into(),
                    format: SpanFormat {
                        bold: true,
                        underline: true,
                        ..Default::default()
                    },
                },
            ],
            alignment: Alignment::Left,
        }];
        let cmds = generate_commands_from_lines(&lines);

        // Should have: SetBold(true), Write(Total), SetBold(false), Write( ),
        //              SetBold(true), SetUnderline(true), Write($10.25),
        //              SetBold(false), SetUnderline(false), Feed
        assert!(cmds.contains(&PrintCommand::SetUnderline(true)));
        assert!(cmds.contains(&PrintCommand::Write("$10.25".into())));
    }

    #[test]
    fn full_receipt_pipeline() {
        let input = "\
# RIVERSIDE CAFE

Espresso | $3.00
Croissant | $4.50

---

**Total** | **$8.25**";

        let blocks = parse_receipt_markdown(input);
        let cmds = generate_commands(&blocks, 42);

        // Should have commands for heading (double_size + centered)
        assert!(cmds.contains(&PrintCommand::SetDoubleSize(true)));
        assert!(cmds.contains(&PrintCommand::SetAlignment(Alignment::Center)));
        assert!(cmds.contains(&PrintCommand::Write("RIVERSIDE CAFE".into())));

        // Should have feeds for line breaks
        let feed_count = cmds
            .iter()
            .filter(|c| matches!(c, PrintCommand::Feed))
            .count();
        assert!(
            feed_count >= 7,
            "Should have feeds for each line: {feed_count}"
        );

        // Should have bold for total
        let bold_writes: Vec<_> = cmds
            .windows(2)
            .filter_map(|w| {
                if matches!(w[0], PrintCommand::SetBold(true)) {
                    if let PrintCommand::Write(ref s) = w[1] {
                        return Some(s.clone());
                    }
                }
                None
            })
            .collect();
        assert!(
            bold_writes.iter().any(|s| s.contains("Total")),
            "Total should be bold: {bold_writes:?}"
        );
    }

    #[test]
    fn alignment_reset_at_end() {
        let lines = vec![WrappedLine {
            spans: vec![ReceiptSpan::plain("centered text")],
            alignment: Alignment::Center,
        }];
        let cmds = generate_commands_from_lines(&lines);

        // Last command should reset alignment to Left
        assert_eq!(
            cmds.last().unwrap(),
            &PrintCommand::SetAlignment(Alignment::Left)
        );
    }
}
