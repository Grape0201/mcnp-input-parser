/// Logical card produced after stripping comments and joining continuation lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogicalCard {
    /// 1-based starting line number in the original file.
    pub line_number: usize,
    /// Joined card text with comments removed.
    pub text: String,
}

/// Result of splitting an MCNP input file into blocks of logical cards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreprocessedInput {
    pub message: Option<String>,
    pub title: String,
    pub cell_cards: Vec<LogicalCard>,
    pub surface_cards: Vec<LogicalCard>,
    pub data_cards: Vec<LogicalCard>,
}

/// Checks if a raw line is an MCNP comment line (`C` or `c` in columns 1-5 followed by space or EOL).
pub fn is_comment_line(line: &str) -> bool {
    // Expand leading tabs conservatively or count leading spaces
    let mut leading_spaces = 0;
    let mut chars = line.chars();
    while let Some(ch) = chars.next() {
        if ch == ' ' {
            leading_spaces += 1;
            if leading_spaces >= 5 {
                return false;
            }
        } else if ch == 'c' || ch == 'C' {
            // Must be followed by nothing or whitespace
            return match chars.next() {
                None => true,
                Some(next_ch) => next_ch.is_whitespace(),
            };
        } else {
            return false;
        }
    }
    false
}

/// Checks if a line is a continuation line (starts with at least 5 spaces).
pub fn is_indented_continuation(line: &str) -> bool {
    let mut spaces = 0;
    for ch in line.chars() {
        if ch == ' ' {
            spaces += 1;
            if spaces >= 5 {
                return true;
            }
        } else if ch == '\t' {
            // Standard MCNP treats tab as advancing to column 8 or multiple spaces
            return true;
        } else {
            break;
        }
    }
    false
}

/// Strips inline `$` comment from a line.
pub fn strip_inline_comment(line: &str) -> &str {
    match line.find('$') {
        Some(idx) => &line[..idx],
        None => line,
    }
}

/// Converts a slice of raw lines (within a single MCNP block) into logical cards.
pub fn extract_logical_cards(lines: &[(usize, &str)]) -> Vec<LogicalCard> {
    let mut cards: Vec<LogicalCard> = Vec::new();
    let mut force_continue_next = false;

    for &(line_no, raw_line) in lines {
        if is_comment_line(raw_line) {
            continue;
        }

        let without_dollar = strip_inline_comment(raw_line);
        let trimmed_end = without_dollar.trim_end();
        if trimmed_end.trim().is_empty() {
            continue;
        }

        let is_indented = is_indented_continuation(without_dollar);
        let (clean_segment, ends_with_amp) = if let Some(stripped) = trimmed_end.strip_suffix('&') {
            (stripped.trim(), true)
        } else {
            (trimmed_end.trim(), false)
        };

        if (force_continue_next || is_indented) && !cards.is_empty() {
            let last = cards.last_mut().unwrap();
            if !clean_segment.is_empty() {
                last.text.push(' ');
                last.text.push_str(clean_segment);
            }
        } else {
            cards.push(LogicalCard {
                line_number: line_no,
                text: clean_segment.to_string(),
            });
        }

        force_continue_next = ends_with_amp;
    }

    cards
}

/// Preprocesses a complete MCNP input string into message, title, and 3 blocks of logical cards.
pub fn preprocess_mcnp(input: &str) -> PreprocessedInput {
    let numbered_lines: Vec<(usize, &str)> = input
        .lines()
        .enumerate()
        .map(|(idx, line)| (idx + 1, line))
        .collect();

    let mut cursor = 0;
    let total = numbered_lines.len();

    // Skip any leading completely empty lines if present before header
    while cursor < total && numbered_lines[cursor].1.trim().is_empty() {
        cursor += 1;
    }

    if cursor >= total {
        return PreprocessedInput {
            message: None,
            title: String::new(),
            cell_cards: Vec::new(),
            surface_cards: Vec::new(),
            data_cards: Vec::new(),
        };
    }

    // 1. Optional message block: starts with `message:` in columns 1-5
    let mut message = None;
    let first_line_trimmed = numbered_lines[cursor].1.trim_start();
    let leading_ws = numbered_lines[cursor].1.len() - first_line_trimmed.len();
    if leading_ws < 5
        && first_line_trimmed
            .get(..8)
            .map(|s| s.eq_ignore_ascii_case("message:"))
            .unwrap_or(false)
    {
        let mut msg_lines = Vec::new();
        let after_prefix = first_line_trimmed[8..].trim();
        if !after_prefix.is_empty() {
            msg_lines.push(after_prefix.to_string());
        }
        cursor += 1;
        while cursor < total && !numbered_lines[cursor].1.trim().is_empty() {
            msg_lines.push(numbered_lines[cursor].1.to_string());
            cursor += 1;
        }
        message = Some(msg_lines.join("\n"));
        // Skip the blank line delimiter after the message block
        if cursor < total && numbered_lines[cursor].1.trim().is_empty() {
            cursor += 1;
        }
    }

    // 2. Title line
    let title = if cursor < total {
        let t = numbered_lines[cursor].1.trim_end().to_string();
        cursor += 1;
        t
    } else {
        String::new()
    };

    // 3. Split remaining lines by blank-line delimiters into up to 3 blocks
    let mut raw_blocks: Vec<Vec<(usize, &str)>> = Vec::new();
    let mut current_block: Vec<(usize, &str)> = Vec::new();

    while cursor < total {
        let (line_no, line) = numbered_lines[cursor];
        cursor += 1;
        if line.trim().is_empty() {
            raw_blocks.push(std::mem::take(&mut current_block));
            if raw_blocks.len() == 3 {
                // Any lines after the 3rd block's terminating blank line are ignored in MCNP
                break;
            }
        } else {
            current_block.push((line_no, line));
        }
    }
    if !current_block.is_empty() && raw_blocks.len() < 3 {
        raw_blocks.push(current_block);
    }

    let cell_cards = raw_blocks
        .first()
        .map(|b| extract_logical_cards(b))
        .unwrap_or_default();
    let surface_cards = raw_blocks
        .get(1)
        .map(|b| extract_logical_cards(b))
        .unwrap_or_default();
    let data_cards = raw_blocks
        .get(2)
        .map(|b| extract_logical_cards(b))
        .unwrap_or_default();

    PreprocessedInput {
        message,
        title,
        cell_cards,
        surface_cards,
        data_cards,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_comment_line_detection() {
        assert!(is_comment_line("c this is a comment"));
        assert!(is_comment_line("C another comment"));
        assert!(is_comment_line("   c indented comment (col 4)"));
        assert!(is_comment_line("c"));
        assert!(is_comment_line("  C   "));
        // Not comment lines:
        assert!(!is_comment_line("     c col 6 is continuation, not comment"));
        assert!(!is_comment_line("cut:n 1e8"));
        assert!(!is_comment_line("c1 10.0"));
        assert!(!is_comment_line("1 0 -1 $ inline comment"));
    }

    #[test]
    fn test_continuation_and_inline_comments() {
        let raw = vec![
            (1, "c cell block comment"),
            (2, "1 1 -1.0 -10 $ sphere cell"),
            (3, "     imp:n=1.0"),
            (4, "2 0 10 & $ void outside"),
            (5, "  imp:n=0"),
        ];
        let cards = extract_logical_cards(&raw);
        assert_eq!(cards.len(), 2);
        assert_eq!(cards[0].line_number, 2);
        assert_eq!(cards[0].text, "1 1 -1.0 -10 imp:n=1.0");
        assert_eq!(cards[1].line_number, 4);
        assert_eq!(cards[1].text, "2 0 10 imp:n=0");
    }

    #[test]
    fn test_preprocess_with_message_block() {
        let input = "\
MESSAGE: outp=myout runtape=myruntpe

Simple MCNP Title
1 1 -1.0 -1 imp:n=1
2 0 1 imp:n=0

1 so 5.0

mode n
m1 1001.80c 1.0
";
        let pre = preprocess_mcnp(input);
        assert_eq!(
            pre.message.as_deref(),
            Some("outp=myout runtape=myruntpe")
        );
        assert_eq!(pre.title, "Simple MCNP Title");
        assert_eq!(pre.cell_cards.len(), 2);
        assert_eq!(pre.surface_cards.len(), 1);
        assert_eq!(pre.data_cards.len(), 2);
    }
}
