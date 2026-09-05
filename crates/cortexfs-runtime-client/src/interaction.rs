//! Provider-neutral bidirectional interaction ABI for terminal, web, and channel clients.
mod error;
mod event;
mod frame;
mod normalize;
mod request;
mod shape;
mod stream;
mod validate;
mod verify;
mod wire;
pub use error::InteractionV2Error;
pub use event::InteractionEvent;
pub use frame::{INTERACTION_ABI, InteractionFrame, InteractionPayload};
pub use normalize::interaction_event_from_agent_frame;
pub use request::{InteractionCommand, InteractionOrigin, InteractionRequest, InteractionResult};
pub use stream::{
    AttachmentMode, INTERACTION_V2_ABI, InteractionCapability, InteractionCorrelation,
    InteractionSide, InteractionV2Event, InteractionV2Frame, InteractionV2Kind,
};
pub use wire::{InteractionWireError, MAX_INTERACTION_FRAME_BYTES};
