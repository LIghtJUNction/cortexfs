use super::{
    is_passthrough_tool, openai_stream_event, provider_key_names, run, ObjectPath, OpenAiStreamEvent,
    RunnerProviderConfig, run_cli_tool_to_writer,
};
use std::ffi::OsString;
use std::fs;
use std::path::Path;

#[test]
fn object_path_parses_class_and_name() {
    assert_eq!(
        ObjectPath::parse(Path::new("/ctx/model/debug/echo")),
        Ok(ObjectPath {
            class: "model".to_owned(),
            name: "debug/echo".to_owned(),
        })
    );
}

#[test]
fn runner_rejects_missing_object_path() {
    assert_eq!(run(Vec::new()), Err("missing object path".to_owned()));
}

#[test]
fn runner_rejects_unknown_model() {
    assert_eq!(
        run(vec![OsString::from("/ctx/model/openai/gpt-4o")]),
        Err("missing provider: openai".to_owned())
    );
}

#[test]
fn runner_recognizes_interactive_tool_passthroughs() {
    assert!(is_passthrough_tool("bash"));
    assert!(is_passthrough_tool("tmux"));
    assert!(is_passthrough_tool("zellij"));
    assert!(is_passthrough_tool("tsh"));
    assert!(!is_passthrough_tool("shell.exec"));
    assert!(!is_passthrough_tool("fs.read"));
}

#[test]
fn cli_tool_mode_outputs_plain_text() {
    let path = std::env::temp_dir().join(format!("cortexfs-runner-cli-{}", std::process::id()));
    assert!(fs::write(&path, "plain").is_ok());
    let mut output = Vec::new();
    let result = run_cli_tool_to_writer("fs.read", &[OsString::from(&path)], &mut output);
    assert!(result.is_ok());
    assert_eq!(String::from_utf8(output).unwrap_or_default(), "plain");
    let _ignored = fs::remove_file(path);
}

#[test]
fn openai_stream_event_extracts_chat_delta_text() {
    let event = openai_stream_event(
        r#"data: {"choices":[{"delta":{"content":"hel"}}]}"#,
    );
    assert!(matches!(event, Ok(OpenAiStreamEvent::Delta(text)) if text == "hel"));
}

#[test]
fn openai_stream_event_accepts_done_marker() {
    assert!(matches!(
        openai_stream_event("data: [DONE]"),
        Ok(OpenAiStreamEvent::Done)
    ));
}

#[test]
fn openai_stream_event_does_not_mix_reasoning_into_answer_text() {
    let event = openai_stream_event(
        r#"data: {"choices":[{"delta":{"reasoning_content":"hidden"}}]}"#,
    );
    assert!(matches!(event, Ok(OpenAiStreamEvent::Delta(text)) if text.is_empty()));
}

#[test]
fn provider_key_names_accept_configured_and_host_fallbacks() {
    assert_eq!(
        provider_key_names(&RunnerProviderConfig {
            base_url: "https://api.openai.com/v1".to_owned(),
            api_key_env: Some("CORTEXFS_OPENAI_KEY".to_owned()),
        }),
        vec![
            "CORTEXFS_OPENAI_KEY".to_owned(),
            "OPENAI_COM_API_KEY".to_owned(),
            "API_OPENAI_COM_API_KEY".to_owned(),
        ]
    );
}

#[test]
fn provider_key_names_reject_invalid_configured_names() {
    assert_eq!(
        provider_key_names(&RunnerProviderConfig {
            base_url: "https://localhost:11434/v1".to_owned(),
            api_key_env: Some("bad-name".to_owned()),
        }),
        vec!["LOCALHOST_API_KEY".to_owned()]
    );
}
