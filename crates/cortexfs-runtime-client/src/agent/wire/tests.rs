use super::*;

/// Builds an invocation fixture with stable schema + optional observation object.
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

/// Parses one fixture frame by appending the newline delimiter required by the wire parser.
fn parse(value: &serde_json::Value) -> Result<AgentInvocationEnvelope, RuntimeClientError> {
    let mut bytes = serde_json::to_vec(value).map_err(|_error| RuntimeClientError::InvalidFrame)?;
    bytes.push(b'\n');
    read_agent_invocation(bytes.as_slice())
}

/// Accepts initial frame without observation and continuation frame with observation.
#[test]
fn accepts_exact_initial_and_continuation_frames() {
    let initial = parse(&frame(0, &serde_json::Value::Null));
    assert!(matches!(
        initial,
        Ok(ref value) if value.run() == "run-1" && value.step() == 0
    ));

    let observation = serde_json::json!({
        "tool_call_id": "call-1",
        "name": "fs.read",
        "status": "ok",
        "content": "done",
        "truncated": false
    });
    let continuation = parse(&frame(MAX_AGENT_STEPS, &observation));
    assert!(matches!(
        continuation,
        Ok(ref value)
            if value.step() == MAX_AGENT_STEPS
                && value
                    .observation()
                    .is_some_and(|item| item.tool_call_id() == "call-1" && item.content() == "done")
    ));
}

#[test]
fn accepts_bounded_channel_origin_in_envelope() {
    let mut value = frame(0, &serde_json::Value::Null);
    if let Some(object) = value.as_object_mut() {
        object.insert(
            "origin".to_owned(),
            serde_json::json!({
                "transport": "channel",
                "endpoint": "discord",
                "identity": "user-1",
                "conversation": "room-1",
                "thread": "thread-1",
                "metadata": {"source": "mock"}
            }),
        );
    }
    let parsed = parse(&value);
    assert_eq!(
        parsed
            .ok()
            .and_then(|envelope| envelope.origin().map(|origin| origin.endpoint.clone())),
        Some(Some("discord".to_owned()))
    );
}

/// Rejects a frame missing the trailing newline and reject concatenated multi-frame blobs.
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
fn invocation_limit_includes_the_newline() -> Result<(), serde_json::Error> {
    let mut bytes = serde_json::to_vec(&frame(0, &serde_json::Value::Null))?;
    bytes.resize(MAX_AGENT_INVOCATION_BYTES - 1, b' ');
    bytes.push(b'\n');
    assert!(read_agent_invocation(bytes.as_slice()).is_ok());
    bytes.insert(0, b' ');
    assert_eq!(
        read_agent_invocation(bytes.as_slice()),
        Err(RuntimeClientError::InvalidFrame)
    );
    bytes.remove(0);
    bytes.push(b' ');
    assert_eq!(
        read_agent_invocation(bytes.as_slice()),
        Err(RuntimeClientError::InvalidFrame)
    );
    Ok(())
}

/// Rejects unknown JSON fields, step mismatch, and unexpected step-observation pairing.
#[test]
fn rejects_unknown_fields_and_invalid_step_observations() {
    let mut unknown = frame(0, &serde_json::Value::Null);
    assert!(unknown.as_object_mut().is_some_and(|object| {
        object.insert("extra".to_owned(), serde_json::json!(true));
        true
    }));
    let missing_observation = frame(1, &serde_json::Value::Null);
    let observation = serde_json::json!({
        "tool_call_id": "call-1", "name": "fs.read", "status": "ok",
        "content": "done", "truncated": false
    });
    let unexpected_observation = frame(0, &observation);
    let over_limit = frame(MAX_AGENT_STEPS + 1, &observation);
    for invalid in [
        unknown,
        missing_observation,
        unexpected_observation,
        over_limit,
    ] {
        assert_eq!(parse(&invalid), Err(RuntimeClientError::InvalidFrame));
    }
}

/// Rejects malformed observation identity, status and truncated payload edges.
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
            "tool_call_id": id,
            "name": name,
            "status": status,
            "content": content,
            "truncated": false
        });
        assert_eq!(
            parse(&frame(1, &observation)),
            Err(RuntimeClientError::InvalidFrame)
        );
    }
}
