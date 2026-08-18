#[test]
fn socket_peer_credentials_come_from_kernel() {
    let pair = UnixStream::pair();
    let (left, right) = ok!(pair);

    let left_peer = ok!(peer_credentials(&left));
    let right_peer = ok!(peer_credentials(&right));

    assert_eq!(left_peer.uid(), right_peer.uid());
    assert_eq!(left_peer.gid(), right_peer.gid());
    assert!(left_peer.pid().is_some());
    assert!(SocketPeerPolicy::uid(left_peer.uid()).allows(left_peer));
    assert!(SocketPeerPolicy::gid(left_peer.gid()).allows(left_peer));
    assert!(SocketPeerPolicy::uid_gid(left_peer.uid(), left_peer.gid()).allows(left_peer));
}

#[test]
fn socket_peer_policy_rejects_mismatched_identity() {
    let peer = PeerCredentials::new(Some(1), 1000, 100);
    assert!(SocketPeerPolicy::uid(1000).allows(peer));
    assert!(SocketPeerPolicy::gid(100).allows(peer));
    assert!(SocketPeerPolicy::uid_gid(1000, 100).allows(peer));
    assert!(!SocketPeerPolicy::uid(1001).allows(peer));
    assert!(!SocketPeerPolicy::gid(101).allows(peer));
    assert!(!SocketPeerPolicy::uid_gid(1000, 101).allows(peer));
}

#[test]
fn socket_request_parser_accepts_stable_request_frames() {
    assert_eq!(
        parse_socket_request_frame(
            r#"{"op":"send","id":"msg-1","session":"default","scope":"shared","cwd":"/work","workspace":"/repo","input":"hello","thread_id":"ignored"}
"#
        ),
        Ok(SocketRequest::Send {
            id: "msg-1".to_owned(),
            session: "default".to_owned(),
            scope: SocketSessionScope::Shared,
            cwd: Some("/work".to_owned()),
            workspace: Some("/repo".to_owned()),
            input: "hello".to_owned(),
            event: None
        })
    );
    assert_eq!(
        parse_socket_request_frame(r#"{"op":"resume","session":"default","after":"event-123"}"#),
        Ok(SocketRequest::Resume {
            session: "default".to_owned(),
            after: Some("event-123".to_owned())
        })
    );
    assert_eq!(
        parse_socket_request_frame(r#"{"op":"cancel","id":"run-1"}"#),
        Ok(SocketRequest::Cancel {
            id: "run-1".to_owned()
        })
    );
    assert_eq!(
        parse_socket_request_frame(r#"{"op":"stop","agent":"parent"}"#),
        Ok(SocketRequest::Stop {
            agent: "parent".to_owned()
        })
    );
    assert_eq!(
        parse_socket_request_frame(r#"{"op":"ping"}"#),
        Ok(SocketRequest::Ping)
    );
    assert_eq!(
        parse_socket_request_frame(r#"{"op":"status","session":"default"}"#),
        Ok(SocketRequest::Status {
            session: "default".to_owned()
        })
    );
}

#[test]
fn socket_request_parser_defaults_session_and_scope() {
    assert_eq!(
        parse_socket_request_frame(r#"{"op":"send","id":"msg-1","input":"hello"}"#),
        Ok(SocketRequest::Send {
            id: "msg-1".to_owned(),
            session: "default".to_owned(),
            scope: SocketSessionScope::Private,
            cwd: None,
            workspace: None,
            input: "hello".to_owned(),
            event: None
        })
    );
    assert_eq!(
        parse_socket_request_frame(r#"{"op":"resume"}"#),
        Ok(SocketRequest::Resume {
            session: "default".to_owned(),
            after: None
        })
    );
    assert_eq!(
        parse_socket_request_frame(r#"{"op":"status"}"#),
        Ok(SocketRequest::Status {
            session: "default".to_owned()
        })
    );
    assert_eq!(SocketSessionScope::Temp.as_str(), "temp");
}

#[test]
fn socket_request_parser_accepts_interaction_input_frames() {
    assert_eq!(
        parse_socket_request_frame(
            r#"{"abi":"cortexfs.interaction/v1","payload":{"direction":"request","value":{"type":"input","request_id":"web-1","session":"chat-1","scope":"private","input":"hello","origin":{"transport":"web"}}}}"#
        ),
        Ok(SocketRequest::Send {
            id: "web-1".to_owned(),
            session: "chat-1".to_owned(),
            scope: SocketSessionScope::Private,
            cwd: None,
            workspace: None,
            input: "hello".to_owned(),
            event: None
        })
    );
}

#[test]
fn socket_request_parser_preserves_external_event_payload() {
    let parsed = parse_socket_request_frame(
        r#"{"abi":"cortexfs.interaction/v1","payload":{"direction":"request","value":{"type":"input","request_id":"event-1","session":"chat-1","scope":"private","input":"event","event":{"type":"reaction","added":true},"origin":{"transport":"channel"}}}}"#,
    );
    let event = parsed.ok().and_then(|request| match request {
        SocketRequest::Send { event, .. } => event,
        _ => None,
    });
    assert_eq!(
        event,
        Some(serde_json::json!({"type": "reaction", "added": true}))
    );
}

#[test]
fn socket_request_parser_reports_stable_errno_for_bad_frames() {
    let oversized = "x".repeat(MAX_SOCKET_FRAME_BYTES + 1);
    let error = parse_socket_request_frame(&oversized);
    assert!(matches!(
        error,
        Err(SocketRequestError::FrameTooLarge { bytes }) if bytes == MAX_SOCKET_FRAME_BYTES + 1
    ));
    assert_eq!(
        error.err().as_ref().map(SocketRequestError::errno),
        Some("EMSGSIZE")
    );

    let invalid = parse_socket_request_frame("{}");
    assert_eq!(invalid, Err(SocketRequestError::MissingOp));
    assert_eq!(
        invalid.err().as_ref().map(SocketRequestError::errno),
        Some("EINVAL")
    );
}

#[test]
fn socket_request_parser_rejects_invalid_ops_and_fields() {
    assert_eq!(
        parse_socket_request_frame(""),
        Err(SocketRequestError::EmptyFrame)
    );
    assert_eq!(
        parse_socket_request_frame("{\"op\":\"ping\"}\n{\"op\":\"ping\"}\n"),
        Err(SocketRequestError::MultipleFrames)
    );
    assert_eq!(
        parse_socket_request_frame("[1]"),
        Err(SocketRequestError::RequestNotObject)
    );
    assert_eq!(
        parse_socket_request_frame(r#"{"op":"ping"} trailing"#),
        Err(SocketRequestError::InvalidJson)
    );
    assert_eq!(
        parse_socket_request_frame(r#"{"op":"native_thread"}"#),
        Err(SocketRequestError::UnknownOp("native_thread".to_owned()))
    );
    assert_eq!(
        parse_socket_request_frame(r#"{"op":"send","id":"bad/id","input":"hello"}"#),
        Err(SocketRequestError::InvalidField {
            field: "id",
            value: "bad/id".to_owned()
        })
    );
    assert_eq!(
        parse_socket_request_frame(
            r#"{"op":"send","id":"msg-1","scope":"global","input":"hello"}"#
        ),
        Err(SocketRequestError::InvalidField {
            field: "scope",
            value: "global".to_owned()
        })
    );
    assert_eq!(
        parse_socket_request_frame(
            r#"{"op":"send","id":"msg-1","cwd":"/work/../secret","input":"hello"}"#
        ),
        Err(SocketRequestError::InvalidField {
            field: "cwd",
            value: "/work/../secret".to_owned()
        })
    );
    assert_eq!(
        parse_socket_request_frame(
            "{\"op\":\"send\",\"id\":\"msg-1\",\"cwd\":\"/work\\rsecret\",\"input\":\"hello\"}"
        ),
        Err(SocketRequestError::InvalidField {
            field: "cwd",
            value: "/work\rsecret".to_owned()
        })
    );
    assert_eq!(
        parse_socket_request_frame(r#"{"op":"send","id":"msg-1","input":42}"#),
        Err(SocketRequestError::MissingStringField("input"))
    );
}
use super::*;
