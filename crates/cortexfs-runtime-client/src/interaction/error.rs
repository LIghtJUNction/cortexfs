use thiserror::Error;

/// Stable validation failures for `cortexfs.interaction/v2` frames.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum InteractionV2Error {
    #[error("interaction v2 frame has wrong ABI")]
    WrongAbi,
    #[error("invalid interaction identifier: {0}")]
    InvalidIdentifier(&'static str),
    #[error("interaction v2 frame has invalid sequence")]
    InvalidSequence,
    #[error("missing interaction correlation: {0}")]
    MissingCorrelation(&'static str),
    #[error("unexpected interaction correlation: {0}")]
    UnexpectedCorrelation(&'static str),
    #[error("interaction v2 frame has invalid capabilities")]
    InvalidCapabilities,
    #[error("interaction v2 frame has invalid attachment mode")]
    InvalidAttachmentMode,
    #[error("durable event and session sequence disagree")]
    DurableSequenceMismatch,
    #[error("interaction v2 frame has invalid event")]
    InvalidEvent,
}
