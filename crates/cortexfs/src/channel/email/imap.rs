use std::time::Duration;

use cortexfs_channels::{ChannelCodec, platform::email::EmailCodec};
use serde_json::{Value, json};

use super::super::bridge::AgentChannelBridge;
use super::{EmailConfig, EmailError, smtp};

fn login(config: &EmailConfig) -> Result<imap::Session<imap::Connection>, EmailError> {
    let client = imap::ClientBuilder::new(&config.imap_host, config.imap_port)
        .tls_kind(imap::TlsKind::Rust)
        .connect()
        .map_err(|error| EmailError::Imap(error.to_string()))?;
    client
        .login(&config.username, &config.password)
        .map_err(|(error, _client)| EmailError::Imap(error.to_string()))
}

pub(super) fn run_once(
    config: &EmailConfig,
    bridge: &AgentChannelBridge,
) -> Result<(), EmailError> {
    let mut session = login(config)?;
    session
        .select(&config.mailbox)
        .map_err(|error| EmailError::Imap(error.to_string()))?;
    let codec = EmailCodec;
    loop {
        receive(&mut session, config, bridge, codec)?;
        session
            .idle()
            .timeout(Duration::from_secs(config.idle_seconds))
            .wait_while(|_| false)
            .map_err(|error| EmailError::Imap(error.to_string()))?;
    }
}

pub(super) fn tool(config: &EmailConfig, name: &str, payload: &Value) -> Result<Value, EmailError> {
    let mut session = login(config)?;
    session
        .select(&config.mailbox)
        .map_err(|error| EmailError::Imap(error.to_string()))?;
    match name {
        "email.search" => {
            let query = payload
                .get("query")
                .and_then(Value::as_str)
                .unwrap_or("ALL");
            let uids = session
                .uid_search(query)
                .map_err(|error| EmailError::Imap(error.to_string()))?;
            Ok(json!({"uids":uids.into_iter().collect::<Vec<_>>() }))
        }
        "email.read" => {
            let uid = uid(payload)?;
            let fetched = session
                .uid_fetch(uid.to_string(), "UID RFC822")
                .map_err(|error| EmailError::Imap(error.to_string()))?;
            let body = fetched
                .iter()
                .find_map(|item| item.body())
                .map(|body| String::from_utf8_lossy(body).into_owned())
                .unwrap_or_default();
            Ok(json!({"uid":uid,"message":body}))
        }
        "email.mark_read" => {
            let uid = uid(payload)?;
            session
                .uid_store(uid.to_string(), "+FLAGS (\\Seen)")
                .map_err(|error| EmailError::Imap(error.to_string()))?;
            Ok(json!({"accepted":true}))
        }
        _ => Err(EmailError::Imap("unsupported operation".to_owned())),
    }
}

fn uid(payload: &Value) -> Result<u32, EmailError> {
    payload
        .get("uid")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| EmailError::Imap("uid is missing".to_owned()))
}

fn receive<T>(
    session: &mut imap::Session<T>,
    config: &EmailConfig,
    bridge: &AgentChannelBridge,
    codec: EmailCodec,
) -> Result<(), EmailError>
where
    T: std::io::Read + std::io::Write,
{
    let uids = session
        .uid_search("UNSEEN")
        .map_err(|error| EmailError::Imap(error.to_string()))?;
    if uids.is_empty() {
        return Ok(());
    }
    let set = uids
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let fetched = session
        .uid_fetch(set, "UID RFC822")
        .map_err(|error| EmailError::Imap(error.to_string()))?;
    let pending = fetched
        .iter()
        .filter_map(|item| item.body().map(|body| (item.uid, body.to_owned())))
        .collect::<Vec<_>>();
    drop(fetched);
    for (uid, body) in pending {
        let Some(inbound) = codec.decode(std::str::from_utf8(&body).unwrap_or(""))? else {
            continue;
        };
        if inbound.sender.id.eq_ignore_ascii_case(&config.username) {
            continue;
        }
        let outbound = bridge.handle(inbound)?;
        codec.encode(&outbound)?;
        smtp::send(config, &outbound)?;
        if let Some(uid) = uid {
            session
                .uid_store(uid.to_string(), "+FLAGS (\\Seen)")
                .map_err(|error| EmailError::Imap(error.to_string()))?;
        }
    }
    Ok(())
}
