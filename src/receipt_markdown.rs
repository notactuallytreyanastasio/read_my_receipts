use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

/// Formatting state for a span of receipt text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpanFormat {
    pub bold: bool,
    pub underline: bool,
    pub double_size: bool,
}

/// A single styled run of text (no newlines).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiptSpan {
    pub text: String,
    pub format: SpanFormat,
}

impl ReceiptSpan {
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            format: SpanFormat::default(),
        }
    }

    pub fn bold(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            format: SpanFormat {
                bold: true,
                ..Default::default()
            },
        }
    }

    pub fn underlined(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            format: SpanFormat {
                underline: true,
                ..Default::default()
            },
        }
    }

    pub fn heading(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            format: SpanFormat {
                bold: true,
                double_size: true,
                ..Default::default()
            },
        }
    }
}

/// Alignment for a line or block.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Alignment {
    #[default]
    Left,
    Center,
    Right,
}

/// A parsed block ready for word-wrapping and printing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReceiptBlock {
    /// A line of styled spans with alignment.
    Line {
        spans: Vec<ReceiptSpan>,
        alignment: Alignment,
    },
    /// A heading (maps to double-size + centered text).
    Heading { spans: Vec<ReceiptSpan> },
    /// A horizontal divider (from `---`).
    Divider,
    /// A columnar row from pipe syntax: `Item | $10`.
    Columns { cells: Vec<Vec<ReceiptSpan>> },
    /// A blank line.
    BlankLine,
}

/// Parse receipt markdown into blocks.
///
/// Supports standard markdown (bold, underline/emphasis, headings, dividers)
/// and ReceiptLine pipe syntax for columns.
pub fn parse_receipt_markdown(input: &str) -> Vec<ReceiptBlock> {
    let mut blocks = Vec::new();
    let mut markdown_buf = String::new();

    for line in input.lines() {
        let trimmed = line.trim();

        if trimmed.is_empty() {
            // Flush any accumulated markdown first
            flush_markdown(&mut markdown_buf, &mut blocks);
            blocks.push(ReceiptBlock::BlankLine);
            continue;
        }

        if is_column_line(trimmed) {
            flush_markdown(&mut markdown_buf, &mut blocks);
            blocks.push(parse_column_line(trimmed));
            continue;
        }

        // Accumulate for pulldown-cmark
        if !markdown_buf.is_empty() {
            markdown_buf.push('\n');
        }
        markdown_buf.push_str(line);
    }

    flush_markdown(&mut markdown_buf, &mut blocks);
    blocks
}

/// Check if a line is a pipe-delimited column (ReceiptLine syntax).
/// Must contain `|` but not be a markdown table header (starting/ending with |).
fn is_column_line(line: &str) -> bool {
    if !line.contains('|') {
        return false;
    }
    // Markdown tables start and end with |, pipe columns don't
    if line.starts_with('|') && line.ends_with('|') {
        return false;
    }
    // Don't treat headings or code fences as columns
    if line.starts_with('#') || line.starts_with("```") {
        return false;
    }
    true
}

/// Parse a pipe-delimited column line into a Columns block.
fn parse_column_line(line: &str) -> ReceiptBlock {
    let cells: Vec<Vec<ReceiptSpan>> = line
        .split('|')
        .map(|cell| parse_inline(cell.trim()))
        .collect();
    ReceiptBlock::Columns { cells }
}

/// Parse inline markdown formatting (bold, underline) within a text fragment.
/// This is a simple scanner — no block-level elements.
pub fn parse_inline(input: &str) -> Vec<ReceiptSpan> {
    let mut spans = Vec::new();
    let mut pos = 0;
    let bytes = input.as_bytes();

    while pos < bytes.len() {
        if pos + 1 < bytes.len() && bytes[pos] == b'*' && bytes[pos + 1] == b'*' {
            // Bold: **text**
            if let Some(end) = find_closing(input, pos + 2, "**") {
                let text = &input[pos + 2..end];
                if !text.is_empty() {
                    spans.push(ReceiptSpan::bold(text));
                }
                pos = end + 2;
                continue;
            }
        }

        if pos + 1 < bytes.len() && bytes[pos] == b'_' && bytes[pos + 1] == b'_' {
            // Bold (alt): __text__
            if let Some(end) = find_closing(input, pos + 2, "__") {
                let text = &input[pos + 2..end];
                if !text.is_empty() {
                    spans.push(ReceiptSpan::bold(text));
                }
                pos = end + 2;
                continue;
            }
        }

        if bytes[pos] == b'_' && (pos + 1 < bytes.len()) && bytes[pos + 1] != b'_' {
            // Underline: _text_
            if let Some(end) = find_closing(input, pos + 1, "_") {
                let text = &input[pos + 1..end];
                if !text.is_empty() {
                    spans.push(ReceiptSpan::underlined(text));
                }
                pos = end + 1;
                continue;
            }
        }

        if bytes[pos] == b'*' && (pos + 1 < bytes.len()) && bytes[pos + 1] != b'*' {
            // Underline (alt): *text*
            if let Some(end) = find_closing(input, pos + 1, "*") {
                let text = &input[pos + 1..end];
                if !text.is_empty() {
                    spans.push(ReceiptSpan::underlined(text));
                }
                pos = end + 1;
                continue;
            }
        }

        // Plain text — collect until next marker
        let start = pos;
        while pos < bytes.len() && bytes[pos] != b'*' && bytes[pos] != b'_' {
            pos += 1;
        }
        let text = &input[start..pos];
        if !text.is_empty() {
            spans.push(ReceiptSpan::plain(text));
        }
    }

    if spans.is_empty() && !input.is_empty() {
        spans.push(ReceiptSpan::plain(input));
    }

    spans
}

/// Find the position of a closing delimiter in the string.
fn find_closing(input: &str, start: usize, delimiter: &str) -> Option<usize> {
    input[start..].find(delimiter).map(|i| i + start)
}

/// Flush accumulated markdown text through pulldown-cmark and append blocks.
fn flush_markdown(buf: &mut String, blocks: &mut Vec<ReceiptBlock>) {
    if buf.is_empty() {
        return;
    }

    let options = Options::ENABLE_STRIKETHROUGH;
    let parser = Parser::new_ext(buf, options);

    let mut spans: Vec<ReceiptSpan> = Vec::new();
    let mut bold = false;
    let mut emphasis = false;
    let mut in_heading = false;

    for event in parser {
        match event {
            Event::Start(Tag::Strong) => bold = true,
            Event::End(TagEnd::Strong) => bold = false,
            Event::Start(Tag::Emphasis) => emphasis = true,
            Event::End(TagEnd::Emphasis) => emphasis = false,
            Event::Start(Tag::Heading { .. }) => {
                in_heading = true;
            }
            Event::End(TagEnd::Heading(_)) => {
                in_heading = false;
                if !spans.is_empty() {
                    blocks.push(ReceiptBlock::Heading {
                        spans: std::mem::take(&mut spans),
                    });
                }
            }
            Event::End(TagEnd::Paragraph) => {
                if !spans.is_empty() {
                    blocks.push(ReceiptBlock::Line {
                        spans: std::mem::take(&mut spans),
                        alignment: Alignment::Left,
                    });
                }
            }
            Event::Text(text) => {
                let format = SpanFormat {
                    bold: bold || in_heading,
                    underline: emphasis,
                    double_size: in_heading,
                };
                spans.push(ReceiptSpan {
                    text: text.to_string(),
                    format,
                });
            }
            Event::SoftBreak => {
                spans.push(ReceiptSpan::plain(" "));
            }
            Event::HardBreak => {
                if !spans.is_empty() {
                    blocks.push(ReceiptBlock::Line {
                        spans: std::mem::take(&mut spans),
                        alignment: Alignment::Left,
                    });
                }
            }
            Event::Rule => {
                blocks.push(ReceiptBlock::Divider);
            }
            _ => {}
        }
    }

    // Flush any remaining spans
    if !spans.is_empty() {
        blocks.push(ReceiptBlock::Line {
            spans,
            alignment: Alignment::Left,
        });
    }

    buf.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Real receipt content tests ---

    #[test]
    fn parse_full_receipt() {
        let input = "# ACME COFFEE SHOP\n\nAmericano | $4.50\nOat Latte | $5.75\n\n---\n\n**Total** | **$10.25**";
        let blocks = parse_receipt_markdown(input);

        assert!(matches!(blocks[0], ReceiptBlock::Heading { .. }));
        assert!(matches!(blocks[1], ReceiptBlock::BlankLine));
        assert!(matches!(blocks[2], ReceiptBlock::Columns { .. }));
        assert!(matches!(blocks[3], ReceiptBlock::Columns { .. }));
        assert!(matches!(blocks[4], ReceiptBlock::BlankLine));
        assert!(matches!(blocks[5], ReceiptBlock::Divider));
        assert!(matches!(blocks[6], ReceiptBlock::BlankLine));
        assert!(matches!(blocks[7], ReceiptBlock::Columns { .. }));
    }

    #[test]
    fn parse_bold_store_name() {
        let input = "**ACME STORE**";
        let blocks = parse_receipt_markdown(input);

        assert_eq!(blocks.len(), 1);
        if let ReceiptBlock::Line { spans, .. } = &blocks[0] {
            assert_eq!(spans.len(), 1);
            assert_eq!(spans[0].text, "ACME STORE");
            assert!(spans[0].format.bold);
            assert!(!spans[0].format.underline);
        } else {
            panic!("Expected Line block, got {blocks:?}");
        }
    }

    #[test]
    fn parse_underline_thank_you() {
        let input = "_Thank you for your purchase!_";
        let blocks = parse_receipt_markdown(input);

        assert_eq!(blocks.len(), 1);
        if let ReceiptBlock::Line { spans, .. } = &blocks[0] {
            assert_eq!(spans.len(), 1);
            assert_eq!(spans[0].text, "Thank you for your purchase!");
            assert!(spans[0].format.underline);
            assert!(!spans[0].format.bold);
        } else {
            panic!("Expected Line block, got {blocks:?}");
        }
    }

    #[test]
    fn parse_mixed_inline_formatting() {
        let input = "**bold** and _underline_";
        let blocks = parse_receipt_markdown(input);

        assert_eq!(blocks.len(), 1);
        if let ReceiptBlock::Line { spans, .. } = &blocks[0] {
            assert_eq!(spans.len(), 3);
            assert_eq!(spans[0].text, "bold");
            assert!(spans[0].format.bold);
            assert_eq!(spans[1].text, " and ");
            assert!(!spans[1].format.bold);
            assert!(!spans[1].format.underline);
            assert_eq!(spans[2].text, "underline");
            assert!(spans[2].format.underline);
        } else {
            panic!("Expected Line block, got {blocks:?}");
        }
    }

    #[test]
    fn parse_heading_double_size_centered() {
        let input = "# ACME STORE";
        let blocks = parse_receipt_markdown(input);

        assert_eq!(blocks.len(), 1);
        if let ReceiptBlock::Heading { spans } = &blocks[0] {
            assert_eq!(spans.len(), 1);
            assert_eq!(spans[0].text, "ACME STORE");
            assert!(spans[0].format.bold);
            assert!(spans[0].format.double_size);
        } else {
            panic!("Expected Heading block, got {blocks:?}");
        }
    }

    #[test]
    fn parse_divider() {
        let input = "---";
        let blocks = parse_receipt_markdown(input);

        assert_eq!(blocks.len(), 1);
        assert!(matches!(blocks[0], ReceiptBlock::Divider));
    }

    #[test]
    fn parse_pipe_columns_coffee_receipt() {
        let input = "Coffee | $4.50";
        let blocks = parse_receipt_markdown(input);

        assert_eq!(blocks.len(), 1);
        if let ReceiptBlock::Columns { cells } = &blocks[0] {
            assert_eq!(cells.len(), 2);
            assert_eq!(cells[0][0].text, "Coffee");
            assert_eq!(cells[1][0].text, "$4.50");
        } else {
            panic!("Expected Columns block, got {blocks:?}");
        }
    }

    #[test]
    fn parse_columns_with_bold_total() {
        let input = "**Subtotal** | $25.00";
        let blocks = parse_receipt_markdown(input);

        assert_eq!(blocks.len(), 1);
        if let ReceiptBlock::Columns { cells } = &blocks[0] {
            assert_eq!(cells.len(), 2);
            assert_eq!(cells[0].len(), 1);
            assert_eq!(cells[0][0].text, "Subtotal");
            assert!(cells[0][0].format.bold);
            assert_eq!(cells[1][0].text, "$25.00");
            assert!(!cells[1][0].format.bold);
        } else {
            panic!("Expected Columns block, got {blocks:?}");
        }
    }

    #[test]
    fn parse_blank_lines_preserved() {
        let input = "Hello\n\nWorld";
        let blocks = parse_receipt_markdown(input);

        assert_eq!(blocks.len(), 3);
        assert!(matches!(blocks[0], ReceiptBlock::Line { .. }));
        assert!(matches!(blocks[1], ReceiptBlock::BlankLine));
        assert!(matches!(blocks[2], ReceiptBlock::Line { .. }));
    }

    #[test]
    fn parse_full_receipt_block_sequence() {
        let input = "\
# RIVERSIDE CAFE

Espresso | $3.00
Croissant | $4.50
_Almond_ Milk | $0.75

---

**Total** | **$8.25**

_Thank you!_";

        let blocks = parse_receipt_markdown(input);

        // Heading
        assert!(matches!(blocks[0], ReceiptBlock::Heading { .. }));
        // Blank
        assert!(matches!(blocks[1], ReceiptBlock::BlankLine));
        // Three column items
        assert!(matches!(blocks[2], ReceiptBlock::Columns { .. }));
        assert!(matches!(blocks[3], ReceiptBlock::Columns { .. }));
        assert!(matches!(blocks[4], ReceiptBlock::Columns { .. }));
        // Blank
        assert!(matches!(blocks[5], ReceiptBlock::BlankLine));
        // Divider
        assert!(matches!(blocks[6], ReceiptBlock::Divider));
        // Blank
        assert!(matches!(blocks[7], ReceiptBlock::BlankLine));
        // Total line (columns with bold)
        assert!(matches!(blocks[8], ReceiptBlock::Columns { .. }));
        // Blank
        assert!(matches!(blocks[9], ReceiptBlock::BlankLine));
        // Thank you (underlined)
        assert!(matches!(blocks[10], ReceiptBlock::Line { .. }));
    }

    // --- Inline parser tests ---

    #[test]
    fn inline_plain_text() {
        let spans = parse_inline("Just plain text");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text, "Just plain text");
        assert!(!spans[0].format.bold);
    }

    #[test]
    fn inline_bold() {
        let spans = parse_inline("**TOTAL**");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text, "TOTAL");
        assert!(spans[0].format.bold);
    }

    #[test]
    fn inline_underline() {
        let spans = parse_inline("_thanks_");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text, "thanks");
        assert!(spans[0].format.underline);
    }

    #[test]
    fn inline_mixed() {
        let spans = parse_inline("**bold** plain _underline_");
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0].text, "bold");
        assert!(spans[0].format.bold);
        assert_eq!(spans[1].text, " plain ");
        assert!(!spans[1].format.bold);
        assert_eq!(spans[2].text, "underline");
        assert!(spans[2].format.underline);
    }
}
