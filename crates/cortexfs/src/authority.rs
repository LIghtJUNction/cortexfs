use self::helpers::{
    is_stable_shared_mount_for, linux_identity_can_execute, linux_identity_can_read,
    linux_identity_can_write, most_specific_mount_for_path, mounted_session_path,
    symlink_safe_metadata, tool_path_denial,
};
use crate::{
    abi::{
        authority::{
            ChildAgentDenial, ChildLifecycle, SessionAccess, SessionAccessDenial, SharedAccess,
            SharedAccessDenial, ToolExecutionDenial, ToolExecutionPrincipal,
        },
        path::is_object_name,
    },
    mount::table::{MountMode, MountOption},
    policy::{PolicyObjectClass, PolicyPermission},
    support::toolpath::ToolPath,
};
use std::path::Path;

mod access;
mod child;
mod helpers;
mod model;
mod network;
mod tool;

pub use access::{
    AccessAuthority, AgentUnixIdentity, SessionAccessAuthority, SharedAccessAuthority,
};
pub use child::{ChildAgentAuthority, ChildAgentControls, ChildAgentRequest};
pub use model::*;
pub use network::*;
pub use tool::{ToolExecutionAuthority, ToolExecutionGrant};

/// Decides whether an agent may execute a tool through `CTX_PATH`.
///
/// This is a pure effective-authority check for the stable tool boundary:
/// the coarse agent permission must admit the selected tool, which must also be
/// executable for the Linux identity, visible on a non-`noexec` mount, and
/// allowed by both agent and tool policy. Tool schemas, prompts, skills, and MCP
/// config files are intentionally not inputs because they never grant
/// authority. Model principals are refused before policy is considered.
pub fn authorize_tool_execution(
    tool_path: &ToolPath,
    tool_name: &str,
    authority: ToolExecutionAuthority<'_>,
) -> Result<ToolExecutionGrant, ToolExecutionDenial> {
    if !is_object_name(tool_name) {
        return Err(ToolExecutionDenial::InvalidToolName);
    }
    if authority.principal == ToolExecutionPrincipal::Model {
        return Err(ToolExecutionDenial::ModelCannotExecute);
    }
    if !authority.permissions.allows_tool(tool_name) {
        return Err(ToolExecutionDenial::AgentPermission);
    }
    let hit = tool_path
        .find(tool_name)
        .map_err(tool_path_denial)?
        .ok_or(ToolExecutionDenial::ToolNotFound)?;

    let metadata = symlink_safe_metadata(hit.path())
        .map_err(|_error| ToolExecutionDenial::CannotInspectTool)?;
    if !metadata.is_file() {
        return Err(ToolExecutionDenial::CannotInspectTool);
    }
    if !linux_identity_can_execute(&metadata, authority.identity) {
        return Err(ToolExecutionDenial::LinuxPermission);
    }

    let mount = most_specific_mount_for_path(authority.mount_table, hit.path())
        .ok_or(ToolExecutionDenial::NotMounted)?;
    if mount.options().contains(&MountOption::NoExec) {
        return Err(ToolExecutionDenial::NoExecMount);
    }

    if !authority.agent_policy.evaluate(
        authority.agent_subject,
        PolicyObjectClass::Tool,
        tool_name,
        PolicyPermission::Execute,
    ) {
        return Err(ToolExecutionDenial::AgentPolicy);
    }

    if !authority.tool_policy.evaluate(
        authority.agent_subject,
        PolicyObjectClass::Tool,
        tool_name,
        PolicyPermission::Execute,
    ) {
        return Err(ToolExecutionDenial::ToolPolicy);
    }

    Ok(ToolExecutionGrant::new(hit))
}

/// Decides whether an agent may access a stable shared-space path.
///
/// Shared access is default-deny and requires all of: a stable shared path for
/// the named space, mount visibility, read-write mount mode for writes, Linux
/// uid/gid/groups/mode permission, and policy v0 permission.
pub fn authorize_shared_access(
    shared_name: &str,
    path: &Path,
    access: SharedAccess,
    authority: SharedAccessAuthority<'_>,
) -> Result<(), SharedAccessDenial> {
    if !is_object_name(shared_name) {
        return Err(SharedAccessDenial::InvalidSharedName);
    }

    let mount = most_specific_mount_for_path(authority.mount_table, path)
        .ok_or(SharedAccessDenial::NotMounted)?;
    if !is_stable_shared_mount_for(mount, shared_name) {
        return Err(SharedAccessDenial::WrongSharedPath);
    }
    if access == SharedAccess::Write && mount.mode() == MountMode::ReadOnly {
        return Err(SharedAccessDenial::ReadOnlyMount);
    }

    let metadata =
        symlink_safe_metadata(path).map_err(|_error| SharedAccessDenial::CannotInspectPath)?;
    let linux_allowed = match access {
        SharedAccess::Read => linux_identity_can_read(&metadata, authority.identity),
        SharedAccess::Write => linux_identity_can_write(&metadata, authority.identity),
    };
    if !linux_allowed {
        return Err(SharedAccessDenial::LinuxPermission);
    }

    if !authority.policy.evaluate(
        authority.agent_subject,
        PolicyObjectClass::Shared,
        shared_name,
        access.policy_permission(),
    ) {
        return Err(SharedAccessDenial::Policy);
    }

    Ok(())
}

/// Decides whether an agent may access a durable private or shared session path.
///
/// Session access is default-deny and requires mount visibility, mount write
/// mode for writes, Linux uid/gid/groups/mode permission, and policy v0
/// `session:<name>` permission. Shared sessions additionally require matching
/// `shared:<space>` policy, so one IM channel cannot read another channel's
/// memory just because both are under `shared/`.
pub fn authorize_session_access(
    path: &Path,
    access: SessionAccess,
    authority: SessionAccessAuthority<'_>,
) -> Result<(), SessionAccessDenial> {
    let mount = most_specific_mount_for_path(authority.mount_table, path)
        .ok_or(SessionAccessDenial::NotMounted)?;
    let session =
        mounted_session_path(mount, path).ok_or(SessionAccessDenial::InvalidSessionPath)?;
    if access == SessionAccess::Write && mount.mode() == MountMode::ReadOnly {
        return Err(SessionAccessDenial::ReadOnlyMount);
    }

    let metadata =
        symlink_safe_metadata(path).map_err(|_error| SessionAccessDenial::CannotInspectPath)?;
    let linux_allowed = match access {
        SessionAccess::Read | SessionAccess::Resume => {
            linux_identity_can_read(&metadata, authority.identity)
        }
        SessionAccess::Write => linux_identity_can_write(&metadata, authority.identity),
    };
    if !linux_allowed || !session.home_uid_allows(authority.identity) {
        return Err(SessionAccessDenial::LinuxPermission);
    }

    if let Some(shared_name) = session.shared_name()
        && !authority.policy.evaluate(
            authority.agent_subject,
            PolicyObjectClass::Shared,
            shared_name,
            access.shared_policy_permission(),
        )
    {
        return Err(SessionAccessDenial::SharedPolicy);
    }

    if !authority.policy.evaluate(
        authority.agent_subject,
        PolicyObjectClass::Session,
        session.session_name(),
        access.policy_permission(),
    ) {
        return Err(SessionAccessDenial::SessionPolicy);
    }

    Ok(())
}

/// Decides whether a requested child agent is attenuated from its parent.
///
/// The stable ABI supports owned and temporary children. This check keeps child creation in
/// the ordinary agent object/control-file ABI while proving that the child
/// cannot expand identity, groups, policy, mount visibility, or tool lookup.
/// The returned tool path is the exact centrally authorized value to
/// materialize for the child.
pub fn authorize_child_agent(
    request: ChildAgentRequest<'_>,
    authority: ChildAgentAuthority<'_>,
) -> Result<ToolPath, ChildAgentDenial> {
    if !is_object_name(request.child_name) {
        return Err(ChildAgentDenial::InvalidChildName);
    }
    if !is_object_name(authority.parent_agent) {
        return Err(ChildAgentDenial::InvalidParentName);
    }
    if !is_object_name(request.controls.subject) || !is_object_name(authority.subject) {
        return Err(ChildAgentDenial::InvalidSubject);
    }
    if !parent_ref_matches(request.parent_ref, authority.parent_agent)? {
        return Err(ChildAgentDenial::ParentMismatch);
    }
    if !matches!(
        request.lifecycle,
        ChildLifecycle::Owned | ChildLifecycle::Temp
    ) {
        return Err(ChildAgentDenial::UnsupportedLifecycle);
    }
    if request.controls.identity.uid() != authority.identity.uid()
        || request.controls.identity.gid() != authority.identity.gid()
    {
        return Err(ChildAgentDenial::IdentityExpansion);
    }
    if !groups_are_subset(
        request.controls.identity.groups(),
        authority.identity.groups(),
    ) {
        return Err(ChildAgentDenial::GroupExpansion);
    }
    if !request.controls.policy.is_authority_subset_of(
        authority.effective_policy,
        request.controls.subject,
        authority.subject,
    ) {
        return Err(ChildAgentDenial::PolicyExpansion);
    }
    if !request
        .controls
        .mounts
        .is_subset_of(authority.visible_mounts)
    {
        return Err(ChildAgentDenial::MountExpansion);
    }
    match request.controls.tool_path {
        None => Ok(authority.tool_path.clone()),
        Some(tool_path)
            if !tool_path.dirs().is_empty()
                && tool_path.is_ordered_subset_of(authority.tool_path) =>
        {
            Ok(tool_path.clone())
        }
        Some(_tool_path) => Err(ChildAgentDenial::ToolPathExpansion),
    }
}

pub(crate) fn parent_ref_matches(
    value: &str,
    parent_agent: &str,
) -> Result<bool, ChildAgentDenial> {
    Ok(parent_ref_agent_name(value)? == parent_agent)
}

pub(crate) fn parent_ref_agent_name(value: &str) -> Result<&str, ChildAgentDenial> {
    let mut fields = value.split_whitespace();
    let Some(agent) = fields.next() else {
        return Err(ChildAgentDenial::InvalidParentRef);
    };
    let Some(agent_name) = agent.strip_prefix("agent:") else {
        return Err(ChildAgentDenial::InvalidParentRef);
    };
    if !is_object_name(agent_name) {
        return Err(ChildAgentDenial::InvalidParentRef);
    }

    let mut session = false;
    let mut run = false;
    for field in fields {
        let Some((kind, value)) = field.split_once(':') else {
            return Err(ChildAgentDenial::InvalidParentRef);
        };
        match kind {
            "session" if is_object_name(value) && !session => session = true,
            "run" if is_object_name(value) && !run => run = true,
            _ => return Err(ChildAgentDenial::InvalidParentRef),
        }
    }

    Ok(agent_name)
}

pub(crate) fn groups_are_subset(child_groups: &[u32], parent_groups: &[u32]) -> bool {
    child_groups
        .iter()
        .all(|child_group| parent_groups.contains(child_group))
}
