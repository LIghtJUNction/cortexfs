use std::{path::Path, time::Duration};

use cortexfs_channels::ChannelDriverSession;

use crate::{ChannelSdkError, ChannelSender, ChannelService};

mod dispatch;

/// Persistent driver loop shared by process-isolated channel adapters.
#[derive(Debug)]
pub struct ChannelRuntime<S> {
    session: ChannelDriverSession,
    service: S,
}

impl<S: ChannelService> ChannelRuntime<S> {
    pub fn connect(
        path: &Path,
        mut service: S,
        request_prefix: &str,
        timeout: Duration,
    ) -> Result<Self, ChannelSdkError> {
        service
            .start()
            .map_err(|error| ChannelSdkError::adapter("start", error))?;
        let session = ChannelDriverSession::connect_retry(
            path,
            &service.id(),
            service.capabilities(),
            service.actions(),
            request_prefix,
            timeout,
        );
        match session {
            Ok(session) => Ok(Self { session, service }),
            Err(error) => {
                let _cleanup = service.stop();
                Err(error.into())
            }
        }
    }

    #[must_use]
    pub fn sender(&self) -> ChannelSender {
        ChannelSender::new(self.session.clone())
    }

    pub fn run(mut self) -> Result<(), ChannelSdkError> {
        let result = self.dispatch();
        let stopped = self
            .service
            .stop()
            .map_err(|error| ChannelSdkError::adapter("stop", error));
        result?;
        stopped
    }
}
