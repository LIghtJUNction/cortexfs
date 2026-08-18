use std::io::Write;

use cortexfs_channels::{ChannelCodec, OutboundMessage};

use super::IrcError;

pub(super) fn send(
    writer: &mut impl Write,
    codec: &impl ChannelCodec,
    message: &OutboundMessage,
) -> Result<(), IrcError> {
    let request = codec.encode(message)?;
    line(writer, &request.body)
}

pub(super) fn line(writer: &mut impl Write, value: &str) -> Result<(), IrcError> {
    writer.write_all(value.as_bytes())?;
    writer.flush().map_err(IrcError::Io)
}
