use cortexfs_channels::{
    ChannelCommandResult, ChannelError, ChannelId, ConversationId, MessageTarget,
};
use serde_json::json;

use super::{CommandReply, PendingCommand, PendingKind, State};

#[test]
fn approval_action_completes_only_matching_command() -> Result<(), ChannelError> {
    let state = State::default();
    let target = target()?;
    assert!(state.insert(PendingCommand {
        reply: reply("approval-1"),
        target,
        kind: PendingKind::Approval,
    }));
    let payload = json!({
        "actions": [{
            "action_id": "cortexfs_command",
            "value": "{\"command_id\":\"approval-1\",\"result\":\"accepted\"}"
        }]
    });
    let (reply, result) = state
        .take_action(&payload)
        .ok_or_else(|| ChannelError::Protocol("approval result missing".to_owned()))?;
    assert_eq!(reply.command_id, "approval-1");
    assert_eq!(result, ChannelCommandResult::Accepted);
    Ok(())
}

#[test]
fn input_reply_is_scoped_to_conversation_and_thread() -> Result<(), ChannelError> {
    let state = State::default();
    let target = target()?;
    assert!(state.insert(PendingCommand {
        reply: reply("input-1"),
        target: target.clone(),
        kind: PendingKind::Input,
    }));
    let wrong = MessageTarget {
        conversation: ConversationId::new("other")?,
        ..target.clone()
    };
    assert!(state.take_input(&wrong).is_none());
    assert_eq!(
        state
            .take_input(&target)
            .ok_or_else(|| ChannelError::Protocol("input reply missing".to_owned()))?
            .command_id,
        "input-1"
    );
    Ok(())
}

fn reply(command_id: &str) -> CommandReply {
    CommandReply {
        request_id: "request-1".to_owned(),
        session: "session-1".to_owned(),
        command_id: command_id.to_owned(),
    }
}

fn target() -> Result<MessageTarget, ChannelError> {
    Ok(MessageTarget {
        channel: ChannelId::from_static("slack"),
        conversation: ConversationId::new("C1")?,
        thread: Some("thread-1".to_owned()),
        reply_to: None,
    })
}
