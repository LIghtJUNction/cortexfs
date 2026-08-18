use serde::{Deserialize, Serialize};

use super::{InteractionEvent, InteractionRequest};

/// Version marker for the logical frontend/runtime interaction protocol.
pub const INTERACTION_ABI: &str = "cortexfs.interaction/v1";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct InteractionFrame {
    pub abi: String,
    pub payload: InteractionPayload,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "direction", content = "value", rename_all = "snake_case")]
pub enum InteractionPayload {
    Request(InteractionRequest),
    Event(InteractionEvent),
}

impl InteractionFrame {
    #[must_use]
    pub fn request(request: InteractionRequest) -> Self {
        Self {
            abi: INTERACTION_ABI.to_owned(),
            payload: InteractionPayload::Request(request),
        }
    }

    #[must_use]
    pub fn event(event: InteractionEvent) -> Self {
        Self {
            abi: INTERACTION_ABI.to_owned(),
            payload: InteractionPayload::Event(event),
        }
    }

    #[expect(
        clippy::pattern_type_mismatch,
        reason = "match ergonomics keep borrowed interaction fields readable"
    )]
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.abi != INTERACTION_ABI {
            return Err("abi");
        }
        match &self.payload {
            InteractionPayload::Request(request) => request.validate(),
            InteractionPayload::Event(_) => Ok(()),
        }
    }
}
