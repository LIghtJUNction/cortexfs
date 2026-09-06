use super::InteractionOrigin;
use serde::{Deserialize, Serialize};
use serde_json::Value;
/// Version marker for the persistent interaction stream.
pub const INTERACTION_V2_ABI: &str = "cortexfs.interaction/v2";
/// A feature negotiated by a master and slave.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractionCapability {
    Input,
    Observe,
    Status,
    Cancel,
    CommandResult,
    Invoke,
    Replay,
}
/// Access granted to one session attachment.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentMode {
    Observe,
    Interact,
}
/// The independent peer that emitted an event.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractionSide {
    Master,
    Slave,
}
/// Event names reserved by the phase-one v2 stream.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractionV2Kind {
    Hello,
    Welcome,
    Attach,
    Attached,
    Detach,
    Input,
    Status,
    Cancel,
    CommandResult,
    Ack,
    Accepted,
    Started,
    Event,
    Command,
    Done,
    Gap,
    Error,
}
/// Correlation carried independently of an event body.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct InteractionCorrelation {
    #[serde(rename = "connection_id")]
    pub connection: String,
    #[serde(rename = "attachment_id")]
    pub attachment: Option<String>,
    #[serde(rename = "request_id")]
    pub request: Option<String>,
    #[serde(rename = "run_id")]
    pub run: Option<String>,
    #[serde(rename = "command_id")]
    pub command: Option<String>,
}
/// The extensible body of a phase-one stream event.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct InteractionV2Event {
    pub side: InteractionSide,
    pub kind: InteractionV2Kind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<InteractionCapability>,
    pub mode: Option<AttachmentMode>,
    pub session: Option<String>,
    pub origin: Option<InteractionOrigin>,
    pub durable: bool,
    pub data: Value,
}
/// One transport-neutral v2 stream frame.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct InteractionV2Frame {
    pub abi: String,
    pub correlation: InteractionCorrelation,
    pub session_seq: Option<u64>,
    pub event: InteractionV2Event,
}
