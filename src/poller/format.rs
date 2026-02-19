use crate::receipt_markdown::{Alignment, ReceiptBlock, ReceiptSpan};

use super::types::ReceiptMessage;

/// Format a website message into receipt blocks for printing.
pub fn format_message(msg: &ReceiptMessage) -> Vec<ReceiptBlock> {
    let mut blocks = vec![
        ReceiptBlock::Divider,
        ReceiptBlock::Heading {
            spans: vec![ReceiptSpan::heading("MESSAGE")],
        },
        ReceiptBlock::Divider,
        ReceiptBlock::BlankLine,
    ];

    // Sender info
    let sender = msg
        .sender_name
        .as_deref()
        .unwrap_or_else(|| msg.sender_ip.as_deref().unwrap_or("anonymous"));
    blocks.push(ReceiptBlock::Line {
        spans: vec![ReceiptSpan::bold("From: "), ReceiptSpan::plain(sender)],
        alignment: Alignment::Left,
    });

    // Time — take first 16 chars of ISO string, replace T with space
    let time_display = format_time(&msg.created_at);
    blocks.push(ReceiptBlock::Line {
        spans: vec![
            ReceiptSpan::bold("Time: "),
            ReceiptSpan::plain(&time_display),
        ],
        alignment: Alignment::Left,
    });

    blocks.push(ReceiptBlock::BlankLine);
    blocks.push(ReceiptBlock::Divider);
    blocks.push(ReceiptBlock::BlankLine);

    // Message content — split into lines, each becomes a ReceiptBlock::Line
    for line in msg.content.lines() {
        if line.trim().is_empty() {
            blocks.push(ReceiptBlock::BlankLine);
        } else {
            blocks.push(ReceiptBlock::Line {
                spans: vec![ReceiptSpan::plain(line)],
                alignment: Alignment::Left,
            });
        }
    }

    blocks.push(ReceiptBlock::BlankLine);
    blocks.push(ReceiptBlock::Divider);

    blocks
}

fn format_time(iso: &str) -> String {
    // "2025-02-19T14:30:00Z" → "2025-02-19 14:30"
    let truncated = if iso.len() >= 16 { &iso[..16] } else { iso };
    truncated.replace('T', " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_message() -> ReceiptMessage {
        ReceiptMessage {
            id: 1,
            content: "Hello from the web!\nThis is line two.".to_string(),
            sender_name: Some("Bob".to_string()),
            sender_ip: Some("192.168.1.5".to_string()),
            image_url: None,
            status: "pending".to_string(),
            created_at: "2025-02-19T14:30:00Z".to_string(),
        }
    }

    #[test]
    fn format_produces_correct_structure() {
        let msg = sample_message();
        let blocks = format_message(&msg);

        // Should start with divider, heading, divider
        assert!(matches!(blocks[0], ReceiptBlock::Divider));
        assert!(matches!(blocks[1], ReceiptBlock::Heading { .. }));
        assert!(matches!(blocks[2], ReceiptBlock::Divider));
        assert!(matches!(blocks[3], ReceiptBlock::BlankLine));

        // Should have From: and Time: lines
        if let ReceiptBlock::Line { spans, .. } = &blocks[4] {
            assert_eq!(spans[0].text, "From: ");
            assert!(spans[0].format.bold);
            assert_eq!(spans[1].text, "Bob");
        } else {
            panic!("Expected Line block for sender");
        }

        if let ReceiptBlock::Line { spans, .. } = &blocks[5] {
            assert_eq!(spans[0].text, "Time: ");
            assert_eq!(spans[1].text, "2025-02-19 14:30");
        } else {
            panic!("Expected Line block for time");
        }

        // Should end with divider
        assert!(matches!(blocks.last(), Some(ReceiptBlock::Divider)));
    }

    #[test]
    fn format_multiline_content() {
        let msg = sample_message();
        let blocks = format_message(&msg);

        // Content should appear as separate Line blocks
        let content_blocks: Vec<_> = blocks
            .iter()
            .filter(|b| {
                if let ReceiptBlock::Line { spans, .. } = b {
                    spans.iter().any(|s| s.text.contains("Hello"))
                        || spans.iter().any(|s| s.text.contains("line two"))
                } else {
                    false
                }
            })
            .collect();

        assert_eq!(content_blocks.len(), 2);
    }

    #[test]
    fn format_anonymous_sender() {
        let mut msg = sample_message();
        msg.sender_name = None;
        msg.sender_ip = None;
        let blocks = format_message(&msg);

        if let ReceiptBlock::Line { spans, .. } = &blocks[4] {
            assert_eq!(spans[1].text, "anonymous");
        } else {
            panic!("Expected Line block for sender");
        }
    }

    #[test]
    fn format_time_parsing() {
        assert_eq!(format_time("2025-02-19T14:30:00Z"), "2025-02-19 14:30");
        assert_eq!(format_time("2025-02-19T14:30:00.000Z"), "2025-02-19 14:30");
        assert_eq!(format_time("short"), "short");
    }
}
