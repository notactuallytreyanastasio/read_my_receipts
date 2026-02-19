use crate::receipt_markdown::{Alignment, ReceiptBlock, ReceiptSpan, SpanFormat};

/// A single wrapped output line, ready for preview or printing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrappedLine {
    pub spans: Vec<ReceiptSpan>,
    pub alignment: Alignment,
}

/// Wrap a full document of receipt blocks into output lines.
pub fn wrap_document(blocks: &[ReceiptBlock], max_chars: u8) -> Vec<WrappedLine> {
    let mut lines = Vec::new();

    for block in blocks {
        match block {
            ReceiptBlock::Heading { spans } => {
                // Headings are double-size, so effective width is halved
                let effective_max = max_chars / 2;
                let mut wrapped = wrap_spans(spans, effective_max);
                for line in &mut wrapped {
                    line.alignment = Alignment::Center;
                }
                lines.extend(wrapped);
            }
            ReceiptBlock::Line { spans, alignment } => {
                let has_double = spans.iter().any(|s| s.format.double_size);
                let effective_max = if has_double { max_chars / 2 } else { max_chars };
                let mut wrapped = wrap_spans(spans, effective_max);
                for line in &mut wrapped {
                    line.alignment = *alignment;
                }
                lines.extend(wrapped);
            }
            ReceiptBlock::Divider => {
                lines.push(WrappedLine {
                    spans: vec![ReceiptSpan::plain("-".repeat(max_chars as usize))],
                    alignment: Alignment::Left,
                });
            }
            ReceiptBlock::Columns { cells } => {
                lines.push(format_columns(cells, max_chars));
            }
            ReceiptBlock::BlankLine => {
                lines.push(WrappedLine {
                    spans: vec![ReceiptSpan::plain("")],
                    alignment: Alignment::Left,
                });
            }
        }
    }

    lines
}

/// Wrap a sequence of spans to fit within max_chars, breaking at word boundaries.
/// Never splits a word — if a single word exceeds max_chars, it gets its own line.
pub fn wrap_spans(spans: &[ReceiptSpan], max_chars: u8) -> Vec<WrappedLine> {
    let max = max_chars as usize;
    let mut lines: Vec<WrappedLine> = Vec::new();
    let mut current_spans: Vec<ReceiptSpan> = Vec::new();
    let mut current_len: usize = 0;
    let mut needs_space = false;

    for span in spans {
        let words = split_words(&span.text);

        for word in words {
            if word.is_empty() {
                continue;
            }

            let word_len = word.len();

            if current_len == 0 {
                // Start of line — just add the word
                push_text_to_spans(&mut current_spans, &word, &span.format);
                current_len = word_len;
                needs_space = true;
            } else if needs_space && current_len + 1 + word_len <= max {
                // Fits with a space
                push_text_to_spans(&mut current_spans, " ", &span.format);
                push_text_to_spans(&mut current_spans, &word, &span.format);
                current_len += 1 + word_len;
            } else if !needs_space && current_len + word_len <= max {
                // Fits without space (continuation)
                push_text_to_spans(&mut current_spans, &word, &span.format);
                current_len += word_len;
                needs_space = true;
            } else {
                // Doesn't fit — emit current line, start new one
                lines.push(WrappedLine {
                    spans: current_spans,
                    alignment: Alignment::Left,
                });
                current_spans = Vec::new();
                push_text_to_spans(&mut current_spans, &word, &span.format);
                current_len = word_len;
                needs_space = true;
            }
        }
    }

    // Emit remaining content
    if !current_spans.is_empty() {
        lines.push(WrappedLine {
            spans: current_spans,
            alignment: Alignment::Left,
        });
    }

    if lines.is_empty() {
        lines.push(WrappedLine {
            spans: vec![ReceiptSpan::plain("")],
            alignment: Alignment::Left,
        });
    }

    lines
}

/// Split text into words (whitespace-separated).
fn split_words(text: &str) -> Vec<String> {
    text.split_whitespace().map(String::from).collect()
}

/// Add text to the last span if it has the same format, or create a new span.
fn push_text_to_spans(spans: &mut Vec<ReceiptSpan>, text: &str, format: &SpanFormat) {
    if let Some(last) = spans.last_mut() {
        if &last.format == format {
            last.text.push_str(text);
            return;
        }
    }
    spans.push(ReceiptSpan {
        text: text.to_string(),
        format: format.clone(),
    });
}

/// Format pipe-delimited columns into a single padded line.
/// Left column is left-justified, right column is right-justified.
fn format_columns(cells: &[Vec<ReceiptSpan>], max_chars: u8) -> WrappedLine {
    let max = max_chars as usize;

    if cells.len() < 2 {
        // Single cell — just return as a line
        let spans = cells.first().cloned().unwrap_or_default();
        return WrappedLine {
            spans,
            alignment: Alignment::Left,
        };
    }

    // Get text content of left and right cells
    let left_text: String = cells[0].iter().map(|s| s.text.as_str()).collect();
    let right_text: String = cells[1].iter().map(|s| s.text.as_str()).collect();

    let left_len = left_text.len();
    let right_len = right_text.len();

    // Calculate padding between left and right
    let padding = if left_len + right_len < max {
        max - left_len - right_len
    } else {
        1 // minimum 1 space between columns
    };

    let mut spans = cells[0].clone();
    spans.push(ReceiptSpan::plain(" ".repeat(padding)));
    spans.extend(cells[1].iter().cloned());

    WrappedLine {
        spans,
        alignment: Alignment::Left,
    }
}

/// Compute the total character length of spans in a line.
pub fn line_char_count(spans: &[ReceiptSpan]) -> usize {
    spans.iter().map(|s| s.text.len()).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::receipt_markdown::parse_receipt_markdown;

    #[test]
    fn short_line_no_wrap() {
        let spans = vec![ReceiptSpan::plain("Hello world")];
        let lines = wrap_spans(&spans, 42);
        assert_eq!(lines.len(), 1);
        assert_eq!(line_char_count(&lines[0].spans), 11);
    }

    #[test]
    fn exact_fit_42_chars() {
        let text = "A".repeat(42);
        let spans = vec![ReceiptSpan::plain(&text)];
        let lines = wrap_spans(&spans, 42);
        assert_eq!(lines.len(), 1);
        assert_eq!(line_char_count(&lines[0].spans), 42);
    }

    #[test]
    fn wrap_at_word_boundary() {
        // "The quick brown fox jumps over the lazy dog near" = 49 chars
        let spans = vec![ReceiptSpan::plain(
            "The quick brown fox jumps over the lazy dog near the river",
        )];
        let lines = wrap_spans(&spans, 42);
        assert!(lines.len() >= 2);
        // No line should exceed 42 chars
        for line in &lines {
            assert!(line_char_count(&line.spans) <= 42);
        }
    }

    #[test]
    fn never_split_words() {
        // A word longer than max_chars gets its own line
        let long_word = "A".repeat(50);
        let spans = vec![ReceiptSpan::plain(&long_word)];
        let lines = wrap_spans(&spans, 42);
        assert_eq!(lines.len(), 1);
        // The word overflows — we never split it
        assert_eq!(line_char_count(&lines[0].spans), 50);
    }

    #[test]
    fn double_size_halves_width() {
        let spans = vec![ReceiptSpan {
            text: "This is a double size heading text".to_string(),
            format: SpanFormat {
                double_size: true,
                bold: true,
                ..Default::default()
            },
        }];
        // With max_chars=42, double_size effective max is 21
        let lines = wrap_spans(&spans, 21);
        assert!(lines.len() >= 2);
        for line in &lines {
            assert!(line_char_count(&line.spans) <= 21);
        }
    }

    #[test]
    fn column_padding_fills_width() {
        let cells = vec![
            vec![ReceiptSpan::plain("Coffee")],
            vec![ReceiptSpan::plain("$4.50")],
        ];
        let line = format_columns(&cells, 42);
        // Total should be 42: "Coffee" (6) + padding (31) + "$4.50" (5)
        assert_eq!(line_char_count(&line.spans), 42);
    }

    #[test]
    fn column_bold_price() {
        let cells = vec![
            vec![ReceiptSpan::bold("Total")],
            vec![ReceiptSpan::bold("$10.25")],
        ];
        let line = format_columns(&cells, 42);
        assert_eq!(line_char_count(&line.spans), 42);
        // First span should be bold
        assert!(line.spans[0].format.bold);
        // Last span should be bold
        assert!(line.spans.last().unwrap().format.bold);
    }

    #[test]
    fn real_receipt_wrap() {
        let input = "\
# RIVERSIDE CAFE

Espresso | $3.00
Croissant with butter | $4.50

---

**Total** | **$8.25**";

        let blocks = parse_receipt_markdown(input);
        let lines = wrap_document(&blocks, 42);

        // All lines should respect width limits
        for line in &lines {
            let count = line_char_count(&line.spans);
            // Allow empty lines and slight overflow for single long words
            if count > 0 {
                // Dividers should be exactly 42
                // Columns should be 42
                // Headings wrap at 21
                // Normal text wraps at 42
                assert!(count <= 42, "Line too long ({count}): {line:?}");
            }
        }
    }

    #[test]
    fn never_splits_words_real_input() {
        let input =
            "whats up buttercup we are gonna attempt the word splitting situation now and see what happens";
        let blocks = parse_receipt_markdown(input);
        let lines = wrap_document(&blocks, 42);

        // Collect all line texts
        let all_output_text: Vec<String> = lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.text.as_str()).collect::<String>())
            .collect();

        // No line should exceed 42 chars
        for (i, line_text) in all_output_text.iter().enumerate() {
            assert!(
                line_text.len() <= 42,
                "Line {i} exceeds 42 chars ({} chars): {line_text:?}",
                line_text.len()
            );
        }

        // Every word from the input should appear intact in a single line
        for word in input.split_whitespace() {
            let found = all_output_text.iter().any(|line| line.contains(word));
            assert!(found, "Word {word:?} was split across lines");
        }
    }

    #[test]
    fn composition_parse_then_wrap() {
        let input = "**Welcome** to our _store_\n\nLatte | $5.00\nScone | $3.50\n\n---\n\n**Total** | **$8.50**";
        let blocks = parse_receipt_markdown(input);
        let lines = wrap_document(&blocks, 42);

        // Should have: welcome line, blank, 2 column lines, blank, divider, blank, total
        assert!(lines.len() >= 7);

        // First line should contain bold text
        assert!(lines[0].spans.iter().any(|s| s.format.bold));

        // Divider line should be 42 dashes
        let divider_line = lines.iter().find(|l| {
            l.spans.len() == 1
                && l.spans[0].text.chars().all(|c| c == '-')
                && l.spans[0].text.len() == 42
        });
        assert!(divider_line.is_some());
    }
}
