use std::{path::Path, thread, time::Duration};

use super::{ChannelDriverClient, ChannelDriverError};
use crate::{
    ChannelActions, ChannelCapabilities, ChannelControlAction, ChannelFrameBody, ChannelId,
};

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

    /// Connects a short-lived controller without replacing the adapter session.
    pub fn connect_control_retry(
        path: &Path,
        channel: &ChannelId,
        request_prefix: &str,
        timeout: Duration,
    ) -> Result<Self, ChannelDriverError> {
        let writer = open(path, timeout)?;
        writer.set_read_timeout(Some(timeout))?;
        writer.set_write_timeout(Some(timeout))?;
        let reader = std::io::BufReader::new(writer.try_clone()?);
        let mut client = Self { writer, reader };
        client.send(ChannelFrameBody::ControlHello {
            request_id: format!("{request_prefix}-hello"),
            channel: channel.clone(),
        })?;
        Ok(client)
    }

    /// Sends one provider-neutral action to the connected channel adapter.
    pub fn request_control(
        &mut self,
        request_id: &str,
        action: ChannelControlAction,
    ) -> Result<(), ChannelDriverError> {
        self.send(ChannelFrameBody::ControlRequest {
            request_id: request_id.to_owned(),
            action,
        })?;
        loop {
            match self.next_frame()? {
                ChannelFrameBody::ControlResponse {
                    request_id: response_id,
                    accepted,
                    error,
                } if response_id == request_id => {
                    return if accepted {
                        Ok(())
                    } else {
                        Err(ChannelDriverError::Protocol(error.unwrap_or_else(|| {
                            "channel control request rejected".to_owned()
                        })))
                    };
                }
                ChannelFrameBody::Error {
                    request_id: Some(response_id),
                    message,
                    ..
                } if response_id == request_id => {
                    return Err(ChannelDriverError::Protocol(message));
                }
                _ => {}
            }
        }
    }
}
