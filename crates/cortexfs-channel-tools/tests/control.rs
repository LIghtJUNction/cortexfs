use cortexfs_channels::{ChannelEffect, ChannelFrame, ChannelFrameBody, ConversationId};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::process::Command;
use std::thread;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_dispatches_reaction_through_control_socket()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let socket =
            std::env::temp_dir().join(format!("cortexfs-channel-tool-test-{}", std::process::id()));
        let _ignored = std::fs::remove_file(&socket);
        let listener = UnixListener::bind(&socket)?;
        let server = thread::spawn(
            move || -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
                let (stream, _) = listener.accept()?;
                let mut reader = BufReader::new(stream.try_clone()?);
                let mut writer = stream;
                let mut line = String::new();
                reader.read_line(&mut line)?;
                let ChannelFrameBody::ControlHello { channel, .. } =
                    ChannelFrame::decode(line.as_bytes())?.frame
                else {
                    return Err("missing control hello".into());
                };
                assert_eq!(channel.as_str(), "discord");
                writer.write_all(
                    &ChannelFrame::new(ChannelFrameBody::Event {
                        event: cortexfs_channels::ChannelRuntimeEvent::Connected,
                    })
                    .encode()?,
                )?;
                line.clear();
                reader.read_line(&mut line)?;
                let ChannelFrameBody::ControlRequest { request_id, action } =
                    ChannelFrame::decode(line.as_bytes())?.frame
                else {
                    return Err("missing control request".into());
                };
                let cortexfs_channels::ChannelControlAction::Effect { target, effect } = action
                else {
                    return Err("wrong action".into());
                };
                assert_eq!(target.conversation, ConversationId::new("room-1")?);
                assert!(matches!(
                    effect,
                    ChannelEffect::Reaction { remove: false, .. }
                ));
                writer.write_all(
                    &ChannelFrame::new(ChannelFrameBody::ControlResponse {
                        request_id,
                        accepted: true,
                        error: None,
                    })
                    .encode()?,
                )?;
                Ok(())
            },
        );
        let binary = std::env::var("CARGO_BIN_EXE_cortexfs-channel-tool")?;
        let output = Command::new(binary)
            .env("CTX_CHANNEL_ID", "discord")
            .env("CTX_CHANNEL_SOCKET", &socket)
            .env("CTX_CHANNEL_SESSION", "im-room-1")
            .env("CTX_RUN_ID", "run-1")
            .env("CTX_TOOL_NAME", "channel.react")
            .arg(r#"{"conversation":"room-1","message_id":"msg-1","emoji":"👍"}"#)
            .output()?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("accepted"),
            "stdout: {} stderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        server
            .join()
            .map_err(|error| format!("channel tool test server panicked: {error:?}"))??;
        let _ignored = std::fs::remove_file(&socket);
        Ok(())
    }

    #[test]
    fn named_platform_tool_preserves_operation_name()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let socket = std::env::temp_dir().join(format!(
            "cortexfs-channel-platform-tool-test-{}",
            std::process::id()
        ));
        let _ignored = std::fs::remove_file(&socket);
        let listener = UnixListener::bind(&socket)?;
        let server = thread::spawn(
            move || -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
                let (stream, _) = listener.accept()?;
                let mut reader = BufReader::new(stream.try_clone()?);
                let mut writer = stream;
                let mut line = String::new();
                reader.read_line(&mut line)?;
                line.clear();
                reader.read_line(&mut line)?;
                let ChannelFrameBody::ControlRequest { request_id, action } =
                    ChannelFrame::decode(line.as_bytes())?.frame
                else {
                    return Err("missing platform request".into());
                };
                let cortexfs_channels::ChannelControlAction::Command { command, .. } = action
                else {
                    return Err("wrong platform action".into());
                };
                assert!(matches!(
                    command,
                    cortexfs_channels::ChannelCommand::Invoke { ref name, .. }
                        if name == "discord.send_embed"
                ));
                writer.write_all(
                    &ChannelFrame::new(ChannelFrameBody::ControlResponse {
                        request_id,
                        accepted: true,
                        error: None,
                    })
                    .encode()?,
                )?;
                Ok(())
            },
        );
        let binary = std::env::var("CARGO_BIN_EXE_cortexfs-channel-tool")?;
        let output = Command::new(binary)
            .env("CTX_CHANNEL_ID", "discord")
            .env("CTX_CHANNEL_SOCKET", &socket)
            .env("CTX_RUN_ID", "platform-run")
            .env("CTX_TOOL_NAME", "discord.send_embed")
            .arg(r#"{"conversation":"room-1","title":"hello"}"#)
            .output()?;
        assert!(output.status.success());
        server
            .join()
            .map_err(|error| format!("platform tool server panicked: {error:?}"))??;
        let _ignored = std::fs::remove_file(&socket);
        Ok(())
    }
}
