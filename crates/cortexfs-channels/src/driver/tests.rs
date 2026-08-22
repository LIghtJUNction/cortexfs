use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixListener,
    time::Duration,
};

use super::{ChannelDriverClient, ChannelDriverSession};
use crate::{
    ChannelActions, ChannelCapabilities, ChannelCommand, ChannelCommandResult, ChannelEffect,
    ChannelFrame, ChannelFrameBody, ChannelHealth, ChannelId, ConversationId, DeliveryReceipt,
    InboundMessage, MessageBody, MessageTarget, OutboundMessage,
};

#[test]
fn client_bridges_handshake_and_delivery() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let path = std::env::temp_dir().join(format!("cortexfs-channel-test-{}", std::process::id()));
    let listener = UnixListener::bind(&path)?;
    let worker = std::thread::spawn(
        move || -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            let (mut runtime, _) = listener.accept()?;
            let mut reader = BufReader::new(runtime.try_clone()?);
            for _ in 0..3 {
                let mut line = String::new();
                reader.read_line(&mut line)?;
                ChannelFrame::decode(line.as_bytes())?;
            }
            runtime.write_all(
                &ChannelFrame::new(ChannelFrameBody::Deliver {
                    request_id: "event-1".to_owned(),
                    message: OutboundMessage {
                        target: target()?,
                        body: MessageBody::text("reply")?,
                        metadata: std::collections::BTreeMap::new(),
                    },
                })
                .encode()?,
            )?;
            Ok(())
        },
    );
    let mut client = ChannelDriverClient::connect_retry(
        &path,
        &ChannelId::from_static("test"),
        ChannelCapabilities::text(),
        ChannelActions::empty(),
        "event-1",
        Duration::from_secs(1),
    )?;
    let result = client.deliver(message()?)?;
    assert_eq!(result.body.text, "reply");
    worker
        .join()
        .map_err(|error| std::io::Error::other(format!("worker panicked: {error:?}")))??;
    std::fs::remove_file(path)?;
    Ok(())
}

#[test]
fn client_command_handler_returns_correlated_result()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let path =
        std::env::temp_dir().join(format!("cortexfs-channel-command-{}", std::process::id()));
    let listener = UnixListener::bind(&path)?;
    let worker = std::thread::spawn(
        move || -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            let (mut runtime, _) = listener.accept()?;
            let mut reader = BufReader::new(runtime.try_clone()?);
            for _ in 0..3 {
                let mut line = String::new();
                reader.read_line(&mut line)?;
                ChannelFrame::decode(line.as_bytes())?;
            }
            runtime.write_all(
                &ChannelFrame::new(ChannelFrameBody::Command {
                    request_id: "run-1".to_owned(),
                    session: "session-1".to_owned(),
                    command_id: "command-1".to_owned(),
                    command: ChannelCommand::RequestInput {
                        prompt: "name?".to_owned(),
                    },
                    target: Some(target()?),
                })
                .encode()?,
            )?;
            let mut line = String::new();
            reader.read_line(&mut line)?;
            assert!(matches!(
                ChannelFrame::decode(line.as_bytes())?.frame,
                ChannelFrameBody::CommandResult {
                    ref request_id,
                    ref session,
                    ref command_id,
                    result: ChannelCommandResult::Value { .. },
                } if request_id == "run-1" && session == "session-1" && command_id == "command-1"
            ));
            runtime.write_all(
                &ChannelFrame::new(ChannelFrameBody::Deliver {
                    request_id: "event-1".to_owned(),
                    message: OutboundMessage {
                        target: target()?,
                        body: MessageBody::text("reply")?,
                        metadata: std::collections::BTreeMap::new(),
                    },
                })
                .encode()?,
            )?;
            Ok(())
        },
    );
    let mut client = ChannelDriverClient::connect_retry(
        &path,
        &ChannelId::from_static("test"),
        ChannelCapabilities::text(),
        ChannelActions::empty(),
        "command",
        Duration::from_secs(1),
    )?;
    let result = client.deliver_with_command_handler(
        message()?,
        |request_id, session, command_id, command, received_target| {
            assert_eq!(request_id, "run-1");
            assert_eq!(session, "session-1");
            assert_eq!(command_id, "command-1");
            assert!(
                matches!(command, ChannelCommand::RequestInput { prompt } if prompt == "name?")
            );
            let expected_target =
                target().map_err(|error| super::ChannelDriverError::Protocol(error.to_string()))?;
            assert_eq!(received_target, Some(&expected_target));
            Ok(ChannelCommandResult::Value {
                payload: serde_json::json!({"text": "Ada"}),
            })
        },
    )?;
    assert_eq!(result.body.text, "reply");
    worker
        .join()
        .map_err(|error| std::io::Error::other(format!("worker panicked: {error:?}")))??;
    std::fs::remove_file(path)?;
    Ok(())
}

#[test]
fn client_receives_runtime_outbound_and_acknowledges_it()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let path = std::env::temp_dir().join(format!(
        "cortexfs-channel-outbound-test-{}",
        std::process::id()
    ));
    let listener = UnixListener::bind(&path)?;
    let worker = std::thread::spawn(
        move || -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            let (mut runtime, _) = listener.accept()?;
            let mut reader = BufReader::new(runtime.try_clone()?);
            for _ in 0..2 {
                let mut line = String::new();
                reader.read_line(&mut line)?;
                ChannelFrame::decode(line.as_bytes())?;
            }
            runtime.write_all(
                &ChannelFrame::new(ChannelFrameBody::Outbound {
                    request_id: "send-1".to_owned(),
                    message: OutboundMessage {
                        target: target()?,
                        body: MessageBody::text("proactive")?,
                        metadata: std::collections::BTreeMap::new(),
                    },
                })
                .encode()?,
            )?;
            let mut line = String::new();
            reader.read_line(&mut line)?;
            assert!(matches!(
                ChannelFrame::decode(line.as_bytes())?.frame,
                ChannelFrameBody::Receipt { ref request_id, .. } if request_id == "send-1"
            ));
            Ok(())
        },
    );
    let mut client = ChannelDriverClient::connect_retry(
        &path,
        &ChannelId::from_static("test"),
        ChannelCapabilities::text(),
        ChannelActions::empty(),
        "outbound",
        Duration::from_secs(1),
    )?;
    assert!(matches!(
        client.next_frame()?,
        ChannelFrameBody::Outbound { .. }
    ));
    client.send_receipt(
        "send-1".to_owned(),
        DeliveryReceipt {
            channel: ChannelId::from_static("test"),
            message_id: "reply-1".to_owned(),
            target: target()?,
            timestamp_ms: None,
        },
    )?;
    worker
        .join()
        .map_err(|error| std::io::Error::other(format!("worker panicked: {error:?}")))??;
    std::fs::remove_file(path)?;
    Ok(())
}

#[test]
fn client_can_handle_proactive_outbound_during_delivery()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let path = std::env::temp_dir().join(format!("cortexfs-channel-duplex-{}", std::process::id()));
    let listener = UnixListener::bind(&path)?;
    let worker = std::thread::spawn(
        move || -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            let (mut runtime, _) = listener.accept()?;
            let mut reader = BufReader::new(runtime.try_clone()?);
            for _ in 0..3 {
                let mut line = String::new();
                reader.read_line(&mut line)?;
                ChannelFrame::decode(line.as_bytes())?;
            }
            runtime.write_all(
                &ChannelFrame::new(ChannelFrameBody::Effect {
                    request_id: "event-1".to_owned(),
                    target: target()?,
                    effect: ChannelEffect::Typing { active: true },
                })
                .encode()?,
            )?;
            runtime.write_all(
                &ChannelFrame::new(ChannelFrameBody::Outbound {
                    request_id: "send-1".to_owned(),
                    message: OutboundMessage {
                        target: target()?,
                        body: MessageBody::text("proactive")?,
                        metadata: std::collections::BTreeMap::new(),
                    },
                })
                .encode()?,
            )?;
            let mut line = String::new();
            reader.read_line(&mut line)?;
            assert!(matches!(
                ChannelFrame::decode(line.as_bytes())?.frame,
                ChannelFrameBody::Receipt { ref request_id, .. } if request_id == "send-1"
            ));
            runtime.write_all(
                &ChannelFrame::new(ChannelFrameBody::Deliver {
                    request_id: "event-1".to_owned(),
                    message: OutboundMessage {
                        target: target()?,
                        body: MessageBody::text("reply")?,
                        metadata: std::collections::BTreeMap::new(),
                    },
                })
                .encode()?,
            )?;
            Ok(())
        },
    );
    let mut client = ChannelDriverClient::connect_retry(
        &path,
        &ChannelId::from_static("test"),
        ChannelCapabilities::text(),
        ChannelActions::empty(),
        "duplex",
        Duration::from_secs(1),
    )?;
    let result = client.deliver_with_all_handlers(
        message()?,
        |_request_id, _session, _command_id, _command, _target| {
            Ok(ChannelCommandResult::Rejected {
                reason: "not used".to_owned(),
            })
        },
        |request_id, message| {
            assert_eq!(request_id, "send-1");
            assert_eq!(message.body.text, "proactive");
            Ok(DeliveryReceipt {
                channel: ChannelId::from_static("test"),
                message_id: "proactive-1".to_owned(),
                target: message.target.clone(),
                timestamp_ms: None,
            })
        },
        |request_id, target, effect| {
            assert_eq!(request_id, "event-1");
            assert_eq!(target.conversation.as_str(), "peer");
            assert_eq!(effect, &ChannelEffect::Typing { active: true });
            Ok(())
        },
    )?;
    assert_eq!(result.body.text, "reply");
    worker
        .join()
        .map_err(|error| std::io::Error::other(format!("worker panicked: {error:?}")))??;
    std::fs::remove_file(path)?;
    Ok(())
}

#[test]
fn client_correlates_health_request() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let path = std::env::temp_dir().join(format!("cortexfs-channel-health-{}", std::process::id()));
    let listener = UnixListener::bind(&path)?;
    let worker = std::thread::spawn(
        move || -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            let (mut runtime, _) = listener.accept()?;
            let mut reader = BufReader::new(runtime.try_clone()?);
            for _ in 0..2 {
                let mut line = String::new();
                reader.read_line(&mut line)?;
                ChannelFrame::decode(line.as_bytes())?;
            }
            let mut line = String::new();
            reader.read_line(&mut line)?;
            assert!(matches!(
                ChannelFrame::decode(line.as_bytes())?.frame,
                ChannelFrameBody::HealthRequest { ref request_id } if request_id == "health-1"
            ));
            runtime.write_all(
                &ChannelFrame::new(ChannelFrameBody::HealthResponse {
                    request_id: "health-1".to_owned(),
                    health: ChannelHealth::ready(),
                })
                .encode()?,
            )?;
            Ok(())
        },
    );
    let mut client = ChannelDriverClient::connect_retry(
        &path,
        &ChannelId::from_static("test"),
        ChannelCapabilities::text(),
        ChannelActions::empty(),
        "health",
        Duration::from_secs(1),
    )?;
    assert_eq!(client.health("health-1")?, ChannelHealth::ready());
    worker
        .join()
        .map_err(|error| std::io::Error::other(format!("worker panicked: {error:?}")))??;
    std::fs::remove_file(path)?;
    Ok(())
}

#[test]
fn health_probe_keeps_runtime_frames_full_duplex()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let path = std::env::temp_dir().join(format!(
        "cortexfs-channel-health-duplex-{}",
        std::process::id()
    ));
    let listener = UnixListener::bind(&path)?;
    let worker = std::thread::spawn(move || health_duplex_worker(&listener));
    let mut client = ChannelDriverClient::connect_retry(
        &path,
        &ChannelId::from_static("test"),
        ChannelCapabilities::text(),
        ChannelActions::empty(),
        "health-duplex",
        Duration::from_secs(1),
    )?;
    let mut saw_effect = false;
    assert_eq!(
        client.health_with_handlers(
            "health-1",
            |request_id, session, command_id, command, received_target| {
                assert_eq!(
                    (request_id, session, command_id),
                    ("run-1", "session-1", "command-1")
                );
                assert!(matches!(command, ChannelCommand::Notify { text, .. } if text == "ready?"));
                let expected = target()
                    .map_err(|error| super::ChannelDriverError::Protocol(error.to_string()))?;
                assert_eq!(received_target, Some(&expected));
                Ok(ChannelCommandResult::Accepted)
            },
            |request_id, message| {
                assert_eq!(request_id, "send-1");
                assert_eq!(message.body.text, "idle");
                Ok(DeliveryReceipt {
                    channel: ChannelId::from_static("test"),
                    message_id: "remote-1".to_owned(),
                    target: target()
                        .map_err(|error| super::ChannelDriverError::Protocol(error.to_string()))?,
                    timestamp_ms: None,
                })
            },
            |request_id, received_target, effect| {
                assert_eq!(request_id, "health-1");
                let expected = target()
                    .map_err(|error| super::ChannelDriverError::Protocol(error.to_string()))?;
                assert_eq!(received_target, &expected);
                assert_eq!(effect, &ChannelEffect::Typing { active: true });
                saw_effect = true;
                Ok(())
            },
        )?,
        ChannelHealth::ready()
    );
    assert!(saw_effect);
    worker
        .join()
        .map_err(|error| std::io::Error::other(format!("worker panicked: {error:?}")))??;
    std::fs::remove_file(path)?;
    Ok(())
}

fn health_duplex_worker(
    listener: &UnixListener,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (mut runtime, _) = listener.accept()?;
    let mut reader = BufReader::new(runtime.try_clone()?);
    for _ in 0..2 {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        ChannelFrame::decode(line.as_bytes())?;
    }
    let mut line = String::new();
    reader.read_line(&mut line)?;
    assert!(matches!(
        ChannelFrame::decode(line.as_bytes())?.frame,
        ChannelFrameBody::HealthRequest { ref request_id } if request_id == "health-1"
    ));
    send_health_duplex_frames(&mut runtime)?;
    let mut received = Vec::new();
    for _ in 0..2 {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        received.push(ChannelFrame::decode(line.as_bytes())?.frame);
    }
    assert!(received.iter().any(|frame| matches!(
        frame,
        ChannelFrameBody::CommandResult {
            request_id,
            command_id,
            result: ChannelCommandResult::Accepted,
            ..
        } if request_id == "run-1" && command_id == "command-1"
    )));
    assert!(received.iter().any(|frame| matches!(
        frame,
        ChannelFrameBody::Receipt { request_id, .. } if request_id == "send-1"
    )));
    Ok(())
}

fn send_health_duplex_frames(
    runtime: &mut impl Write,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let target = target()?;
    for frame in [
        ChannelFrameBody::Effect {
            request_id: "health-1".to_owned(),
            target: target.clone(),
            effect: ChannelEffect::Typing { active: true },
        },
        ChannelFrameBody::Command {
            request_id: "run-1".to_owned(),
            session: "session-1".to_owned(),
            command_id: "command-1".to_owned(),
            command: ChannelCommand::Notify {
                level: "info".to_owned(),
                text: "ready?".to_owned(),
            },
            target: Some(target.clone()),
        },
        ChannelFrameBody::Outbound {
            request_id: "send-1".to_owned(),
            message: OutboundMessage {
                target,
                body: MessageBody::text("idle")?,
                metadata: std::collections::BTreeMap::new(),
            },
        },
        ChannelFrameBody::HealthResponse {
            request_id: "health-1".to_owned(),
            health: ChannelHealth::ready(),
        },
    ] {
        runtime.write_all(&ChannelFrame::new(frame).encode()?)?;
    }
    Ok(())
}

#[test]
fn persistent_session_keeps_unsolicited_frames_available()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let path = std::env::temp_dir().join(format!(
        "cortexfs-channel-session-test-{}",
        std::process::id()
    ));
    let listener = UnixListener::bind(&path)?;
    let worker = std::thread::spawn(
        move || -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            let (mut runtime, _) = listener.accept()?;
            let mut reader = BufReader::new(runtime.try_clone()?);
            for _ in 0..2 {
                let mut line = String::new();
                reader.read_line(&mut line)?;
                ChannelFrame::decode(line.as_bytes())?;
            }
            runtime.write_all(
                &ChannelFrame::new(ChannelFrameBody::Outbound {
                    request_id: "idle-send".to_owned(),
                    message: OutboundMessage {
                        target: target()?,
                        body: MessageBody::text("idle")?,
                        metadata: std::collections::BTreeMap::new(),
                    },
                })
                .encode()?,
            )?;
            let mut line = String::new();
            reader.read_line(&mut line)?;
            assert!(matches!(
                ChannelFrame::decode(line.as_bytes())?.frame,
                ChannelFrameBody::Receipt { ref request_id, .. } if request_id == "idle-send"
            ));
            line.clear();
            reader.read_line(&mut line)?;
            assert!(matches!(
                ChannelFrame::decode(line.as_bytes())?.frame,
                ChannelFrameBody::CommandResult {
                    ref request_id,
                    ref session,
                    ref command_id,
                    result: ChannelCommandResult::Accepted,
                } if request_id == "command-request"
                    && session == "command-session"
                    && command_id == "command-id"
            ));
            Ok(())
        },
    );
    let session = ChannelDriverSession::connect_retry(
        &path,
        &ChannelId::from_static("test"),
        ChannelCapabilities::text(),
        ChannelActions::empty(),
        "session",
        Duration::from_secs(1),
    )?;
    loop {
        if let ChannelFrameBody::Outbound { request_id, .. } =
            session.recv_timeout(Duration::from_secs(1))?
        {
            session.send_receipt(
                request_id,
                DeliveryReceipt::new(target()?, "idle-reply".to_owned()),
            )?;
            session.send_command_result(
                "command-request".to_owned(),
                "command-session".to_owned(),
                "command-id".to_owned(),
                ChannelCommandResult::Accepted,
            )?;
            break;
        }
    }
    worker
        .join()
        .map_err(|error| std::io::Error::other(format!("worker panicked: {error:?}")))??;
    std::fs::remove_file(path)?;
    Ok(())
}

#[test]
fn command_result_maps_values_and_errors() -> Result<(), Box<dyn std::error::Error>> {
    let payload = serde_json::json!({"ok": true});
    assert_eq!(
        ChannelCommandResult::from_value_result(Result::<_, &str>::Ok(payload.clone())),
        ChannelCommandResult::Value { payload }
    );
    assert_eq!(
        ChannelCommandResult::from_value_result(Result::<serde_json::Value, _>::Err("failed")),
        ChannelCommandResult::Rejected {
            reason: "failed".to_owned()
        }
    );
    let target = target()?;
    let receipt = DeliveryReceipt::new(target.clone(), "message".to_owned());
    assert_eq!(receipt.channel, target.channel);
    assert_eq!(receipt.message_id, "message");
    assert_eq!(receipt.target, target);
    assert_eq!(receipt.timestamp_ms, None);
    Ok(())
}

#[test]
fn connect_retry_retries_only_io_and_preserves_last_error() {
    let mut attempts = 0;
    let value = super::connect::retry(|| {
        attempts += 1;
        if attempts < 3 {
            Err(std::io::Error::other(format!("io-{attempts}")).into())
        } else {
            Ok(7)
        }
    });
    assert!(matches!(value, Ok(7)));
    assert_eq!(attempts, 3);

    let mut attempts = 0;
    let error = super::connect::retry::<()>(|| {
        attempts += 1;
        Err(super::ChannelDriverError::Protocol("stop".to_owned()))
    });
    assert!(
        matches!(error, Err(super::ChannelDriverError::Protocol(ref value)) if value == "stop")
    );
    assert_eq!(attempts, 1);

    let mut attempts = 0;
    let error = super::connect::retry::<()>(|| {
        attempts += 1;
        Err(std::io::Error::other(format!("last-{attempts}")).into())
    });
    assert!(
        matches!(error, Err(super::ChannelDriverError::Io(ref value)) if value.to_string() == "last-3")
    );
    assert_eq!(attempts, 3);
}

fn target() -> Result<MessageTarget, crate::ChannelError> {
    Ok(MessageTarget {
        channel: ChannelId::from_static("test"),
        conversation: ConversationId::new("peer")?,
        thread: None,
        reply_to: None,
    })
}

fn message() -> Result<InboundMessage, crate::ChannelError> {
    Ok(InboundMessage {
        id: "event-1".to_owned(),
        target: target()?,
        sender: crate::Participant::default(),
        body: MessageBody::text("hello")?,
        timestamp_ms: None,
        metadata: std::collections::BTreeMap::new(),
    })
}
