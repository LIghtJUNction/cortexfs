//! Provider-neutral bidirectional interaction ABI for terminal, web, and channel clients.

mod event;
mod frame;
mod normalize;
mod request;
mod validate;
mod wire;

pub use event::InteractionEvent;
pub use frame::{INTERACTION_ABI, InteractionFrame, InteractionPayload};
pub use normalize::interaction_event_from_agent_frame;
pub use request::{InteractionCommand, InteractionOrigin, InteractionRequest, InteractionResult};
pub use wire::{InteractionWireError, MAX_INTERACTION_FRAME_BYTES};
