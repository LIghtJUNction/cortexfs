use std::{collections::BTreeSet, net::SocketAddr, path::PathBuf};

use super::*;
use crate::config::{ChannelKind, Config, Provider};

fn config(executable: Option<&str>) -> Config {
    Config {
        channel: ChannelKind::VoiceWake,
        provider: Provider::Telnyx,
        api_base: "https://example.invalid".to_owned(),
        auth_token: String::new(),
        account_id: String::new(),
        from_number: String::new(),
        allowed_destinations: BTreeSet::new(),
        socket: PathBuf::from("/run/cortexfs/channel/voice_wake.sock"),
        webhook_bind: SocketAddr::from(([127, 0, 0, 1], 8789)),
        webhook_token: None,
        webhook_base: None,
        hangup_after: None,
        wake_executable: executable.map(str::to_owned),
    }
}

#[tokio::test]
async fn wake_command_requires_configured_engine() {
    let result = run(&config(None), "voice_wake.wake", &serde_json::json!({})).await;
    assert!(result.is_err());
    assert!(
        result
            .err()
            .is_some_and(|error| error.to_string().contains("WAKE_EXECUTABLE"))
    );
}

#[tokio::test]
async fn wake_command_returns_result_from_engine() {
    let result = run(
        &config(Some("/bin/true")),
        "voice_wake.wake",
        &serde_json::json!({}),
    )
    .await;
    assert_eq!(
        result.ok().and_then(|value| value.get("accepted").cloned()),
        Some(Value::Bool(true))
    );
}
