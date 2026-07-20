#[test]
fn context_pack_sources_are_session_relative_and_inspectable() {
    let report = crate::inspect_context_pack_json(
        r#"{
  "session": "default",
  "agent": "coder",
  "items": [
{"kind": "summary", "source": "context/summary.md"},
{"kind": "messages", "source": "messages.jsonl"},
{"kind": "child_result", "source": "context/child/rev-1/result.md"},
{"kind": "child_refs", "source": "context/child/rev-1/refs.jsonl"},
{"kind": "artifact", "source": "context/child/rev-1/artifact/report.md"},
{"kind": "pinned", "source": "context/pinned/system.md"}
  ]
}"#,
    );
    assert!(report.is_ok());
    assert!(crate::validate_context_pack_source("context/facts.jsonl").is_ok());
}

#[test]
fn context_pack_sources_reject_escapes_and_child_history() {
    assert_eq!(
        crate::validate_context_pack_source(
            "/ctx/shared/im-a/agent/bot/session/group-1/messages.jsonl"
        ),
        Err(crate::ContextPackSourceError::Absolute)
    );
    assert_eq!(
        crate::validate_context_pack_source("../other/messages.jsonl"),
        Err(crate::ContextPackSourceError::ParentComponent)
    );
    assert_eq!(
        crate::validate_context_pack_source("session/other/messages.jsonl"),
        Err(crate::ContextPackSourceError::UnsupportedSessionPath)
    );
    assert_eq!(
        crate::validate_context_pack_source("context/child/rev-1/messages.jsonl"),
        Err(crate::ContextPackSourceError::UnsupportedChildPath)
    );

    let report = crate::inspect_context_pack_json(
        r#"{
  "items": [
{"kind": "ok", "source": "context/summary.md"},
{"kind": "absolute", "source": "/ctx/shared/im-b/agent/bot/session/channel-2/messages.jsonl"},
{"kind": "child_full_history", "source": "context/child/rev-1/messages.jsonl"},
{"kind": "missing"},
{"kind": "not_string", "source": 42}
  ]
}"#,
    );
    assert!(!report.is_ok());
    assert_eq!(
        report.issues(),
        [
            crate::ContextPackIssue::InvalidSource {
                item: 1,
                source: "/ctx/shared/im-b/agent/bot/session/channel-2/messages.jsonl".to_owned(),
                reason: crate::ContextPackSourceError::Absolute
            },
            crate::ContextPackIssue::InvalidSource {
                item: 2,
                source: "context/child/rev-1/messages.jsonl".to_owned(),
                reason: crate::ContextPackSourceError::UnsupportedChildPath
            },
            crate::ContextPackIssue::MissingSource(3),
            crate::ContextPackIssue::SourceNotString(4)
        ]
    );
}
