#[test]
fn context_pack_rejects_invalid_json_shape() {
    assert_eq!(
        crate::inspect_context_pack_json("{").issues(),
        &[crate::ContextPackIssue::InvalidJson]
    );
    assert_eq!(
        crate::inspect_context_pack_json(r#"{"items": []} trailing"#).issues(),
        &[crate::ContextPackIssue::InvalidJson]
    );
    assert_eq!(
        crate::inspect_context_pack_json(r#"{"items": {"source": "messages.jsonl"}}"#).issues(),
        &[crate::ContextPackIssue::ItemsNotArray]
    );
    assert_eq!(
        crate::inspect_context_pack_json(r#"{"items":[],"items":[]}"#).issues(),
        &[crate::ContextPackIssue::ItemsNotArray]
    );
    assert_eq!(
        crate::inspect_context_pack_json(r#"{"items": ["messages.jsonl"]}"#).issues(),
        &[crate::ContextPackIssue::ItemNotObject(0)]
    );
}
