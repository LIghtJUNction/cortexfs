use std::os::unix::net::UnixStream;

use super::peer::PeerIdentity;
use super::session::{rejected, require_request};
use super::state::{BrokerState, Supervisor, TerminalKey};
use super::{
    BrokerProtocolError, BrokerReply, ConnectRequest, TerminalMode, read_frame, send_fd,
    write_frame,
};

pub(super) fn connect(
    stream: &mut UnixStream,
    state: &BrokerState,
    peer: &PeerIdentity,
    request: ConnectRequest,
) -> Result<(), BrokerProtocolError> {
    require_request(
        peer,
        state,
        &request.abi,
        &request.nonce,
        &request.agent,
        &request.session,
    )?;
    let key = TerminalKey {
        uid: peer.uid,
        agent: request.agent,
        session: request.session,
    };
    let supervisor = state
        .get(&key)
        .ok_or_else(|| rejected("not_ready", "terminal supervisor is not registered"))?;
    let result = offer(stream, &supervisor, &request.nonce, request.mode);
    if matches!(result, Err(BrokerProtocolError::SupervisorLost)) {
        state.remove(&key, &supervisor.generation);
    }
    result
}

pub(super) fn offer(
    stream: &mut UnixStream,
    supervisor: &Supervisor,
    nonce: &str,
    mode: TerminalMode,
) -> Result<(), BrokerProtocolError> {
    let mut control = supervisor
        .control
        .lock()
        .map_err(|_error| BrokerProtocolError::SupervisorLost)?;
    supervisor_result(write_frame(
        &mut control,
        &BrokerReply::Offer {
            nonce: nonce.into(),
            mode,
        },
    ))?;
    supervisor_result(send_fd(&control, stream))?;
    match supervisor_result(read_frame(&mut control))? {
        BrokerReply::Prepared { nonce: prepared } if prepared == nonce => {}
        BrokerReply::Error { code, message } => {
            return Err(BrokerProtocolError::Rejected(code, message));
        }
        _ => return Err(BrokerProtocolError::SupervisorLost),
    }
    if let Err(error) = write_frame(
        stream,
        &BrokerReply::Accepted {
            nonce: nonce.into(),
            generation: supervisor.generation.clone(),
        },
    ) {
        supervisor_result(write_frame(
            &mut control,
            &BrokerReply::Abort {
                nonce: nonce.into(),
            },
        ))?;
        return Err(error);
    }
    supervisor_result(write_frame(
        &mut control,
        &BrokerReply::Commit {
            nonce: nonce.into(),
        },
    ))
}

fn supervisor_result<T>(result: Result<T, BrokerProtocolError>) -> Result<T, BrokerProtocolError> {
    result.map_err(|_error| BrokerProtocolError::SupervisorLost)
}
