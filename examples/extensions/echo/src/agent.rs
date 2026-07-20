use std::io::Write;

use cortexfs_agent_sdk::{
    Agent, AgentEmitter, AgentInvocation, AgentOutcome, AgentResult, AgentToolCallRequest,
    cortexfs_agent_main,
};

#[derive(Debug)]
struct EchoAgent;

impl Agent for EchoAgent {
    fn run(
        &self,
        invocation: &AgentInvocation,
        output: &mut AgentEmitter<&mut dyn Write>,
    ) -> AgentResult<AgentOutcome> {
        if let Some(input) = invocation.input().strip_prefix("tool ") {
            return match invocation.step() {
                0 => AgentToolCallRequest::new(
                    "echo-call-1",
                    "example.echo",
                    vec![serde_json::json!({ "text": input }).to_string()],
                )
                .map(AgentOutcome::YieldToolCall),
                1 => AgentToolCallRequest::new(
                    "echo-call-2",
                    "example.echo",
                    vec![
                        serde_json::json!({
                            "text": format!(
                                "second:{}",
                                invocation.observation().map_or("", |value| value.content())
                            )
                        })
                        .to_string(),
                    ],
                )
                .map(AgentOutcome::YieldToolCall),
                _ => {
                    output
                        .message(invocation.observation().map_or("", |value| value.content()))
                        .map_err(|error| {
                            cortexfs_agent_sdk::AgentError::new("EIO", error.to_string())
                        })?;
                    Ok(AgentOutcome::Complete)
                }
            };
        }
        output
            .message(invocation.input())
            .map_err(|error| cortexfs_agent_sdk::AgentError::new("EIO", error.to_string()))?;
        Ok(AgentOutcome::Complete)
    }
}

cortexfs_agent_main!(EchoAgent);
