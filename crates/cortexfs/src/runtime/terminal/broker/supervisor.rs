use std::os::unix::net::UnixStream;

use super::client::exchange;
use super::{
    ActivateRequest, BROKER_ABI, BrokerProtocolError, BrokerReply, BrokerRequest, RegisterRequest,
    read_frame, require_reply, write_frame,
};

pub fn register_supervisor(
    agent: &str,
    session: &str,
    unit: &str,
) -> Result<(UnixStream, String), BrokerProtocolError> {
    let request = BrokerRequest::Register(RegisterRequest {
        abi: BROKER_ABI.into(),
        agent: agent.into(),
        session: session.into(),
        unit: unit.into(),
    });
    let (stream, reply) = exchange(&request)?;
    match require_reply(reply, None)? {
        BrokerReply::Registered { generation } => Ok((stream, generation)),
        _ => Err(BrokerProtocolError::Protocol),
    }
}

pub fn activate_supervisor(
    stream: &mut UnixStream,
    generation: &str,
) -> Result<(), BrokerProtocolError> {
    write_frame(
        stream,
        &BrokerRequest::Activate(ActivateRequest {
            abi: BROKER_ABI.into(),
            generation: generation.into(),
        }),
    )?;
    match require_reply(read_frame(stream)?, None)? {
        BrokerReply::Activated { generation: value } if value == generation => Ok(()),
        _ => Err(BrokerProtocolError::Protocol),
    }
}
