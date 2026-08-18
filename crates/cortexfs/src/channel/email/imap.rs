use std::time::Duration;

use cortexfs_channels::{ChannelCodec, platform::email::EmailCodec};

use super::super::bridge::AgentChannelBridge;
use super::{EmailConfig, EmailError, smtp};

pub(super) fn run_once(
    config: &EmailConfig,
    bridge: &AgentChannelBridge,
) -> Result<(), EmailError> {
    let client = imap::ClientBuilder::new(&config.imap_host, config.imap_port)
        .tls_kind(imap::TlsKind::Rust)
        .connect()
        .map_err(|error| EmailError::Imap(error.to_string()))?;
    let mut session = client
        .login(&config.username, &config.password)
        .map_err(|(error, _client)| EmailError::Imap(error.to_string()))?;
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
