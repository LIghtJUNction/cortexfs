use super::{
    MAX_AGENT_EXECUTABLE_STDERR_BYTES, MAX_AGENT_PROCESS_DIAGNOSTIC_CHARS,
    read_agent_executable_stderr_limited, safe_agent_process_diagnostic, terminal_error_message,
};
use std::io::Cursor;

#[test]
fn diagnostic_boundaries_remain_bounded_redacted_and_terminal()
-> Result<(), Box<dyn std::error::Error>> {
    let limit = usize::try_from(MAX_AGENT_EXECUTABLE_STDERR_BYTES)?;
    let output = read_agent_executable_stderr_limited(Cursor::new(vec![b'x'; limit + 1]))?;
    assert_eq!(output.len(), limit);

    let diagnostic = safe_agent_process_diagnostic(
        "cortexfs-object-runner: request failed api_key=sk-live-secret Bearer abc\nsecond",
    );
    assert!(diagnostic.contains("api_key=<redacted>"));
    assert!(diagnostic.contains("Bearer <redacted>"));
    assert!(!diagnostic.contains("live-secret") && !diagnostic.contains("abc"));
    assert!(diagnostic.chars().count() <= MAX_AGENT_PROCESS_DIAGNOSTIC_CHARS);

    let frames = [
        serde_json::json!({"type":"error", "run":"run", "message":"first"}).to_string(),
        serde_json::json!({
            "type":"error", "run":"run", "message":"recover", "recoverable":true
        })
        .to_string(),
        serde_json::json!({"type":"error", "run":"run", "message":"final"}).to_string(),
    ];
    assert_eq!(
        terminal_error_message("run", &frames).as_deref(),
        Some("final")
    );
    assert_eq!(terminal_error_message("missing", &frames), None);
    Ok(())
}
