use super::access::AgentUnixIdentity;
use crate::{
    abi::authority::ChildLifecycle,
    mount::table::MountTable,
    policy::{PolicyEvaluator, PolicyV0},
    support::toolpath::ToolPath,
};

/// Child control values that carry attenuable authority.
#[derive(Clone, Copy, Debug)]
pub struct ChildAgentControls<'a> {
    pub(crate) identity: &'a AgentUnixIdentity,
    pub(crate) subject: &'a str,
    pub(crate) policy: &'a PolicyV0,
    pub(crate) mounts: &'a MountTable,
    pub(crate) tool_path: Option<&'a ToolPath>,
}

impl<'a> ChildAgentControls<'a> {
    /// Creates child control values from `uid/gid/groups`, `label`, `policy`,
    /// `mount`, and an optional explicit tool-path attenuation. `None` keeps
    /// the parent's canonical path unchanged.
    #[must_use]
    pub const fn new(
        identity: &'a AgentUnixIdentity,
        subject: &'a str,
        policy: &'a PolicyV0,
        mounts: &'a MountTable,
        tool_path: Option<&'a ToolPath>,
    ) -> Self {
        Self {
            identity,
            subject,
            policy,
            mounts,
            tool_path,
        }
    }
}

/// Child-agent creation or validation request.
#[derive(Clone, Copy, Debug)]
pub struct ChildAgentRequest<'a> {
    pub(crate) child_name: &'a str,
    pub(crate) parent_ref: &'a str,
    pub(crate) lifecycle: ChildLifecycle,
    pub(crate) controls: ChildAgentControls<'a>,
}

impl<'a> ChildAgentRequest<'a> {
    /// Creates a child request from ordinary child agent control values.
    #[must_use]
    pub const fn new(
        child_name: &'a str,
        parent_ref: &'a str,
        lifecycle: ChildLifecycle,
        controls: ChildAgentControls<'a>,
    ) -> Self {
        Self {
            child_name,
            parent_ref,
            lifecycle,
            controls,
        }
    }
}

/// Parent effective authority used to attenuate a child agent.
#[derive(Clone, Copy, Debug)]
pub struct ChildAgentAuthority<'a> {
    pub(crate) parent_agent: &'a str,
    pub(crate) identity: &'a AgentUnixIdentity,
    pub(crate) subject: &'a str,
    pub(crate) effective_policy: &'a dyn PolicyEvaluator,
    pub(crate) visible_mounts: &'a MountTable,
    pub(crate) tool_path: &'a ToolPath,
}

impl<'a> ChildAgentAuthority<'a> {
    /// Creates a parent authority context for child attenuation.
    #[must_use]
    pub const fn new(
        parent_agent: &'a str,
        identity: &'a AgentUnixIdentity,
        subject: &'a str,
        effective_policy: &'a dyn PolicyEvaluator,
        visible_mounts: &'a MountTable,
        tool_path: &'a ToolPath,
    ) -> Self {
        Self {
            parent_agent,
            identity,
            subject,
            effective_policy,
            visible_mounts,
            tool_path,
        }
    }
}
