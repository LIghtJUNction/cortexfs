#![forbid(unsafe_code)]

//! Protocol-neutral AI model request and event types.

pub mod anthropic;
mod borrowed;
pub mod bridge;
mod content;
mod context;
#[doc(hidden)]
pub mod decodeanthropic;
#[doc(hidden)]
pub mod decodeanthropicpart;
#[doc(hidden)]
pub mod decodechoice;
#[doc(hidden)]
pub mod decodegoogle;
#[doc(hidden)]
pub mod decodegooglepart;
#[doc(hidden)]
pub mod decodeopenai;
#[doc(hidden)]
pub mod decoderesponsepart;
#[doc(hidden)]
pub mod decoderesponses;
pub mod direct;
#[doc(hidden)]
pub mod directpart;
#[doc(hidden)]
pub mod directtool;
#[doc(hidden)]
pub mod encode;
#[doc(hidden)]
pub mod encodeanthropic;
#[doc(hidden)]
pub mod encodegoogle;
#[doc(hidden)]
pub mod encodeopenai;
#[doc(hidden)]
pub mod encoderesponses;
mod error;
mod event;
pub mod gemini;
mod message;
pub mod openaichat;
pub mod openairesponses;
mod request;
mod response;
#[doc(hidden)]
pub mod responseanthropic;
#[doc(hidden)]
pub mod responsedecode;
#[doc(hidden)]
pub mod responseencode;
#[doc(hidden)]
pub mod responsegoogle;
#[doc(hidden)]
pub mod responsegooglepart;
#[doc(hidden)]
pub mod responseopenai;
#[doc(hidden)]
pub mod responseopenaipart;
#[doc(hidden)]
pub mod responseresponses;
#[doc(hidden)]
pub mod responseutil;
#[doc(hidden)]
pub mod reversepart;
#[doc(hidden)]
pub mod semantic;
mod tool;
mod usage;
pub mod wire;

pub use anthropic::Request as AnthropicRequest;
pub use borrowed::{ContentView, MessageView, ModelRequestView};
pub use bridge::{
    BridgePath, NativeRequest, TranscodedRequest, decode_model_request, decode_native_request,
    encode_model_request, transcode_request,
};
pub use content::{Content, ContentPart};
pub use context::{ContextOwnership, ContextReference, ContextState, ReplayPolicy};
pub use error::{ProtocolError, ProviderError};
pub use event::{EventStatus, ModelEvent};
pub use gemini::Request as GeminiRequest;
pub use message::{Message, Role};
pub use openaichat::Request as OpenAiChatRequest;
pub use openairesponses::Request as OpenAiResponsesRequest;
pub use request::{MODEL_PROTOCOL, ModelRequest};
pub use response::{
    TranscodedResponse, decode_response_events, encode_response_events, transcode_response,
};
pub use tool::{ContentResult, ToolCall, ToolChoice, ToolDefinition, ToolResult};
pub use usage::Usage;
pub use wire::{ConversionError, WireProtocol};
