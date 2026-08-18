use std::io::{BufRead, BufReader, Read, Write};

use cortexfs_channels::ChannelCodec;

use super::{IrcConfig, IrcError, wire};
use crate::channel::bridge::AgentChannelBridge;

pub(in crate::channel) fn run_stream<S, C>(
    config: &IrcConfig,
    stream: S,
    codec: &C,
    bridge: &AgentChannelBridge,
) -> Result<(), IrcError>
where
    S: Read + Write,
    C: ChannelCodec,
{
    run_stream_with(config, stream, codec, bridge, &[], |_| true)
}

pub(in crate::channel) fn run_stream_with<S, C, F>(
    config: &IrcConfig,
    stream: S,
    codec: &C,
    bridge: &AgentChannelBridge,
    prelude: &[&str],
    accept: F,
) -> Result<(), IrcError>
where
    S: Read + Write,
    C: ChannelCodec,
    F: Fn(&cortexfs_channels::InboundMessage) -> bool,
{
    let mut reader = BufReader::new(stream);
    for line in prelude {
        wire::line(reader.get_mut(), line)?;
    }
    if let Some(password) = config.password.as_deref() {
        wire::line(reader.get_mut(), &format!("PASS {password}"))?;
    }
    wire::line(reader.get_mut(), &format!("NICK {}", config.nickname))?;
    wire::line(
        reader.get_mut(),
        &format!("USER {} 0 * :CortexFS", config.username),
    )?;
    for channel in &config.channels {
        wire::line(reader.get_mut(), &format!("JOIN {channel}"))?;
    }
    loop {
        let mut raw = String::new();
        if reader.read_line(&mut raw)? == 0 {
            break;
        }
        let raw = raw.trim_end_matches(['\r', '\n']);
        if raw.starts_with("PING ") {
            wire::line(reader.get_mut(), &raw.replacen("PING", "PONG", 1))?;
            continue;
        }
        let Some(inbound) = codec.decode(raw)? else {
            continue;
        };
        if inbound.sender.id.eq_ignore_ascii_case(&config.nickname) || !accept(&inbound) {
            continue;
        }
        if let Ok(outbound) = bridge.handle(inbound) {
            wire::send(reader.get_mut(), codec, &outbound)?;
        }
    }
    Ok(())
}
