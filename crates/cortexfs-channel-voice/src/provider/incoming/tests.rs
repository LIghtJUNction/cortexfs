use std::{collections::BTreeSet, net::SocketAddr, path::PathBuf};

use serde_json::json;

use super::*;
use crate::{
    config::{ChannelKind, Config, Provider},
    error::Result,
};

fn config() -> Config {
    Config {
        channel: ChannelKind::VoiceCall,
        provider: Provider::Telnyx,
        api_base: "https://example.invalid/v2".to_owned(),
        auth_token: "secret".to_owned(),
        account_id: "connection".to_owned(),
        from_number: "+100".to_owned(),
        allowed_destinations: BTreeSet::from(["+101".to_owned()]),
        socket: PathBuf::from("/run/cortexfs/channel/voice_call.sock"),
        webhook_bind: SocketAddr::from(([127, 0, 0, 1], 8789)),
        webhook_token: None,
        webhook_base: None,
        hangup_after: None,
        wake_executable: None,
    }
}

#[test]
fn decodes_twilio_form_into_call_conversation() -> Result<()> {
    let mut calls = Calls::new();
    let message = decode(
        &config(),
        "application/x-www-form-urlencoded",
        "CallSid=CA1&From=%2B101&Speech=hello",
        &mut calls,
    )?
    .ok_or_else(|| crate::error::Error::Protocol("expected voice message".to_owned()))?;
    assert_eq!(message.target.conversation.as_str(), "call:CA1");
    assert_eq!(message.sender.id, "+101");
    assert_eq!(message.body.text, "hello");
    assert!(calls.contains_key("phone:+101"));
    Ok(())
}

#[test]
fn decodes_telnyx_payload_and_removes_terminal_call() -> Result<()> {
    let mut calls = Calls::new();
    let body = json!({
        "data": {"event_type": "call.answered", "payload": {
            "call_control_id": "CC1", "from": "+101", "text": "hi"
        }}
    })
    .to_string();
    assert!(decode(&config(), "application/json", &body, &mut calls)?.is_some());
    let ended = r#"{"data":{"event_type":"call.hangup","payload":{"call_control_id":"CC1","from":"+101"}}}"#;
    assert!(decode(&config(), "application/json", ended, &mut calls)?.is_none());
    assert!(!calls.values().any(|call| call.id == "CC1"));
    Ok(())
}
