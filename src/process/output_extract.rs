//! ANSI stripping and response text extraction for pipeline context chaining.

use std::fmt::Write as _;

/// Maximum length of extracted response text.
pub(crate) const MAX_RESPONSE_LEN: usize = 4000;

/// Maximum length of accumulated previous-step context.
pub(crate) const MAX_CONTEXT_LEN: usize = 8000;

/// Strip ANSI escape sequences from raw bytes, returning clean UTF-8.
pub fn strip_ansi(raw: &[u8]) -> String {
    let lossy = String::from_utf8_lossy(raw);
    let mut result = String::with_capacity(lossy.len());
    let mut chars = lossy.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            // ESC — determine sequence type
            match chars.peek() {
                Some('[') => {
                    // CSI sequence: ESC [ ... final_byte (0x40–0x7E)
                    chars.next(); // consume '['
                    for c in chars.by_ref() {
                        if ('\x40'..='\x7e').contains(&c) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    // OSC sequence: ESC ] ... (ST or BEL)
                    chars.next(); // consume ']'
                    loop {
                        match chars.next() {
                            Some('\x07') | None => break, // BEL or EOF
                            Some('\x1b') => {
                                // Check for ST (ESC \)
                                if chars.peek() == Some(&'\\') {
                                    chars.next();
                                }
                                break;
                            }
                            _ => {}
                        }
                    }
                }
                _ => {
                    // Single-char escape — consume the next char
                    chars.next();
                }
            }
        } else {
            result.push(ch);
        }
    }

    result
}

/// Extract clean response text from raw output bytes.
///
/// Strips ANSI, trims whitespace, and truncates to the last [`MAX_RESPONSE_LEN`] chars.
pub fn extract_response_text(raw: &[u8]) -> String {
    if raw.is_empty() {
        return String::new();
    }

    let clean = strip_ansi(raw);
    let trimmed = clean.trim();

    if trimmed.is_empty() {
        return String::new();
    }

    if trimmed.len() > MAX_RESPONSE_LEN {
        let start = trimmed.len() - MAX_RESPONSE_LEN;
        // Find a safe char boundary
        let start = trimmed.ceil_char_boundary(start);
        trimmed[start..].to_string()
    } else {
        trimmed.to_string()
    }
}

/// Build a composite prompt with context from previous pipeline steps.
///
/// Wraps the user request, completed step outputs, and current step prompt
/// in XML tags. If accumulated previous steps exceed [`MAX_CONTEXT_LEN`],
/// the oldest steps are dropped (keeping the most recent 2).
pub fn build_context_chain(
    user_request: &str,
    labels: &[String],
    outputs: &[String],
    current_step_prompt: &str,
) -> String {
    let mut previous_sections: Vec<String> = labels
        .iter()
        .zip(outputs.iter())
        .enumerate()
        .map(|(i, (label, output))| {
            format!("## Step {}: {} (completed)\n{}", i + 1, label, output)
        })
        .collect();

    // Truncate oldest steps if total exceeds limit, keeping most recent 2
    loop {
        let total_len: usize = previous_sections.iter().map(String::len).sum();
        if total_len <= MAX_CONTEXT_LEN || previous_sections.len() <= 2 {
            break;
        }
        previous_sections.remove(0);
    }

    let mut result = format!("<user_request>\n{user_request}\n</user_request>\n\n");

    if !previous_sections.is_empty() {
        result.push_str("<previous_steps>\n");
        result.push_str(&previous_sections.join("\n\n"));
        result.push_str("\n</previous_steps>\n\n");
    }

    let _ = write!(result, "<current_step>\n{current_step_prompt}\n</current_step>");

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_ansi_escape_sequences() {
        let input = b"\x1b[31mhello\x1b[0m";
        assert_eq!(strip_ansi(input), "hello");
    }

    #[test]
    fn strips_complex_ansi_sequences() {
        let input = b"\x1b[38;5;196mcolored\x1b[0m normal";
        assert_eq!(strip_ansi(input), "colored normal");
    }

    #[test]
    fn extract_response_returns_last_text_block() {
        let input = b"some output text here";
        let result = extract_response_text(input);
        assert_eq!(result, "some output text here");
    }

    #[test]
    fn extract_response_strips_ansi_first() {
        let input = b"\x1b[32mgreen text\x1b[0m";
        let result = extract_response_text(input);
        assert_eq!(result, "green text");
    }

    #[test]
    fn extract_response_truncates_to_limit() {
        let long = "x".repeat(10_000);
        let result = extract_response_text(long.as_bytes());
        assert_eq!(result.len(), MAX_RESPONSE_LEN);
    }

    #[test]
    fn extract_response_empty_input() {
        assert_eq!(extract_response_text(b""), "");
    }

    #[test]
    fn build_context_chain_basic() {
        let result = build_context_chain(
            "fix the bug",
            &["Analyze".to_string()],
            &["Found issue in main.rs".to_string()],
            "Now apply the fix",
        );

        assert!(result.contains("<user_request>"));
        assert!(result.contains("fix the bug"));
        assert!(result.contains("</user_request>"));
        assert!(result.contains("<previous_steps>"));
        assert!(result.contains("## Step 1: Analyze (completed)"));
        assert!(result.contains("Found issue in main.rs"));
        assert!(result.contains("</previous_steps>"));
        assert!(result.contains("<current_step>"));
        assert!(result.contains("Now apply the fix"));
        assert!(result.contains("</current_step>"));
    }

    #[test]
    fn build_context_chain_truncates_oldest_steps() {
        let labels: Vec<String> = (1..=10).map(|i| format!("Step{i}")).collect();
        let outputs: Vec<String> = (1..=10).map(|_| "a".repeat(2000)).collect();

        let result = build_context_chain("request", &labels, &outputs, "do next thing");

        // Oldest steps should be dropped, only most recent ones kept
        // With 10 steps × 2000 chars each = 20000 > 8000, so oldest get dropped
        // until <= 2 remain or total <= 8000
        // Each section is ~2020 chars ("## Step N: StepN (completed)\n" + 2000 'a's)
        // 4 sections ≈ 8080 > 8000, so we keep the last 2 when we can't fit more

        // The last two steps (Step9, Step10) must be present
        assert!(result.contains("Step9"));
        assert!(result.contains("Step10"));

        // The first step should have been dropped
        assert!(!result.contains("Step1 (completed)"));
        assert!(!result.contains("Step2 (completed)"));
    }
}
