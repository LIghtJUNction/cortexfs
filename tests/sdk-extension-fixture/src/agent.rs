use std::io::Write;

use cortexfs_agent_sdk::{
    Agent, AgentEmitter, AgentError, AgentInvocation, AgentOutcome, AgentResult,
    AgentToolCallRequest, cortexfs_agent_main,
};

#[derive(Debug)]
struct FixtureAgent;

impl Agent for FixtureAgent {
    fn run(
        &self,
        invocation: &AgentInvocation,
        output: &mut AgentEmitter<&mut dyn Write>,
    ) -> AgentResult<AgentOutcome> {
        if invocation.history_messages().is_none()
            && invocation.tool_context().is_none()
            && invocation.observation().is_none()
        {
            output
                .message(invocation.input())
                .map_err(|error| AgentError::new("EIO", error.to_string()))?;
            return Ok(AgentOutcome::Complete);
        }
        match invocation.step() {
            0 => {
                AgentToolCallRequest::new("fixture-call-1", "example.echo", vec!["one".to_owned()])
                    .map(AgentOutcome::YieldToolCall)
            }
            1 => {
                require_observation(invocation, "fixture-call-1", "native:one")?;
                AgentToolCallRequest::new("fixture-call-2", "example.echo", vec!["two".to_owned()])
                    .map(AgentOutcome::YieldToolCall)
            }
            2 => {
                require_observation(invocation, "fixture-call-2", "native:two")?;
                output
                    .message("fixture-complete")
                    .map_err(|error| AgentError::new("EIO", error.to_string()))?;
                Ok(AgentOutcome::Complete)
            }
            _ => Err(AgentError::invalid("unexpected continuation step")),
        }
    }
}

fn require_observation(
    invocation: &AgentInvocation,
    call_id: &str,
    content: &str,
) -> AgentResult<()> {
    let observation = invocation
        .observation()
        .ok_or_else(|| AgentError::invalid("missing host observation"))?;
    if observation.tool_call_id() != call_id
        || observation.name() != "example.echo"
        || observation.status() != "ok"
        || observation.content().trim() != content
        || observation.truncated()
    {
        return Err(AgentError::invalid("unexpected host observation"));
    }
    Ok(())
}

cortexfs_agent_main!(FixtureAgent);
