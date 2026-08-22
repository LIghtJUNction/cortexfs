use std::os::unix::net::UnixStream;
use std::time::Duration;

use super::peer::{PeerIdentity, validate_names};
use super::state::{BrokerState, Supervisor, TerminalKey};
use super::{
    AwaitRequest, BROKER_ABI, BrokerProtocolError, BrokerReply, BrokerRequest, RegisterRequest,
    read_frame, write_frame,
};

pub(super) fn register(
    stream: &mut UnixStream,
    state: &BrokerState,
    peer: &PeerIdentity,
    request: RegisterRequest,
) -> Result<(), BrokerProtocolError> {
    require_abi(&request.abi)?;
    peer.authorize_supervisor(&request.agent, &request.session, &request.unit)?;
    let generation = peer.generation.clone();
    let key = TerminalKey {
        uid: peer.uid,
        agent: request.agent,
        session: request.session,
    };
    write_frame(
        stream,
        &BrokerReply::Registered {
            generation: generation.clone(),
        },
    )?;
    match read_frame(stream)? {
        BrokerRequest::Activate(request)
            if request.abi == BROKER_ABI && request.generation == generation => {}
        _ => return Err(BrokerProtocolError::Protocol),
    }
    state
        .register(
            key.clone(),
            Supervisor {
                unit: request.unit,
                generation: generation.clone(),
                control: std::sync::Mutex::new(stream.try_clone()?),
            },
        )
        .map_err(|()| {
            rejected(
                "registration",
                "terminal supervisor registration unavailable",
            )
        })?;
    if let Err(error) = write_frame(
        stream,
        &BrokerReply::Activated {
            generation: generation.clone(),
        },
    ) {
        state.remove(&key, &generation);
        return Err(error);
    }
    Ok(())
}

pub(super) fn await_ready(
    stream: &mut UnixStream,
    state: &BrokerState,
    peer: &PeerIdentity,
    request: AwaitRequest,
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
        .wait(&key, &request.unit, Duration::from_millis(750))
        .ok_or_else(|| rejected("not_ready", "terminal supervisor is not registered"))?;
    write_frame(
        stream,
        &BrokerReply::Ready {
            nonce: request.nonce,
            generation: supervisor.generation.clone(),
        },
    )
}

pub(super) fn require_request(
    peer: &PeerIdentity,
    state: &BrokerState,
    abi: &str,
    nonce: &str,
    agent: &str,
    session: &str,
) -> Result<(), BrokerProtocolError> {
    require_abi(abi)?;
    validate_names(agent, session)?;
    peer.authorize_operator()?;
    if nonce.len() != 24 || !state.consume_nonce(peer.uid, nonce.into()) {
        return Err(rejected("replay", "invalid or replayed request nonce"));
    }
    Ok(())
}

fn require_abi(abi: &str) -> Result<(), BrokerProtocolError> {
    (abi == BROKER_ABI)
        .then_some(())
        .ok_or(BrokerProtocolError::Protocol)
}

pub(super) fn rejected(code: &str, message: &str) -> BrokerProtocolError {
    BrokerProtocolError::Rejected(code.into(), message.into())
}
