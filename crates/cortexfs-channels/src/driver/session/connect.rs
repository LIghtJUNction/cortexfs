use std::{path::Path, sync::mpsc, thread, time::Duration};

use crate::{ChannelActions, ChannelCapabilities, ChannelFrameBody, ChannelId};

use super::{ChannelDriverError, ChannelDriverSession, read, send};

impl ChannelDriverSession {
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
                Ok(session) => return Ok(session),
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
        let reader_stream = super::super::connect::open(path, timeout)?;
        reader_stream.set_read_timeout(None)?;
        let writer = std::sync::Arc::new(std::sync::Mutex::new(reader_stream.try_clone()?));
        send(
            &writer,
            ChannelFrameBody::Hello {
                request_id: format!("{request_prefix}-hello"),
                channel: channel.clone(),
                capabilities,
                actions,
            },
        )?;
        send(
            &writer,
            ChannelFrameBody::Start {
                request_id: format!("{request_prefix}-start"),
            },
        )?;
        let (sender, frames) = mpsc::sync_channel(64);
        let _reader = read::spawn(reader_stream, sender);
        Ok(Self {
            writer,
            frames: std::sync::Arc::new(std::sync::Mutex::new(frames)),
        })
    }
}
