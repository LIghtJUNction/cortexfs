use serde::{Deserialize, Serialize};

pub const BROKER_ABI: &str = "cortexfs.terminal-broker/v1";
pub const BROKER_SOCKET: &str = "/run/cortexfs/terminal/broker.sock";
pub const MAX_BROKER_FRAME_BYTES: usize = 4 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalMode {
    Watch,
    Attach,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RegisterRequest {
    pub abi: String,
    pub agent: String,
    pub session: String,
    pub unit: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AwaitRequest {
    pub abi: String,
    pub nonce: String,
    pub agent: String,
    pub session: String,
    pub unit: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectRequest {
    pub abi: String,
    pub nonce: String,
    pub agent: String,
    pub session: String,
    pub mode: TerminalMode,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ActivateRequest {
    pub abi: String,
    pub generation: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum BrokerRequest {
    Register(RegisterRequest),
    Await(AwaitRequest),
    Connect(ConnectRequest),
    Activate(ActivateRequest),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum BrokerReply {
    Registered { generation: String },
    Activated { generation: String },
    Ready { nonce: String, generation: String },
    Offer { nonce: String, mode: TerminalMode },
    Prepared { nonce: String },
    Accepted { nonce: String, generation: String },
    Commit { nonce: String },
    Abort { nonce: String },
    Error { code: String, message: String },
}
