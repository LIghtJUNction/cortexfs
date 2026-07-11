#[test]
fn context_pack_sources_are_session_relative_and_inspectable() {
    let report = inspect_context_pack_json(
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
    assert!(validate_context_pack_source("context/facts.jsonl").is_ok());
}

#[test]
fn context_pack_sources_reject_escapes_and_child_history() {
    assert_eq!(
        validate_context_pack_source("/ctx/shared/im-a/agent/bot/session/group-1/messages.jsonl"),
        Err(ContextPackSourceError::Absolute)
    );
    assert_eq!(
        validate_context_pack_source("../other/messages.jsonl"),
        Err(ContextPackSourceError::ParentComponent)
    );
    assert_eq!(
        validate_context_pack_source("session/other/messages.jsonl"),
        Err(ContextPackSourceError::UnsupportedSessionPath)
    );
    assert_eq!(
        validate_context_pack_source("context/child/rev-1/messages.jsonl"),
        Err(ContextPackSourceError::UnsupportedChildPath)
    );

    let report = inspect_context_pack_json(
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
            ContextPackIssue::InvalidSource {
                item: 1,
                source: "/ctx/shared/im-b/agent/bot/session/channel-2/messages.jsonl".to_owned(),
                reason: ContextPackSourceError::Absolute
            },
            ContextPackIssue::InvalidSource {
                item: 2,
                source: "context/child/rev-1/messages.jsonl".to_owned(),
                reason: ContextPackSourceError::UnsupportedChildPath
            },
            ContextPackIssue::MissingSource(3),
            ContextPackIssue::SourceNotString(4)
        ]
    );
}

#[test]
fn context_pack_rebuild_writes_inspectable_sources_without_child_history() {
    let root = clean_test_dir("context-pack-rebuild");
    let session = root.join("default");
    let context = session.join("context");

    create_complete_session_layout(&session);
    write_text_file(
        &session.join("messages.jsonl"),
        "{\"role\":\"system\",\"content\":\"base rules\"}\n{\"role\":\"user\",\"content\":\"fix tests\"}\n{\"role\":\"assistant\",\"content\":\"working\"}\n",
    );
    write_text_file(&context.join("budget"), "0\n");
    write_text_file(
        &context.join("pinned").join("system.md"),
        "Pinned system text\n",
    );
    write_text_file(&context.join("summary.md"), "Short summary\n");
    write_text_file(
        &context.join("facts.jsonl"),
        "{\"id\":\"f1\",\"text\":\"Root ABI is frozen.\",\"source\":\"messages:1-2\"}\n",
    );
    write_text_file(
        &context.join("decisions.jsonl"),
        "{\"id\":\"d1\",\"decision\":\"Do not add provider root.\",\"source\":\"messages:3\"}\n",
    );
    write_text_file(&context.join("todo.md"), "Keep FUSE small\n");
    write_text_file(
        &context.join("refs.jsonl"),
        "{\"id\":\"r1\",\"path\":\"docs/spec/16-context.md\",\"kind\":\"file\",\"summary\":\"context spec\"}\n",
    );
    write_text_file(
        &context.join("child").join("rev-1").join("result.md"),
        "Child says ok\n",
    );
    write_text_file(
        &context.join("child").join("rev-1").join("refs.jsonl"),
        "{\"id\":\"cr1\",\"path\":\"artifact/report.md\",\"kind\":\"artifact\",\"summary\":\"child report\"}\n",
    );
    write_text_file(
        &context.join("child").join("rev-1").join("messages.jsonl"),
        "{\"role\":\"user\",\"content\":\"must not be packed\"}\n",
    );

    let built = rebuild_context_pack(&session, Some("coder"), 2);
    let built = ok!(built);

    let pack_json = fs::read_to_string(context.join("pack.json"));
    let pack_json = ok!(pack_json);
    let pack_md = fs::read_to_string(context.join("pack.md"));
    let pack_md = ok!(pack_md);

    assert_eq!(built.pack_json(), pack_json);
    assert_eq!(built.pack_md(), pack_md);
    assert!(inspect_context_pack_json(&pack_json).is_ok());
    assert!(pack_json.contains("\"source\":\"context/pinned/system.md\""));
    assert!(pack_json.contains("\"source\":\"messages.jsonl\""));
    assert!(pack_json.contains("\"range\":\"tail:2\""));
    assert!(pack_json.contains("\"source\":\"context/child/rev-1/result.md\""));
    assert!(pack_json.contains("\"source\":\"context/child/rev-1/refs.jsonl\""));
    assert!(!pack_json.contains("context/child/rev-1/messages.jsonl"));
    assert!(pack_md.contains("Pinned system text"));
    assert!(pack_md.contains("Child says ok"));
    assert!(pack_md.contains("\"role\":\"assistant\""));
    assert!(!pack_md.contains("must not be packed"));
    assert!(built.items().iter().all(|item| {
        validate_context_pack_source(item.source()).is_ok()
            && item.source() != "context/child/rev-1/messages.jsonl"
    }));
}
use super::*;
