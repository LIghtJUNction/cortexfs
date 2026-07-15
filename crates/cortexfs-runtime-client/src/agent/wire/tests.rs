use super::*;

fn frame(step: u8, observation: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "schema": AGENT_INVOCATION_SCHEMA,
        "run": "run-1",
        "step": step,
        "input": "work",
        "history_messages": "history",
        "tool_context": "tools",
        "observation": observation
    })
}

fn parse(value: &serde_json::Value) -> Result<AgentInvocationEnvelope, RuntimeClientError> {
    let mut bytes = serde_json::to_vec(value).map_err(|_error| RuntimeClientError::InvalidFrame)?;
    bytes.push(b'\n');
    read_agent_invocation(bytes.as_slice())
}

#[test]
fn accepts_exact_initial_and_continuation_frames() {
    let initial = parse(&frame(0, &serde_json::Value::Null));
    assert!(matches!(initial, Ok(ref value) if value.run() == "run-1" && value.step() == 0));

    let observation = serde_json::json!({
        "tool_call_id": "call-1",
        "name": "fs.read",
        "status": "ok",
        "content": "done",
        "truncated": false
    });
    let continuation = parse(&frame(1, &observation));
    assert!(matches!(
        continuation,
        Ok(ref value) if value.observation().is_some_and(|item|
            item.tool_call_id() == "call-1" && item.content() == "done")
    ));
}

#[test]
fn rejects_noncanonical_framing() {
    let encoded = serde_json::to_vec(&frame(0, &serde_json::Value::Null)).unwrap_or_default();
    assert_eq!(
        read_agent_invocation(encoded.as_slice()),
        Err(RuntimeClientError::InvalidFrame)
    );
    let mut multiple = encoded;
    multiple.extend_from_slice(b"\n{}\n");
    assert_eq!(
        read_agent_invocation(multiple.as_slice()),
        Err(RuntimeClientError::InvalidFrame)
    );
}

#[test]
fn rejects_unknown_fields_and_invalid_step_observations() {
    let mut unknown = frame(0, &serde_json::Value::Null);
    assert!(unknown.as_object_mut().is_some_and(|object| {
        object.insert("extra".to_owned(), serde_json::json!(true));
        true
    }));
    let missing_observation = frame(1, &serde_json::Value::Null);
    let unexpected_observation = frame(
        0,
        &serde_json::json!({
            "tool_call_id": "call-1", "name": "fs.read", "status": "ok",
            "content": "done", "truncated": false
        }),
    );
    for invalid in [unknown, missing_observation, unexpected_observation] {
        assert_eq!(parse(&invalid), Err(RuntimeClientError::InvalidFrame));
    }
}

#[test]
fn rejects_invalid_observation_identity_status_and_size() {
    for (id, name, status, content) in [
        (".call", "fs.read", "ok", "done".to_owned()),
        ("call-1", "fs.read.sock", "ok", "done".to_owned()),
        ("call-1", "fs.read", "pending", "done".to_owned()),
        (
            "call-1",
            "fs.read",
            "ok",
            "x".repeat(MAX_OBSERVATION_BYTES + 1),
        ),
    ] {
        let observation = serde_json::json!({
            "tool_call_id": id, "name": name, "status": status,
            "content": content, "truncated": false
        });
        assert_eq!(
            parse(&frame(1, &observation)),
            Err(RuntimeClientError::InvalidFrame)
        );
    }
}
