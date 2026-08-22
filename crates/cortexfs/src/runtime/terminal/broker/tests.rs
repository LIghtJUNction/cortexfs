use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::sync::Mutex;

use super::state::{BrokerState, Supervisor};
use super::*;

#[test]
fn request_json_is_flat_and_rejects_unknown_fields() -> Result<(), Box<dyn std::error::Error>> {
    let request = BrokerRequest::Connect(ConnectRequest {
        abi: BROKER_ABI.into(),
        nonce: "abcdefghijklmnopqrstuvwx".into(),
        agent: "coder".into(),
        session: "default".into(),
        mode: TerminalMode::Watch,
    });
    let value = serde_json::to_value(request)?;
    assert_eq!(value.get("agent"), Some(&serde_json::json!("coder")));
    let mut unknown = value;
    let object = unknown
        .as_object_mut()
        .ok_or_else(|| io::Error::other("not object"))?;
    object.insert("unexpected".into(), serde_json::json!(true));
    assert!(serde_json::from_value::<BrokerRequest>(unknown).is_err());
    Ok(())
}

#[test]
fn frame_round_trip_preserves_typed_reply() -> Result<(), Box<dyn std::error::Error>> {
    let (mut sender, mut receiver) = UnixStream::pair()?;
    write_frame(
        &mut sender,
        &BrokerReply::Registered {
            generation: "42:900".into(),
        },
    )?;
    assert!(matches!(
        read_frame(&mut receiver)?,
        BrokerReply::Registered { generation } if generation == "42:900"
    ));
    Ok(())
}

#[test]
fn frame_reader_rejects_oversized_body_before_allocation() -> Result<(), Box<dyn std::error::Error>>
{
    let (mut sender, mut receiver) = UnixStream::pair()?;
    let oversized = u32::try_from(MAX_BROKER_FRAME_BYTES + 1)?;
    sender.write_all(&oversized.to_be_bytes())?;
    assert!(matches!(
        read_frame::<BrokerReply>(&mut receiver),
        Err(BrokerProtocolError::FrameLimit)
    ));
    Ok(())
}

#[test]
fn grant_transaction_commits_the_authenticated_client_fd() -> Result<(), Box<dyn std::error::Error>>
{
    let (broker_control, mut supervisor_control) = UnixStream::pair()?;
    let (mut broker_client, mut operator) = UnixStream::pair()?;
    let supervisor = Supervisor {
        unit: "unit".into(),
        generation: "42:900".into(),
        control: Mutex::new(broker_control),
    };
    let worker = std::thread::spawn(move || -> Result<[u8; 4], BrokerProtocolError> {
        let BrokerReply::Offer { nonce, .. } = read_frame(&mut supervisor_control)? else {
            return Err(BrokerProtocolError::Protocol);
        };
        let mut client = UnixStream::from(receive_fd(&supervisor_control)?);
        write_frame(
            &mut supervisor_control,
            &BrokerReply::Prepared {
                nonce: nonce.clone(),
            },
        )?;
        if !matches!(read_frame(&mut supervisor_control)?, BrokerReply::Commit { nonce: value } if value == nonce)
        {
            return Err(BrokerProtocolError::Protocol);
        }
        let mut bytes = [0_u8; 4];
        client.read_exact(&mut bytes)?;
        Ok(bytes)
    });
    grant::offer(
        &mut broker_client,
        &supervisor,
        "abcdefghijklmnopqrstuvwx",
        TerminalMode::Attach,
    )?;
    assert!(matches!(
        read_frame(&mut operator)?,
        BrokerReply::Accepted { .. }
    ));
    operator.write_all(b"ping")?;
    let received = worker
        .join()
        .map_err(|_error| io::Error::other("supervisor worker failed"))??;
    assert_eq!(received, *b"ping");
    Ok(())
}

#[test]
fn replay_window_rejects_a_reused_nonce() {
    let state = BrokerState::new();
    assert!(state.consume_nonce(1000, "abcdefghijklmnopqrstuvwx".into()));
    assert!(!state.consume_nonce(1000, "abcdefghijklmnopqrstuvwx".into()));
    assert!(state.consume_nonce(1001, "abcdefghijklmnopqrstuvwx".into()));
}
