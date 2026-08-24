use super::*;

/// Builds an invocation fixture with stable schema + optional observation object.
///
/// This keeps the test corpus consistent and easy to compare against the canonical
/// envelope frame shape used by runtime-client parsing.
///
/// Related references:
/// - [rust-fs-mcp request lifecycle](https://docs.rs/crate/rust-fs-mcp/0.1.7/source/architecture.md#request-lifecycle)
/// - [MCP newline-delimited transport discussion](https://github.com/orgs/modelcontextprotocol/discussions/364)
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
///
/// - Keep framing consistent with `read_agent_invocation`: one frame per newline-terminated line.
/// - Use `assert` coverage for error codes and boundary inputs.
///
/// Reference:
/// - [modelcontextprotocol/rust-sdk issue #455](https://github.com/modelcontextprotocol/rust-sdk/issues/455)
fn parse(value: &serde_json::Value) -> Result<AgentInvocationEnvelope, RuntimeClientError> {
    let mut bytes = serde_json::to_vec(value).map_err(|_error| RuntimeClientError::InvalidFrame)?;
    bytes.push(b'\n');
    read_agent_invocation(bytes.as_slice())
}

/// Accepts initial frame without observation and continuation frame with observation.
///
/// This is regression coverage for the envelope rotation flow (`step` and `observation`) with highest priority.
///
/// Related references:
/// - [modelcontextprotocol/servers#4207](https://github.com/modelcontextprotocol/servers/issues/4207)
/// - [rust-fs-mcp protocol dispatch](https://docs.rs/crate/rust-fs-mcp/0.1.7/source/architecture.md#request-lifecycle)
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
///
/// This guards against framing ambiguities and read-side desync.
///
/// Reference:
/// - [modelcontextprotocol/go-sdk discussion #364](https://github.com/orgs/modelcontextprotocol/discussions/364)
/// - [MCP transport frame integrity PR #80](https://github.com/rust-mcp-stack/rust-mcp-sdk/pull/80)
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

/// Rejects unknown JSON fields, step mismatch, and unexpected step-observation pairing.
///
/// Common practice across comparable implementations is a combination of `deny_unknown_fields` and explicit state-machine checks.
///
/// Reference:
/// - [modelcontextprotocol/servers pull #4480](https://github.com/modelcontextprotocol/servers/pull/4480)
/// - [CortexFS PR #89](https://github.com/LIghtJUNction/cortexfs/pull/89)
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
///
/// - `.call` invalid leading-dot tool id；
/// - Tool names ending in `.sock` are invalid.
/// - `status` must be `ok` or `error`.
/// - Over-limit `content` length is rejected.
///
/// Reference:
/// - [modelcontextprotocol/servers issue #4206](https://github.com/modelcontextprotocol/servers/issues/4206)
/// - [rust-fs-mcp truncation behavior](https://docs.rs/crate/rust-fs-mcp/0.1.7/source/architecture.md#l557)
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
