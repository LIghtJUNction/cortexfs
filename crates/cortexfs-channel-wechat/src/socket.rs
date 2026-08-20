#![expect(
    clippy::redundant_pub_crate,
    reason = "socket helper is private driver plumbing"
)]

use std::{path::PathBuf, time::Duration};

use cortexfs_channels::{
    ChannelActions, ChannelCapabilities, ChannelDriverSession, ChannelFrameBody, ChannelId,
    DeliveryReceipt, InboundMessage,
};

use crate::error::{Error, Result};

#[derive(Clone)]
pub(crate) struct Session {
    client: ChannelDriverSession,
}

impl Session {
    pub(crate) async fn connect(path: PathBuf, timeout: Duration) -> Result<Self> {
        tokio::task::spawn_blocking(move || {
            Ok(Self {
                client: ChannelDriverSession::connect_retry(
                    &path,
                    &ChannelId::from_static("wechat"),
                    ChannelCapabilities {
                        polling: true,
                        long_polling: true,
                        tool_control: true,
                        ..ChannelCapabilities::text()
                    },
                    ChannelActions::empty(),
                    "wechat",
                    timeout,
                )?,
            })
        })
        .await
        .map_err(|error| Error::Task(error.to_string()))?
    }

    pub(crate) fn send_inbound(&self, message: InboundMessage) -> Result<()> {
        self.client.send_inbound(message)?;
        Ok(())
    }

    pub(crate) fn next(&self) -> Result<ChannelFrameBody> {
        Ok(self.client.recv()?)
    }

    pub(crate) fn send_frame(&self, frame: ChannelFrameBody) -> Result<()> {
        self.client.send_frame(frame)?;
        Ok(())
    }

    pub(crate) fn receipt(&self, request_id: String, receipt: DeliveryReceipt) -> Result<()> {
        self.client.send_receipt(request_id, receipt)?;
        Ok(())
    }
}
