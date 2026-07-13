use std::io::Write;

use cortexfs_agent_sdk::{
    Agent, AgentEmitter, AgentInvocation, AgentOutcome, AgentResult, cortexfs_agent_main,
};

// Argv: `CTX_AGENT=echo CTX_RUN_ID=example-run cargo run -p cortexfs-agent-sdk --example echo -- hello`.
// Stdin: `printf 'hello\n' | CTX_AGENT=echo CTX_RUN_ID=example-run cargo run -p cortexfs-agent-sdk --example echo`.
#[derive(Debug)]
struct EchoAgent;

impl Agent for EchoAgent {
    fn run(
        &self,
        invocation: &AgentInvocation,
        output: &mut AgentEmitter<&mut dyn Write>,
    ) -> AgentResult<AgentOutcome> {
        let _session = invocation.session();
        output
            .message(invocation.input())
            .map_err(|error| cortexfs_agent_sdk::AgentError::new("EIO", error.to_string()))?;
        Ok(AgentOutcome::Complete)
    }
}

cortexfs_agent_main!(EchoAgent);
