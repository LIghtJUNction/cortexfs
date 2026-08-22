use std::os::unix::net::UnixStream;

use base64::Engine as _;

use super::{
    AwaitRequest, BROKER_ABI, BROKER_SOCKET, BrokerProtocolError, BrokerReply, BrokerRequest,
    ConnectRequest, TerminalMode, read_frame, require_reply, write_frame,
};

pub fn await_terminal(
    agent: &str,
    session: &str,
    unit: &str,
) -> Result<String, BrokerProtocolError> {
    let nonce = random_nonce()?;
    let request = BrokerRequest::Await(AwaitRequest {
        abi: BROKER_ABI.into(),
        nonce: nonce.clone(),
        agent: agent.into(),
        session: session.into(),
        unit: unit.into(),
    });
    match require_reply(exchange(&request)?.1, Some(&nonce))? {
        BrokerReply::Ready { generation, .. } => Ok(generation),
        _ => Err(BrokerProtocolError::Protocol),
    }
}

pub fn connect_terminal(
    agent: &str,
    session: &str,
    mode: TerminalMode,
) -> Result<UnixStream, BrokerProtocolError> {
    let nonce = random_nonce()?;
    let request = BrokerRequest::Connect(ConnectRequest {
        abi: BROKER_ABI.into(),
        nonce: nonce.clone(),
        agent: agent.into(),
        session: session.into(),
        mode,
    });
    let (stream, reply) = exchange(&request)?;
    match require_reply(reply, Some(&nonce))? {
        BrokerReply::Accepted { .. } => Ok(stream),
        _ => Err(BrokerProtocolError::Protocol),
    }
}

pub(super) fn exchange(
    request: &BrokerRequest,
) -> Result<(UnixStream, BrokerReply), BrokerProtocolError> {
    let mut stream = UnixStream::connect(BROKER_SOCKET)?;
    if crate::peer_credentials(&stream)
        .map_err(|_error| BrokerProtocolError::UntrustedPeer)?
        .uid()
        != 0
    {
        return Err(BrokerProtocolError::UntrustedPeer);
    }
    write_frame(&mut stream, request)?;
    let reply = read_frame(&mut stream)?;
    Ok((stream, reply))
}

fn random_nonce() -> Result<String, BrokerProtocolError> {
    let mut bytes = [0_u8; 18];
    getrandom::fill(&mut bytes)
        .map_err(|error| std::io::Error::other(format!("random nonce failed: {error}")))?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
}
