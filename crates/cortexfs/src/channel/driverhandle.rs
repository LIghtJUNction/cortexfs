use std::os::unix::net::UnixStream;

use cortexfs_channels::{ChannelFrame, ChannelFrameBody, ChannelHealth, ChannelRuntimeEvent};

use super::{driver::DriverConfig, driverprogress::DriverProgress};

#[expect(
    clippy::redundant_pub_crate,
    reason = "the parent and its unit tests exercise the socket state machine"
)]
pub(crate) fn handle(
    frame: ChannelFrame,
    config: &DriverConfig,
    stream: &mut UnixStream,
) -> (Option<ChannelFrame>, bool) {
    match frame.frame {
        ChannelFrameBody::Hello {
            request_id,
            channel,
            ..
        } => {
            if channel == config.channel {
                (Some(event(ChannelRuntimeEvent::Connected)), false)
            } else {
                (Some(error(Some(request_id))), false)
            }
        }
        ChannelFrameBody::ControlHello {
            request_id,
            channel,
        } => {
            if channel == config.channel {
                (Some(event(ChannelRuntimeEvent::Connected)), false)
            } else {
                (Some(error(Some(request_id))), true)
            }
        }
        ChannelFrameBody::ControlRequest { request_id, action } => {
            let response = match config.hub.dispatch(&config.channel, &request_id, action) {
                Ok(()) => ChannelFrame::new(ChannelFrameBody::ControlResponse {
                    request_id,
                    accepted: true,
                    error: None,
                }),
                Err(error) => ChannelFrame::new(ChannelFrameBody::ControlResponse {
                    request_id,
                    accepted: false,
                    error: Some(error.to_string()),
                }),
            };
            (Some(response), false)
        }
        ChannelFrameBody::Start { .. } => (Some(event(ChannelRuntimeEvent::Connected)), false),
        ChannelFrameBody::Inbound { event_id, message } => {
            if message.target.channel != config.channel {
                return (Some(error(Some(event_id))), false);
            }
            let result = {
                let mut progress =
                    DriverProgress::new(stream, message.target.clone(), event_id.clone());
                config.bridge.handle_with_progress(message, &mut progress)
            };
            match result {
                Ok(message) => (
                    Some(ChannelFrame::new(ChannelFrameBody::Deliver {
                        request_id: event_id,
                        message,
                    })),
                    false,
                ),
                Err(_) => (Some(error(Some(event_id))), false),
            }
        }
        ChannelFrameBody::InboundEvent { event_id, event } => {
            if event.context().target.channel != config.channel {
                return (Some(error(Some(event_id))), false);
            }
            let result = {
                let target = event.context().target.clone();
                let mut progress = DriverProgress::new(stream, target, event_id.clone());
                config
                    .bridge
                    .handle_event_with_progress(&event_id, &event, &mut progress)
            };
            match result {
                Ok(message) => (
                    Some(ChannelFrame::new(ChannelFrameBody::Deliver {
                        request_id: event_id,
                        message,
                    })),
                    false,
                ),
                Err(_) => (Some(error(Some(event_id))), false),
            }
        }
        ChannelFrameBody::Stop { .. } => (Some(event(ChannelRuntimeEvent::Disconnected)), true),
        ChannelFrameBody::Receipt {
            request_id,
            receipt,
        } => {
            let _accepted = config.hub.complete(&request_id, receipt);
            (None, false)
        }
        ChannelFrameBody::HealthRequest { request_id } => (
            Some(ChannelFrame::new(ChannelFrameBody::HealthResponse {
                request_id,
                health: ChannelHealth::ready(),
            })),
            false,
        ),
        ChannelFrameBody::Health { .. }
        | ChannelFrameBody::HealthResponse { .. }
        | ChannelFrameBody::Event { .. } => (None, false),
        _ => (Some(error(None)), false),
    }
}

fn event(event: ChannelRuntimeEvent) -> ChannelFrame {
    ChannelFrame::new(ChannelFrameBody::Event { event })
}

pub(super) fn error(request_id: Option<String>) -> ChannelFrame {
    ChannelFrame::new(ChannelFrameBody::Error {
        request_id,
        code: "channel_driver_error".to_owned(),
        message: "channel driver request failed".to_owned(),
        retryable: true,
    })
}
