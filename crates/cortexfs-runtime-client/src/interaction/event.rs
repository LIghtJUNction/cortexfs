use serde::{Deserialize, Serialize};

use super::InteractionCommand;

/// Runtime-to-client events normalized across terminal, web, and channels.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InteractionEvent {
    Accepted {
        request_id: String,
        session: String,
        run: String,
    },
    Started {
        request_id: String,
        run: String,
        model: Option<String>,
    },
    Delta {
        request_id: String,
        run: String,
        text: String,
    },
    Message {
        request_id: String,
        run: String,
        role: String,
        text: String,
    },
    Tool {
        request_id: String,
        run: String,
        call_id: String,
        name: String,
        state: String,
    },
    Command {
        request_id: String,
        run: String,
        command_id: String,
        command: InteractionCommand,
    },
    Status {
        request_id: String,
        session: String,
        status: String,
        phase: Option<String>,
        step: u32,
    },
    Error {
        request_id: String,
        run: Option<String>,
        code: String,
        message: String,
        retryable: bool,
    },
    Done {
        request_id: String,
        run: String,
        status: String,
    },
}
