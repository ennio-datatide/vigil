//! End-to-end integration tests.

#![cfg(test)]

#[tokio::test]
async fn output_extraction_large_data() {
    use crate::process::output_extract::{build_context_chain, extract_response_text, strip_ansi};

    // -- Large output with ANSI codes.
    let mut large = Vec::new();
    for i in 0..10_000 {
        large.extend_from_slice(format!("\x1b[32mLine {i}\x1b[0m\n").as_bytes());
    }
    let extracted = extract_response_text(&large);
    assert!(
        !extracted.contains("\x1b["),
        "should strip all ANSI sequences"
    );
    assert!(
        extracted.len() <= 4000,
        "should truncate to 4000 chars, got {}",
        extracted.len()
    );

    // -- Plain text — no ANSI.
    let plain = b"Hello, this is a simple response.";
    let result = extract_response_text(plain);
    assert_eq!(result, "Hello, this is a simple response.");

    // -- Empty input.
    assert!(extract_response_text(b"").is_empty());

    // -- ANSI-only content.
    let ansi_only = b"\x1b[31m\x1b[0m\x1b[32m\x1b[0m";
    let result = strip_ansi(ansi_only);
    assert!(
        result.trim().is_empty(),
        "ANSI-only content should be empty after stripping"
    );

    // -- Context chain with truncation.
    let labels: Vec<String> = vec!["A", "B", "C", "D", "E"]
        .into_iter()
        .map(String::from)
        .collect();
    let long_output = "X".repeat(3000);
    let outputs: Vec<String> = vec![&long_output; 5]
        .into_iter()
        .map(|s| s.clone())
        .collect();
    let chain = build_context_chain("user prompt", &labels, &outputs, "current step");
    assert!(
        chain.contains("<user_request>"),
        "should contain user request tag"
    );
    assert!(
        chain.contains("<current_step>"),
        "should contain current step tag"
    );
    // Oldest steps should have been truncated; most recent step (E) must be present.
    assert!(
        chain.contains("Step 5: E"),
        "most recent step should be kept after truncation"
    );
}
