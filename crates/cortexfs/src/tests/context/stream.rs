#[test]
fn message_stream_accepts_canonical_role_content_frames() {
    let report = inspect_message_stream_jsonl(
        r#"{"role":"system","content":"You are concise."}
{"role":"user","content":[{"type":"text","text":"hello"}]}
{"role":"assistant","content":[{"type":"text","text":"hi"}]}
{"role":"tool","content":[{"type":"tool_result","tool_call_id":"call-1","content":"ok"}]}
"#,
    );
    assert!(report.is_ok());
    assert!(report.issues().is_empty());
}

#[test]
fn message_stream_reports_missing_final_newline() {
    let complete =
        "{\"role\":\"user\",\"content\":\"one\"}\n{\"role\":\"assistant\",\"content\":\"two\"}\n";
    assert!(inspect_message_stream_jsonl(complete).is_ok());

    let report = inspect_message_stream_jsonl(complete.trim_end_matches('\n'));
    assert_eq!(
        report.issues(),
        [MessageStreamIssue::MissingFinalNewline(2)]
    );
}

#[test]
fn message_stream_rejects_native_state_and_bad_shape() {
    let report = inspect_message_stream_jsonl(
        r#"not-json
[]
{"content":"missing role"}
{"role":"developer","content":"private role"}
{"role":"assistant","response_id":"resp-1","content":"hi"}
{"role":"assistant","content":[{"type":"provider_blob","text":"x"}]}
{"role":"assistant"}
{"role":"assistant","content":[{"type":"text","text":"safe","text":"shadow"}]}
"#,
    );
    assert_eq!(
        report.issues(),
        [
            MessageStreamIssue::InvalidJson(1),
            MessageStreamIssue::MessageNotObject(2),
            MessageStreamIssue::MissingRole(3),
            MessageStreamIssue::InvalidRole {
                line: 4,
                role: "developer".to_owned()
            },
            MessageStreamIssue::ProviderNativeField {
                line: 5,
                field: "response_id".to_owned()
            },
            MessageStreamIssue::InvalidContent(6),
            MessageStreamIssue::MissingContent(7),
            MessageStreamIssue::InvalidContent(8)
        ]
    );
}

#[test]
fn context_jsonl_accepts_spec_record_shapes() {
    assert!(
        inspect_context_jsonl(
            ContextJsonlKind::Facts,
            r#"{"id":"f1","text":"CortexFS root is small.","source":"messages:12-18"}
"#
        )
        .is_ok()
    );
    assert!(
        inspect_context_jsonl(
            ContextJsonlKind::Decisions,
            r#"{"id":"d1","decision":"Child agents are owned.","source":"user:latest"}
"#
        )
        .is_ok()
    );
    assert!(
        inspect_context_jsonl(
            ContextJsonlKind::Refs,
            r#"{"id":"r1","path":"/work/DESIGN.md","kind":"file","summary":"design"}
{"id":"r2","path":"context/swap/chunk/sha256-abc","kind":"swap","summary":"old design"}
"#
        )
        .is_ok()
    );
    assert!(
        inspect_context_jsonl(
            ContextJsonlKind::SwapIndex,
            r#"{"id":"sha256-abc","kind":"message_range","source":"messages.jsonl","summary":"initial design","tokens":18000}
{"id":"sha256-def","kind":"tool_output","source":"events.jsonl","summary":"test output","tokens":45000}
"#
        )
        .is_ok()
    );
    assert!(
        inspect_context_jsonl(
            ContextJsonlKind::DedupIndex,
            r#"{"hash":"sha256-abc","refs":["messages:1-40","swap:old-design"],"bytes":12000,"tokens":3000}
"#
        )
        .is_ok()
    );
}

#[test]
fn context_jsonl_preserves_field_issue_order() {
    let cases: &[(ContextJsonlKind, &[&str])] = &[
        (ContextJsonlKind::Facts, &["id", "text", "source"]),
        (ContextJsonlKind::Decisions, &["id", "decision", "source"]),
        (ContextJsonlKind::Refs, &["id", "path", "kind", "summary"]),
        (
            ContextJsonlKind::SwapIndex,
            &["id", "kind", "source", "summary", "tokens"],
        ),
        (
            ContextJsonlKind::DedupIndex,
            &["hash", "refs", "bytes", "tokens"],
        ),
    ];
    for &(kind, expected) in cases {
        let report = inspect_context_jsonl(kind, "{}\n");
        let actual = report
            .issues()
            .iter()
            .map(|issue| match *issue {
                ContextJsonlIssue::MissingStringField { ref field, .. }
                | ContextJsonlIssue::MissingNumberField { ref field, .. }
                | ContextJsonlIssue::MissingStringArrayField { ref field, .. } => field.as_str(),
                _ => "unexpected issue",
            })
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);
    }
}

fn invalid_context_field(line: usize, field: &str, value: &str) -> ContextJsonlIssue {
    ContextJsonlIssue::InvalidField {
        line,
        field: field.to_owned(),
        value: value.to_owned(),
    }
}

#[test]
fn context_jsonl_rejects_invalid_records() {
    let facts = inspect_context_jsonl(
        ContextJsonlKind::Facts,
        "not-json\n[]\n{\"id\":\"fact-1\",\"text\":\"ok\",\"source\":\"messages:1\"} trailing\n{\"id\":\"bad/id\",\"text\":\"ok\"}\n",
    );
    assert_eq!(
        facts.issues(),
        [
            ContextJsonlIssue::InvalidJson(1),
            ContextJsonlIssue::RecordNotObject(2),
            ContextJsonlIssue::InvalidJson(3),
            invalid_context_field(4, "id", "bad/id"),
            ContextJsonlIssue::MissingStringField {
                line: 4,
                field: "source".to_owned()
            }
        ]
    );

    let duplicate = inspect_context_jsonl(
        ContextJsonlKind::Facts,
        "{\"id\":\"fact-1\",\"id\":\"fact-2\",\"text\":\"ok\",\"source\":\"messages:1\"}\n",
    );
    assert_eq!(duplicate.issues(), [ContextJsonlIssue::InvalidJson(1)]);

    let refs = inspect_context_jsonl(
        ContextJsonlKind::Refs,
        r#"{"id":"r1","path":"../secret","kind":"provider_thread","summary":"bad"}
"#,
    );
    assert_eq!(
        refs.issues(),
        [
            invalid_context_field(1, "path", "../secret"),
            invalid_context_field(1, "kind", "provider_thread")
        ]
    );

    let control_path = inspect_context_jsonl(
        ContextJsonlKind::Refs,
        "{\"id\":\"r1\",\"path\":\"context/\\u001bhidden.md\",\"kind\":\"file\",\"summary\":\"bad\"}\n",
    );
    assert_eq!(
        control_path.issues(),
        [invalid_context_field(1, "path", "context/\u{1b}hidden.md")]
    );

    let dedup = inspect_context_jsonl(
        ContextJsonlKind::DedupIndex,
        r#"{"hash":"md5-old","refs":[],"bytes":"120","tokens":3000}
"#,
    );
    assert_eq!(
        dedup.issues(),
        [
            invalid_context_field(1, "hash", "md5-old"),
            ContextJsonlIssue::MissingStringArrayField {
                line: 1,
                field: "refs".to_owned()
            },
            ContextJsonlIssue::MissingNumberField {
                line: 1,
                field: "bytes".to_owned()
            }
        ]
    );
}

#[test]
fn event_stream_accepts_canonical_model_jsonl() {
    let report = inspect_event_stream_jsonl(
        r#"{"type":"start","run":"r1","model":"debug/echo"}
{"type":"delta","run":"r1","text":"hello"}
{"type":"message","run":"r1","role":"assistant","content":[{"type":"text","text":"hello"}]}
{"type":"tool_call","run":"r1","id":"call-1","name":"fs.read","arguments":{"path":"README.md"}}
{"type":"usage","run":"r1","input_tokens":10,"output_tokens":1,"cached_tokens":8,"cache_write_tokens":4}
{"type":"done","run":"r1","status":"ok"}
"#,
    );
    assert!(report.is_ok());
    assert!(report.issues().is_empty());
}

#[test]
fn event_stream_reports_missing_final_newline() {
    let complete = "{\"type\":\"start\",\"run\":\"r1\"}\n{\"type\":\"done\",\"run\":\"r1\",\"status\":\"ok\"}\n";
    assert!(inspect_event_stream_jsonl(complete).is_ok());

    let report = inspect_event_stream_jsonl(complete.trim_end_matches('\n'));
    assert_eq!(report.issues(), [EventStreamIssue::MissingFinalNewline(2)]);
}

#[test]
fn event_stream_accepts_stable_error_frames() {
    let report = inspect_event_stream_jsonl(
        r#"{"type":"error","run":"r1","code":"EACCES","message":"permission denied"}
{"type":"done","run":"r1","status":"error"}
"#,
    );
    assert!(report.is_ok());
}

#[test]
fn event_stream_accepts_host_approval_frames() {
    let report = inspect_event_stream_jsonl(
        r#"{"type":"approval_request","run":"r1","id":"call-1","name":"example.echo","args":["one","two"]}
{"type":"approval_result","run":"r1","id":"call-1","name":"example.echo","decision":"allow_once","reason":"approved once"}
"#,
    );
    assert!(report.is_ok(), "{:?}", report.issues());
}

#[test]
fn event_stream_rejects_invalid_approval_frames() {
    let report = inspect_event_stream_jsonl(
        r#"{"type":"approval_request","run":"r1","id":"call-1","name":"example.echo"}
{"type":"approval_request","run":"r1","id":"call-1","name":"bad/name","args":[]}
{"type":"approval_request","run":"r1","id":"call-1","name":"example.echo","args":["one",2]}
{"type":"approval_result","run":"r1","id":"call-1","name":"example.echo","decision":"always","reason":"approved"}
{"type":"approval_result","run":"r1","id":"call-1","name":"example.echo","decision":"deny","reason":""}
"#,
    );
    assert_eq!(
        report.issues(),
        [
            EventStreamIssue::InvalidApproval(1),
            EventStreamIssue::InvalidApproval(2),
            EventStreamIssue::InvalidApproval(3),
            EventStreamIssue::InvalidApproval(4),
            EventStreamIssue::InvalidApproval(5),
        ]
    );
}

#[test]
fn event_stream_accepts_child_lifecycle_frames() {
    let report = inspect_event_stream_jsonl(
        r#"{"type":"agent.child.cancel","parent":"executor","child":"rev-123","reason":"parent_dead"}
{"type":"agent.stop","agent":"rev-123","status":"cancelled"}
"#,
    );
    assert!(report.is_ok());
    assert!(report.issues().is_empty());
}

#[test]
fn event_stream_rejects_provider_native_state_and_unknown_events() {
    let report = inspect_event_stream_jsonl(
        r#"{"type":"start","run":"r1","model":"debug/echo","response_id":"resp_123"}
{"type":"native_thread","run":"r1","thread_id":"thread_123"}
{"type":"message","run":"r1","content":[{"type":"text","text":"x","provider_response_id":"abc"}]}
"#,
    );
    assert_eq!(
        report.issues(),
        [
            EventStreamIssue::ProviderNativeField {
                line: 1,
                field: "response_id".to_owned()
            },
            EventStreamIssue::ProviderNativeField {
                line: 2,
                field: "thread_id".to_owned()
            },
            EventStreamIssue::UnknownType {
                line: 2,
                event_type: "native_thread".to_owned()
            },
            EventStreamIssue::ProviderNativeField {
                line: 3,
                field: "provider_response_id".to_owned()
            }
        ]
    );
}

#[test]
fn event_stream_rejects_invalid_shape_and_specialized_frames() {
    let report = inspect_event_stream_jsonl(
        r#"not-json
[]
{"run":"r1"}
{"type":"delta","text":"missing run"}
{"type":"error","run":"r1","code":"PROVIDER_DENIED"}
{"type":"done","run":"r1","status":"maybe"}
{"type":"usage","run":"r1","input_tokens":10,"output_tokens":1,"cached_tokens":"8"}
{"type":"tool_call","run":"r1","id":"bad/id","name":"fs.read"}
{"type":"agent.child.cancel","parent":"bad/parent","child":"rev-1","reason":"manual"}
{"type":"agent.stop","agent":"rev-1","status":"dead"}
"#,
    );
    assert_eq!(
        report.issues(),
        [
            EventStreamIssue::InvalidJson(1),
            EventStreamIssue::EventNotObject(2),
            EventStreamIssue::MissingType(3),
            EventStreamIssue::MissingRun(4),
            EventStreamIssue::InvalidErrorCode(5),
            EventStreamIssue::InvalidDoneStatus(6),
            EventStreamIssue::InvalidUsage(7),
            EventStreamIssue::InvalidToolCall(8),
            EventStreamIssue::InvalidAgentLifecycle(9),
            EventStreamIssue::InvalidAgentLifecycle(10)
        ]
    );
}
use super::*;
