use std::io;

pub mod client;
pub mod fd;
pub mod frame;
mod grant;
mod handle;
mod peer;
mod protocol;
pub mod server;
mod session;
mod state;
pub mod supervisor;

pub use client::{await_terminal, connect_terminal};
pub use fd::{receive_fd, send_fd};
pub use frame::{read_frame, write_frame};
pub use protocol::*;
pub use supervisor::{activate_supervisor, register_supervisor};

#[derive(Debug, thiserror::Error)]
pub enum BrokerProtocolError {
    #[error("terminal broker I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("terminal broker frame exceeds limit")]
    FrameLimit,
    #[error("terminal broker protocol mismatch")]
    Protocol,
    #[error("terminal broker peer is not root")]
    UntrustedPeer,
    #[error("terminal supervisor disconnected")]
    SupervisorLost,
    #[error("terminal broker rejected request: {0}: {1}")]
    Rejected(String, String),
}

pub fn require_reply(
    reply: BrokerReply,
    expected_nonce: Option<&str>,
) -> Result<BrokerReply, BrokerProtocolError> {
    if let BrokerReply::Error { code, message } = reply {
        return Err(BrokerProtocolError::Rejected(code, message));
    }
    let nonce = match &reply {
        &BrokerReply::Ready { ref nonce, .. }
        | &BrokerReply::Prepared { ref nonce }
        | &BrokerReply::Accepted { ref nonce, .. }
        | &BrokerReply::Offer { ref nonce, .. }
        | &BrokerReply::Commit { ref nonce }
        | &BrokerReply::Abort { ref nonce } => Some(nonce.as_str()),
        _ => None,
    };
    if expected_nonce.is_some() && nonce != expected_nonce {
        return Err(BrokerProtocolError::Protocol);
    }
    Ok(reply)
}

#[cfg(test)]
mod tests;
