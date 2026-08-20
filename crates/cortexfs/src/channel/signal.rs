use std::{
    io::{BufRead, BufReader},
    process::{Command, Stdio},
    thread,
    time::Duration,
};

use cortexfs_channels::{
    ChannelCodec, ChannelError, OutboundMessage, platform::signal::SignalCodec,
};

use super::bridge::{AgentChannelBridge, ChannelBridgeError};

mod control;

/// signal-cli JSON-lines adapter. signal-cli owns Signal Protocol state.
#[derive(Clone, Debug)]
pub struct SignalConfig {
    pub account: String,
    pub executable: String,
}

impl SignalConfig {
    pub fn new(account: String, executable: String) -> Result<Self, SignalError> {
        if account.is_empty() || executable.is_empty() {
            return Err(SignalError::Config(
                "account and executable are required".to_owned(),
            ));
        }
        Ok(Self {
            account,
            executable,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SignalError {
    #[error("Signal configuration failed: {0}")]
    Config(String),
    #[error("signal-cli process failed: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Channel(#[from] ChannelError),
    #[error(transparent)]
    Bridge(#[from] ChannelBridgeError),
    #[error("signal-cli exited unsuccessfully")]
    Process,
}

/// Runs signal-cli receive JSON output and reconnects when it exits.
pub fn run(config: &SignalConfig, bridge: &AgentChannelBridge) -> Result<(), SignalError> {
    let control = control::start(config, bridge)?;
    loop {
        control
            .check()
            .map_err(|error| SignalError::Config(error.to_string()))?;
        if let Err(_error) = run_once(config, bridge) {
            thread::sleep(Duration::from_secs(5));
        }
    }
}

fn run_once(config: &SignalConfig, bridge: &AgentChannelBridge) -> Result<(), SignalError> {
    let mut child = Command::new(&config.executable)
        .args([
            "-a",
            &config.account,
            "receive",
            "--json",
            "--ignore-attachments",
        ])
        .stdout(Stdio::piped())
        .spawn()?;
    let stdout = child.stdout.take().ok_or(SignalError::Process)?;
    let codec = SignalCodec;
    for line in BufReader::new(stdout).lines() {
        let line = line?;
        let Some(inbound) = codec.decode(&line)? else {
            continue;
        };
        if let Ok(outbound) = bridge.handle(inbound) {
            send(config, codec, &outbound)?;
        }
    }
    if child.wait()?.success() {
        Ok(())
    } else {
        Err(SignalError::Process)
    }
}

fn send(
    config: &SignalConfig,
    codec: SignalCodec,
    message: &OutboundMessage,
) -> Result<(), SignalError> {
    codec.encode(message)?;
    let mut command = Command::new(&config.executable);
    command.args(["-a", &config.account, "send"]);
    if let Some(group) = message.metadata.get("signal.group") {
        command.args(["-g", group]);
    } else {
        command.args(["--", message.target.conversation.as_str()]);
    }
    command.args(["-m", &message.body.text]);
    if command.status()?.success() {
        Ok(())
    } else {
        Err(SignalError::Process)
    }
}
