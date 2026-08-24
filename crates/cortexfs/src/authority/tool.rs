use super::access::AgentUnixIdentity;
use crate::{
    abi::authority::{AgentPermissions, ToolExecutionPrincipal},
    mount::table::MountTable,
    policy::PolicyEvaluator,
    support::toolpath::ToolHit,
};

/// Positive tool execution authority decision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolExecutionGrant {
    hit: ToolHit,
}

impl ToolExecutionGrant {
    /// Creates a grant for a concrete `CTX_PATH` hit.
    #[must_use]
    pub const fn new(hit: ToolHit) -> Self {
        Self { hit }
    }

    /// Returns the executable tool selected by left-to-right `CTX_PATH`.
    #[must_use]
    pub const fn hit(&self) -> &ToolHit {
        &self.hit
    }
}

/// Inputs that define an agent's effective authority for a tool execution.
#[derive(Clone, Copy, Debug)]
pub struct ToolExecutionAuthority<'a> {
    pub(crate) principal: ToolExecutionPrincipal,
    pub(crate) identity: &'a AgentUnixIdentity,
    pub(crate) mount_table: &'a MountTable,
    pub(crate) agent_subject: &'a str,
    pub(crate) agent_policy: &'a dyn PolicyEvaluator,
    pub(crate) tool_policy: &'a dyn PolicyEvaluator,
    pub(crate) permissions: AgentPermissions,
}

impl<'a> ToolExecutionAuthority<'a> {
    /// Creates an authority context for one tool execution decision.
    #[must_use]
    pub const fn new(
        identity: &'a AgentUnixIdentity,
        mount_table: &'a MountTable,
        agent_subject: &'a str,
        agent_policy: &'a dyn PolicyEvaluator,
        tool_policy: &'a dyn PolicyEvaluator,
        permissions: AgentPermissions,
    ) -> Self {
        Self {
            principal: ToolExecutionPrincipal::Agent,
            identity,
            mount_table,
            agent_subject,
            agent_policy,
            tool_policy,
            permissions,
        }
    }

    /// Creates an authority context for a model-originated tool execution
    /// attempt. This always denies at the `CortexFS` boundary.
    #[must_use]
    pub const fn model(
        identity: &'a AgentUnixIdentity,
        mount_table: &'a MountTable,
        model_subject: &'a str,
        agent_policy: &'a dyn PolicyEvaluator,
        tool_policy: &'a dyn PolicyEvaluator,
    ) -> Self {
        Self {
            principal: ToolExecutionPrincipal::Model,
            identity,
            mount_table,
            agent_subject: model_subject,
            agent_policy,
            tool_policy,
            permissions: AgentPermissions::ALL,
        }
    }
}
