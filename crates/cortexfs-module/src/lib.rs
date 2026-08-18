#![forbid(unsafe_code)]

//! Static Rust module API plus a stable external wire contract for extensions.
//!
//! This crate is deliberately independent from FUSE, `/ctx`, providers, and
//! any async executor. The Rust trait is only for static composition inside one
//! Cargo build. External processes use [`ModuleFrame`] over a Unix socket.

mod frame;
mod lifecycle;
mod metadata;
mod registry;
mod wire;

pub use frame::{ModuleFrame, ModuleOperation};
pub use lifecycle::{
    CortexModule, ModuleContext, ModuleError, ModuleFuture, ModuleResult, ModuleState,
};
pub use metadata::{CORTEX_MODULE_ABI, ModuleCapability, ModuleKind, ModuleMetadata};
pub use registry::ModuleRegistry;
pub use wire::{CORTEX_MODULE_WIRE_ABI, MAX_MODULE_FRAME_BYTES, ModuleWireError};

pub(crate) fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}
