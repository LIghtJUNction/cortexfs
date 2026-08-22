use std::os::unix::net::UnixStream;
use std::sync::Arc;

use super::grant::connect;
use super::peer::PeerIdentity;
use super::session::{await_ready, register};
use super::state::BrokerState;
use super::{BrokerProtocolError, BrokerRequest, read_frame};

pub(super) fn serve(
    stream: &mut UnixStream,
    state: &Arc<BrokerState>,
) -> Result<(), BrokerProtocolError> {
    let peer = PeerIdentity::read(stream)?;
    match read_frame(&mut *stream)? {
        BrokerRequest::Register(request) => register(stream, state, &peer, request),
        BrokerRequest::Await(request) => await_ready(stream, state, &peer, request),
        BrokerRequest::Connect(request) => connect(stream, state, &peer, request),
        BrokerRequest::Activate(_request) => Err(BrokerProtocolError::Protocol),
    }
}
