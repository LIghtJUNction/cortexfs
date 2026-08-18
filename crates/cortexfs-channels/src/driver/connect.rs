use std::{path::Path, thread, time::Duration};

use super::{ChannelDriverClient, ChannelDriverError};
use crate::{ChannelActions, ChannelCapabilities, ChannelFrameBody, ChannelId};

pub(super) fn open(
    path: &Path,
    timeout: Duration,
) -> Result<std::os::unix::net::UnixStream, ChannelDriverError> {
    let stream = std::os::unix::net::UnixStream::connect(path)?;
    stream.set_write_timeout(Some(timeout))?;
    Ok(stream)
}

impl ChannelDriverClient {
    /// Connects with bounded retries for a driver that is still starting.
    pub fn connect_retry(
        path: &Path,
        channel: &ChannelId,
        capabilities: ChannelCapabilities,
        actions: ChannelActions,
        request_prefix: &str,
        timeout: Duration,
    ) -> Result<Self, ChannelDriverError> {
        let mut last = None;
        for attempt in 0..3 {
            match Self::connect(
                path,
                channel,
                capabilities,
                actions,
                request_prefix,
                timeout,
            ) {
                Ok(client) => return Ok(client),
                Err(error @ ChannelDriverError::Io(_)) if attempt < 2 => {
                    last = Some(error);
                    thread::sleep(Duration::from_millis(100 * (attempt + 1)));
                }
                Err(error) => return Err(error),
            }
        }
        Err(last.unwrap_or_else(|| {
            ChannelDriverError::Protocol("channel driver unavailable".to_owned())
        }))
    }

    fn connect(
        path: &Path,
        channel: &ChannelId,
        capabilities: ChannelCapabilities,
        actions: ChannelActions,
        request_prefix: &str,
        timeout: Duration,
    ) -> Result<Self, ChannelDriverError> {
        let writer = open(path, timeout)?;
        writer.set_read_timeout(Some(timeout))?;
        writer.set_write_timeout(Some(timeout))?;
        let reader = std::io::BufReader::new(writer.try_clone()?);
        let mut client = Self { writer, reader };
        client.send(ChannelFrameBody::Hello {
            request_id: format!("{request_prefix}-hello"),
            channel: channel.clone(),
            capabilities,
            actions,
        })?;
        client.send(ChannelFrameBody::Start {
            request_id: format!("{request_prefix}-start"),
        })?;
        Ok(client)
    }
}
