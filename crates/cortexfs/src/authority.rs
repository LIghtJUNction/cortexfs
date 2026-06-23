/// Decides whether an agent may execute a tool through `CTX_PATH`.
///
/// This is a pure effective-authority check for the stable tool boundary:
/// the selected tool must be executable for the agent's Linux identity, visible
/// through a mount that is not `noexec`, and allowed by both the agent policy
/// and the tool object's own policy. Tool schemas, prompts, skills, and MCP
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
    let hit = tool_path
        .find(tool_name)
        .map_err(tool_path_denial)?
        .ok_or(ToolExecutionDenial::ToolNotFound)?;

    let metadata =
        fs::metadata(hit.path()).map_err(|_error| ToolExecutionDenial::CannotInspectTool)?;
    if !linux_identity_can_execute(&metadata, authority.identity) {
        return Err(ToolExecutionDenial::LinuxPermission);
    }

    let mount = most_specific_mount_for_path(authority.mount_table, hit.path())
        .ok_or(ToolExecutionDenial::NotMounted)?;
    if mount.options().contains(&MountOption::NoExec) {
        return Err(ToolExecutionDenial::NoExecMount);
    }

    if !authority.agent_policy.allows(
        authority.agent_subject,
        PolicyObjectClass::Tool,
        tool_name,
        PolicyPermission::Execute,
    ) {
        return Err(ToolExecutionDenial::AgentPolicy);
    }

    if !authority.tool_policy.allows(
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

    let metadata = fs::metadata(path).map_err(|_error| SharedAccessDenial::CannotInspectPath)?;
    let linux_allowed = match access {
        SharedAccess::Read => linux_identity_can_read(&metadata, authority.identity),
        SharedAccess::Write => linux_identity_can_write(&metadata, authority.identity),
    };
    if !linux_allowed {
        return Err(SharedAccessDenial::LinuxPermission);
    }

    if !authority.policy.allows(
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

    let metadata = fs::metadata(path).map_err(|_error| SessionAccessDenial::CannotInspectPath)?;
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
        && !authority.policy.allows(
            authority.agent_subject,
            PolicyObjectClass::Shared,
            shared_name,
            access.shared_policy_permission(),
        )
    {
        return Err(SessionAccessDenial::SharedPolicy);
    }

    if !authority.policy.allows(
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
/// v1 supports only owned children. This check keeps child creation in the
/// ordinary agent object/control-file ABI while proving that the child cannot
/// expand identity, groups, policy, or mount visibility.
pub fn authorize_child_agent(
    request: ChildAgentRequest<'_>,
    authority: ChildAgentAuthority<'_>,
) -> Result<(), ChildAgentDenial> {
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
    if request.lifecycle != ChildLifecycle::Owned {
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

    Ok(())
}

/// Builds the canonical event pair for owned child cancellation caused by
/// parent death.
pub fn owned_child_cancellation_events(
    parent_agent: &str,
    child_agent: &str,
) -> Result<OwnedChildCancellationEvents, OwnedChildCancellationError> {
    if !is_object_name(parent_agent) {
        return Err(OwnedChildCancellationError::InvalidParentName);
    }
    if !is_object_name(child_agent) {
        return Err(OwnedChildCancellationError::InvalidChildName);
    }

    Ok(OwnedChildCancellationEvents {
        parent_event: serde_json::json!({
            "type": "agent.child.cancel",
            "parent": parent_agent,
            "child": child_agent,
            "reason": "parent_dead"
        })
        .to_string(),
        child_event: serde_json::json!({
            "type": "agent.stop",
            "agent": child_agent,
            "status": "cancelled"
        })
        .to_string(),
    })
}

/// Records the durable filesystem effects of cancelling an owned child runtime.
///
/// This function does not supervise or signal processes. It is the auditable
/// state transition a runtime calls after parent death cancellation: child
/// history remains in place, the child session state becomes `cancelled`, and
/// canonical lifecycle events are appended to the existing session logs.
pub fn record_owned_child_cancellation(
    parent_agent: &str,
    child_agent: &str,
    parent_session_dir: &Path,
    child_session_dir: &Path,
) -> Result<OwnedChildCancellationEvents, OwnedChildCancellationError> {
    let events = owned_child_cancellation_events(parent_agent, child_agent)?;
    let parent_events = parent_session_dir.join("events.jsonl");
    let child_messages = child_session_dir.join("messages.jsonl");
    let child_events = child_session_dir.join("events.jsonl");
    let child_state = child_session_dir.join("state");

    if !parent_events.is_file() {
        return Err(OwnedChildCancellationError::MissingParentEvents);
    }
    if !child_messages.is_file() || !child_events.is_file() || !child_state.is_file() {
        return Err(OwnedChildCancellationError::MissingChildHistory);
    }

    atomic_replace_text(&child_state, "cancelled\n")
        .map_err(|_error| OwnedChildCancellationError::CannotRecord)?;
    append_jsonl_event(&parent_events, events.parent_event())
        .map_err(|_error| OwnedChildCancellationError::CannotRecord)?;
    append_jsonl_event(&child_events, events.child_event())
        .map_err(|_error| OwnedChildCancellationError::CannotRecord)?;

    Ok(events)
}

fn parent_ref_matches(value: &str, parent_agent: &str) -> Result<bool, ChildAgentDenial> {
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

    for field in fields {
        let Some((kind, value)) = field.split_once(':') else {
            return Err(ChildAgentDenial::InvalidParentRef);
        };
        if !matches!(kind, "session" | "run") || !is_object_name(value) {
            return Err(ChildAgentDenial::InvalidParentRef);
        }
    }

    Ok(agent_name)
}

fn groups_are_subset(child_groups: &[u32], parent_groups: &[u32]) -> bool {
    child_groups
        .iter()
        .all(|child_group| parent_groups.contains(child_group))
}
