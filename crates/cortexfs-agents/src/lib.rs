//! Official `CortexFS` role agents built on the Agent SDK.

use cortexfs_agent_sdk::{
    Agent, AgentEmitter, AgentError, AgentInvocation, AgentOutcome, AgentResult,
};
use std::io::Write;

#[derive(Clone, Copy, Debug)]
pub struct RoleAgent(u8);

impl RoleAgent {
    fn response(self, input: &str) -> String {
        let (title, mission, handoff) = match self.0 {
            0 => (
                "Architect",
                "Turn ambiguous goals into an auditable plan",
                "plan, dependencies, risks, and acceptance criteria",
            ),
            1 => (
                "Executor",
                "Implement approved work with the least risky change",
                "changed files, commands, tests, and remaining risks",
            ),
            _ => (
                "Product Manager",
                "Clarify user value, scope, and measurable success",
                "problem statement, non-goals, and acceptance criteria",
            ),
        };
        let request = if input.trim().is_empty() {
            "(no request supplied)"
        } else {
            input.trim()
        };
        format!("Role: {title}\nMission: {mission}\n\nRequest:\n{request}\n\nHandoff: {handoff}.")
    }
}

impl Agent for RoleAgent {
    fn run(
        &self,
        invocation: &AgentInvocation,
        output: &mut AgentEmitter<&mut dyn Write>,
    ) -> AgentResult<AgentOutcome> {
        output
            .message(&self.response(invocation.input()))
            .map_err(|error| AgentError::new("EIO", error.to_string()))?;
        Ok(AgentOutcome::Complete)
    }
}

pub const ARCHITECT_AGENT: RoleAgent = RoleAgent(0);
pub const EXECUTOR_AGENT: RoleAgent = RoleAgent(1);
pub const PRODUCT_MANAGER_AGENT: RoleAgent = RoleAgent(2);
