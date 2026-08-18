#![expect(
    clippy::redundant_pub_crate,
    reason = "socket helper is private driver plumbing"
)]

use std::time::Duration;

use cortexfs_channels::{
    ChannelActions, ChannelDriverSession, ChannelFrameBody, ChannelId, DeliveryReceipt,
    InboundMessage,
};

use crate::{
    config::Config,
    error::{Error, Result},
};

#[derive(Clone)]
pub(crate) struct Session {
    client: ChannelDriverSession,
}

impl Session {
    pub(crate) async fn connect(config: &Config) -> Result<Self> {
        let path = config.socket.clone();
        let channel = ChannelId::new(config.channel.id())?;
        let capabilities = Config::capabilities();
        tokio::task::spawn_blocking(move || {
            Ok(Self {
                client: ChannelDriverSession::connect_retry(
                    &path,
                    &channel,
                    capabilities,
                    ChannelActions::empty(),
                    "voice",
                    Duration::from_secs(10),
                )?,
            })
        })
        .await
        .map_err(|error| Error::Task(error.to_string()))?
    }

    pub(crate) fn send(&self, message: InboundMessage) -> Result<()> {
        self.client.send_inbound(message)?;
        Ok(())
    }

    pub(crate) fn next(&self) -> Result<ChannelFrameBody> {
        Ok(self.client.recv()?)
    }

    pub(crate) fn receipt(&self, request_id: String, receipt: DeliveryReceipt) -> Result<()> {
        self.client.send_receipt(request_id, receipt)?;
        Ok(())
    }
}
