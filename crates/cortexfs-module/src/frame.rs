use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ModuleMetadata;

/// Lifecycle operation requested by a host.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModuleOperation {
    Init,
    Start,
    Stop,
    Shutdown,
}

/// One directional message exchanged by a host and an external module.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ModuleFrame {
    Hello {
        abi: String,
        instance: String,
    },
    Ready {
        metadata: ModuleMetadata,
    },
    Lifecycle {
        request_id: String,
        operation: ModuleOperation,
    },
    Call {
        request_id: String,
        method: String,
        payload: Value,
    },
    Result {
        request_id: String,
        payload: Value,
    },
    Event {
        name: String,
        payload: Value,
    },
    Error {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
        code: String,
        message: String,
    },
}
